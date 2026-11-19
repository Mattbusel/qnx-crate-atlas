//! QNX Crate Atlas: which of the most downloaded crates.io crates build for QNX 8.0, what breaks
//! the ones that do not, and which crates claim no_std, embedded, QNX or "portable" support they do
//! not have (`atlas claims`, a CI gate that fails on a claim that does not build).

mod claimcheck;
mod claims;
mod classify;
mod cratesio;
mod fix;
mod ontarget;
mod probe;
mod qnxenv;
mod report;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "atlas", version, about = "Cross-compile the top crates.io crates for QNX 8.0 and map what breaks")]
struct Cli {
    /// QNX SDP 8.0 install (default: $QNX_BASE, then ~/qnx/qnx800)
    #[arg(long, global = true)]
    sdp: Option<PathBuf>,
    /// QNX-supplied rustc (default: $QNX_HOST/usr/bin/rustc)
    #[arg(long, global = true)]
    rustc: Option<PathBuf>,
    /// rustup toolchain whose cargo drives the QNX rustc; QNX ships rustc 1.85.1 and no cargo
    #[arg(long, global = true, default_value = "1.85.0")]
    cargo_toolchain: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check the QNX SDP, the QNX rustc, cargo, qcc and the license
    Doctor,
    /// Fetch the most downloaded crates from crates.io
    Top {
        #[arg(long, default_value_t = 500)]
        count: usize,
        #[arg(long, default_value = "data/top.json")]
        out: PathBuf,
    },
    /// Build every crate in the list for each QNX target and append outcomes to results.jsonl
    Run {
        #[arg(long, default_value = "data/top.json")]
        list: PathBuf,
        #[arg(long, value_delimiter = ',', default_values_t = qnxenv::TARGETS.map(String::from))]
        targets: Vec<String>,
        #[arg(long, default_value = "data/results.jsonl")]
        out: PathBuf,
        /// scratch space for probe packages and the shared per-target build cache
        #[arg(long, default_value = "work")]
        work: PathBuf,
        /// rustc jobs per build (CARGO_BUILD_JOBS); builds run one at a time at idle priority
        #[arg(long, default_value_t = 2)]
        jobs: usize,
        /// seconds before a single build is killed
        #[arg(long, default_value_t = 1200)]
        timeout: u64,
        /// only the first N crates of the list
        #[arg(long)]
        limit: Option<usize>,
        /// only these crates
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// rebuild crates that already have a result
        #[arg(long)]
        force: bool,
        /// keep cargo's JSON and stderr for every build under work/probes
        #[arg(long)]
        keep_logs: bool,
    },
    /// Re-build every crate that did not pass out of the box with our patched copies swapped in
    /// (libc compatibility copy plus any --patch NAME=PATH); appends to results-compat.jsonl
    Compat {
        /// directory made by make_libc_compat.py
        #[arg(long)]
        libc: PathBuf,
        /// more patched crates, NAME=PATH (repeatable), e.g. aws-lc-sys=work/aws-lc-sys-qnx
        #[arg(long = "patch")]
        patches: Vec<String>,
        #[arg(long, default_value = "data/results.jsonl")]
        results: PathBuf,
        #[arg(long, default_value = "data/results-compat.jsonl")]
        out: PathBuf,
        #[arg(long, default_value = "work")]
        work: PathBuf,
        #[arg(long, default_value_t = 2)]
        jobs: usize,
        #[arg(long, default_value_t = 1200)]
        timeout: u64,
    },
    /// Re-run the classifier over the saved build logs (after classifier changes) and rewrite results
    Reclassify {
        #[arg(long, default_value = "data/results.jsonl")]
        results: PathBuf,
        #[arg(long, default_value = "data/results-compat.jsonl")]
        compat: PathBuf,
        #[arg(long, default_value = "work")]
        work: PathBuf,
    },
    /// Build one crate for one target and print the outcome
    Probe {
        name: String,
        #[arg(long, default_value = "x86_64-pc-nto-qnx800")]
        target: String,
        #[arg(long, default_value = "work")]
        work: PathBuf,
        #[arg(long, default_value_t = 2)]
        jobs: usize,
    },
    /// Build the unit tests of passing crates for x86_64 QNX and run them on a QNX 8.0 QEMU VM (WSL2)
    Ontarget {
        #[arg(long, default_value = "data/top.json")]
        list: PathBuf,
        #[arg(long, default_value = "data/results.jsonl")]
        results: PathBuf,
        #[arg(long, default_value = "data/ontarget.jsonl")]
        out: PathBuf,
        #[arg(long, default_value = "work")]
        work: PathBuf,
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        #[arg(long, default_value_t = 2)]
        jobs: usize,
        /// WSL distro that holds the QNX VM (see scripts/vm_session.sh)
        #[arg(long, default_value = "Ubuntu-24.04")]
        distro: String,
        /// only build the test binaries, do not boot the VM
        #[arg(long)]
        no_run: bool,
        /// patched crates NAME=PATH applied to the test builds (repeatable); default pins libc 0.2.186
        #[arg(long = "patch")]
        patches: Vec<String>,
    },
    /// Fold results.jsonl into the published matrix
    Report {
        #[arg(long, default_value = "data/top.json")]
        list: PathBuf,
        #[arg(long, default_value = "data/results.jsonl")]
        results: PathBuf,
        #[arg(long, default_value = "data/ontarget.jsonl")]
        ontarget: PathBuf,
        #[arg(long, default_value = "data/results-compat.jsonl")]
        compat: PathBuf,
        #[arg(long, default_value = "site/data/atlas.json")]
        out: PathBuf,
        #[arg(long, value_delimiter = ',', default_values_t = qnxenv::TARGETS.map(String::from))]
        targets: Vec<String>,
    },
    /// Apply the atlas's known QNX fixes to your Cargo project, print what changed, and verify it builds for both QNX targets
    Fix {
        /// project directory (any member of a workspace works; the [patch] goes in the workspace root)
        #[arg(default_value = ".")]
        project: PathBuf,
        /// directory holding the patched copies made by patches/prepare.py (default: $QNX_ATLAS_STORE, then <atlas checkout>/work)
        #[arg(long)]
        store: Option<PathBuf>,
        #[arg(long, value_delimiter = ',', default_values_t = qnxenv::TARGETS.map(String::from))]
        targets: Vec<String>,
        /// pin libc 0.2.186 in Cargo.lock instead of patching in the libc compatibility copy
        #[arg(long)]
        libc_pin: bool,
        /// print the plan, change nothing
        #[arg(long)]
        dry_run: bool,
        /// apply the fixes but do not build
        #[arg(long)]
        no_verify: bool,
        /// also build before applying the fixes, to show the difference
        #[arg(long)]
        before: bool,
        #[arg(long, default_value_t = 8)]
        jobs: usize,
    },
    /// Classify a saved build log (cargo --message-format=json stdout + stderr)
    Classify { stdout: PathBuf, stderr: PathBuf },
    /// Fail when a crate claims no_std, embedded, QNX or portable support that does not build.
    /// Reads categories, keywords, description, #![no_std] and the README, builds every claim for a
    /// target that tests it, and exits 1 with the exact first error when a claim is false.
    Claims {
        /// package directory (holding Cargo.toml); ignored with --crate
        #[arg(default_value = ".")]
        path: PathBuf,
        /// check a published crate instead: NAME or NAME@VERSION
        #[arg(long = "crate")]
        krate: Option<String>,
        #[command(flatten)]
        opts: ClaimOpts,
        /// text, json or github (workflow annotations plus a step summary)
        #[arg(long, default_value = "text")]
        format: String,
        /// also fail on inconclusive checks (no C cross compiler, crate demands a feature choice)
        #[arg(long)]
        strict: bool,
        /// run these checks even without a claim: bare-metal, qnx
        #[arg(long, value_delimiter = ',')]
        also: Vec<String>,
    },
    /// Run the claims check over a list of crates (default: the top 500) and write the claims atlas
    ClaimsAtlas {
        #[arg(long, default_value = "data/top.json")]
        list: PathBuf,
        #[arg(long, default_value = "data/claims.jsonl")]
        out: PathBuf,
        #[arg(long, default_value = "site/data/claims.json")]
        summary: PathBuf,
        #[command(flatten)]
        opts: ClaimOpts,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// re-check crates that already have a line in --out
        #[arg(long)]
        force: bool,
        /// build every crate for QNX, claim or not (the upstream-target column of the atlas)
        #[arg(long)]
        qnx_all: bool,
        /// only rebuild the summary from --out
        #[arg(long)]
        summary_only: bool,
        /// re-read every crate's claims from its unpacked source (after claim-detection changes)
        #[arg(long)]
        reclaim: bool,
    },
}

#[derive(clap::Args, Clone)]
struct ClaimOpts {
    /// bare-metal target for no_std and embedded claims
    #[arg(long, default_value = claimcheck::BARE_METAL_TARGET)]
    bare_metal_target: String,
    /// rustup toolchain for the bare-metal build (needs `rustup target add <target>`)
    #[arg(long, default_value = "stable")]
    bare_metal_toolchain: String,
    /// how to test QNX and portable claims: nightly (upstream x86_64-pc-qnx with -Zbuild-std, any host),
    /// sdp (the QNX SDP 8.0 toolchain, x86_64-pc-nto-qnx800), off
    #[arg(long, default_value = "nightly")]
    qnx: String,
    /// nightly toolchain for --qnx nightly (needs the rust-src component)
    #[arg(long, default_value = "nightly")]
    nightly: String,
    /// QNX target (default x86_64-pc-qnx for nightly, x86_64-pc-nto-qnx800 for sdp)
    #[arg(long)]
    qnx_target: Option<String>,
    #[arg(long, default_value = "work")]
    work: PathBuf,
    #[arg(long, default_value_t = 4)]
    jobs: usize,
    /// seconds before one build is killed
    #[arg(long, default_value_t = 900)]
    timeout: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let env = || qnxenv::QnxEnv::detect(cli.sdp.clone(), cli.rustc.clone(), &cli.cargo_toolchain);
    match &cli.cmd {
        Cmd::Doctor => doctor(&env()?),
        Cmd::Top { count, out } => {
            let list = cratesio::top_crates(*count)?;
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(out, serde_json::to_string_pretty(&list)?)?;
            println!("wrote {} crates to {}", list.len(), out.display());
            Ok(())
        }
        Cmd::Run { list, targets, out, work, jobs, timeout, limit, only, force, keep_logs } => {
            let env = env()?;
            let crates: Vec<cratesio::CrateInfo> = serde_json::from_str(&std::fs::read_to_string(list).with_context(|| format!("reading {}", list.display()))?)?;
            let crates: Vec<_> = crates.into_iter().filter(|c| only.is_empty() || only.contains(&c.name)).take(limit.unwrap_or(usize::MAX)).collect();
            let done = if *force { Default::default() } else { report::latest(report::load_results(out)?) };
            let work = std::path::absolute(work)?;
            let runner = probe::Runner { env: &env, work, jobs: *jobs, timeout: Duration::from_secs(*timeout), keep_logs: *keep_logs };
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out)?;
            let total = crates.len() * targets.len();
            let mut i = 0;
            for c in &crates {
                for t in targets {
                    i += 1;
                    if done.contains_key(&(c.name.clone(), t.clone())) {
                        continue;
                    }
                    let o = match runner.probe(&c.name, c.rank, &c.max_version, t) {
                        Ok(o) => o,
                        Err(e) => {
                            eprintln!("[{i}/{total}] {} {t}: runner error: {e:#}", c.name);
                            continue;
                        }
                    };
                    println!(
                        "[{i}/{total}] #{} {} {} {} {} {}{:.0}s",
                        c.rank,
                        c.name,
                        o.version.as_deref().unwrap_or("?"),
                        t,
                        o.status,
                        o.class.map(|c| format!("{} ", c.slug())).unwrap_or_default(),
                        o.seconds
                    );
                    writeln!(f, "{}", serde_json::to_string(&o)?)?;
                    f.flush()?;
                }
            }
            Ok(())
        }
        Cmd::Compat { libc, patches, results, out, work, jobs, timeout } => {
            let env = env()?;
            let mut all = vec![("libc".to_string(), std::path::absolute(libc)?)];
            for p in patches {
                let (n, path) = p.split_once('=').with_context(|| format!("--patch {p}: expected NAME=PATH"))?;
                all.push((n.to_string(), std::path::absolute(path)?));
            }
            let res = report::latest(report::load_results(results)?);
            let done = report::load_compat(out);
            let runner = probe::Runner { env: &env, work: std::path::absolute(work)?, jobs: *jobs, timeout: Duration::from_secs(*timeout), keep_logs: true };
            let mut todo: Vec<&probe::Outcome> =
                res.values().filter(|o| o.status != "pass" && o.status != "skip").filter(|o| !done.contains_key(&(o.crate_name.clone(), o.target.clone()))).collect();
            todo.sort_by_key(|o| (o.rank, o.target.clone()));
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out)?;
            let n = todo.len();
            for (i, o) in todo.into_iter().enumerate() {
                let (st, cl, fe) = match runner.probe_compat(&o.crate_name, &o.target, &all) {
                    Ok(x) => x,
                    Err(e) => {
                        eprintln!("{} {}: {e:#}", o.crate_name, o.target);
                        continue;
                    }
                };
                if st == "unchanged" {
                    println!("[{}/{n}] #{} {} {} compat: no patched crate in the graph", i + 1, o.rank, o.crate_name, o.target);
                    continue;
                }
                println!("[{}/{n}] #{} {} {} compat {} {}", i + 1, o.rank, o.crate_name, o.target, st, cl.map(|c| c.slug()).unwrap_or(""));
                let line = serde_json::json!({ "crate_name": o.crate_name, "target": o.target, "status": st, "class": cl, "first_error": fe });
                writeln!(f, "{line}")?;
                f.flush()?;
            }
            Ok(())
        }
        Cmd::Reclassify { results, compat, work } => {
            let log = |name: &str, pass: &str, t: &str| -> Option<classify::Verdict> {
                let d = work.join("probes").join(name).join(pass);
                let so = std::fs::read_to_string(d.join(format!("{t}.stdout.jsonl"))).ok()?;
                let se = std::fs::read_to_string(d.join(format!("{t}.stderr.txt"))).ok()?;
                Some(classify::classify(false, &so, &se))
            };
            let mut changed = 0;
            let mut out = String::new();
            for mut o in report::load_results(results)? {
                if o.status == "fail" && !matches!(o.class, Some(classify::Class::Timeout | classify::Class::Resolve | classify::Class::LibcPinConflict)) {
                    let pass = if o.pass1_class.is_some() { "libc-pin" } else { "as-resolved" };
                    if let Some(v) = log(&o.crate_name, pass, &o.target) {
                        if v.class != o.class || v.first_error != o.first_error {
                            changed += 1;
                        }
                        o.class = v.class;
                        o.first_error = v.first_error;
                        if matches!(o.class, Some(classify::Class::NoLibrary | classify::Class::NeedsFeature)) {
                            o.status = "skip".into();
                        }
                    }
                }
                out.push_str(&serde_json::to_string(&o)?);
                out.push('\n');
            }
            std::fs::write(results, out)?;
            let mut cout = String::new();
            for l in std::fs::read_to_string(compat).unwrap_or_default().lines() {
                let Ok(mut v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
                if v["status"] == "fail" && v["class"] != "resolve" && v["class"] != "timeout" {
                    if let Some(r) = log(v["crate_name"].as_str().unwrap_or(""), "libc-compat", v["target"].as_str().unwrap_or("")) {
                        v["class"] = serde_json::to_value(r.class)?;
                        v["first_error"] = serde_json::to_value(r.first_error)?;
                    }
                }
                cout.push_str(&v.to_string());
                cout.push('\n');
            }
            if !cout.is_empty() {
                std::fs::write(compat, cout)?;
            }
            println!("reclassified, {changed} results changed");
            Ok(())
        }
        Cmd::Probe { name, target, work, jobs } => {
            let env = env()?;
            let runner = probe::Runner { env: &env, work: std::path::absolute(work)?, jobs: *jobs, timeout: Duration::from_secs(1800), keep_logs: true };
            let o = runner.probe(name, 0, "", target)?;
            println!("{}", serde_json::to_string_pretty(&o)?);
            Ok(())
        }
        Cmd::Ontarget { list, results, out, work, limit, only, jobs, distro, no_run, patches } => {
            let env = env()?;
            let crates: Vec<cratesio::CrateInfo> = serde_json::from_str(&std::fs::read_to_string(list)?)?;
            let res = report::latest(report::load_results(results)?);
            let mut pv = Vec::new();
            for p in patches {
                let (n, path) = p.split_once('=').with_context(|| format!("--patch {p}: expected NAME=PATH"))?;
                pv.push((n.to_string(), std::path::absolute(path)?));
            }
            let cfg = ontarget::Config { work: std::path::absolute(work)?, jobs: *jobs, distro: distro.clone(), run: !no_run, out: out.clone(), patches: pv };
            ontarget::run(&env, &cfg, &crates, &res, *limit, only)
        }
        Cmd::Report { list, results, ontarget, compat, out, targets } => {
            let crates: Vec<cratesio::CrateInfo> = serde_json::from_str(&std::fs::read_to_string(list)?)?;
            let res = report::load_results(results)?;
            let meta = match env() {
                Ok(e) => serde_json::json!({
                    "rustc": e.rustc_version().unwrap_or_default(),
                    "cargo": e.cargo_version().unwrap_or_default(),
                    "list": "top crates.io crates by all-time downloads",
                    "list_size": crates.len(),
                    "generated_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                    "libc_pin": probe::LIBC_PIN,
                }),
                Err(_) => serde_json::json!({ "list_size": crates.len() }),
            };
            let ot = ontarget::load(ontarget);
            let cp = report::load_compat(compat);
            let v = report::build(&crates, res, &cp, targets, meta, ot);
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(out, serde_json::to_string(&v)?)?;
            for t in targets {
                let s = &v["summary"][t];
                println!("{t}: attempted {} pass {} pass-with-libc-pin {} fail {} skip {}", s["attempted"], s["pass"], s["pass_with_libc_pin"], s["fail"], s["skip"]);
            }
            println!("wrote {}", out.display());
            Ok(())
        }
        Cmd::Fix { project, store, targets, libc_pin, dry_run, no_verify, before, jobs } => {
            let opts = fix::Opts { project, store: store.as_deref(), targets, libc_pin: *libc_pin, dry_run: *dry_run, no_verify: *no_verify, baseline: *before, jobs: *jobs };
            if !fix::fix(&env()?, &opts)? {
                bail!("the project still fails for at least one QNX target (see above)");
            }
            Ok(())
        }
        Cmd::Claims { path, krate, opts, format, strict, also } => {
            let checker = make_checker(opts, &env)?;
            let extra = parse_checks(also)?;
            let (dir, dep, prefix) = match krate {
                Some(k) => {
                    let (name, version) = match k.split_once('@') {
                        Some((n, v)) => (n.to_string(), v.to_string()),
                        None => (k.clone(), cratesio::latest_version(k)?),
                    };
                    let dir = cratesio::fetch_source(&name, &version, &checker.work.join("crates"))?;
                    (dir, claimcheck::Dep::Registry { name, version }, None)
                }
                None => {
                    let dir = std::path::absolute(path)?;
                    let name = claims::read_package(&dir)?.name;
                    (dir.clone(), claimcheck::Dep::Path { name, dir }, Some(path.clone()))
                }
            };
            let pkg = claims::read_package(&dir).with_context(|| format!("reading {}", dir.join("Cargo.toml").display()))?;
            let results = if pkg.claims.is_empty() && extra.is_empty() { Vec::new() } else { checker.run(&pkg, &dep, &extra)? };
            let failed = results.iter().any(|r| r.status == claimcheck::Status::Fail || (*strict && r.status == claimcheck::Status::Inconclusive));
            match format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "package": pkg, "results": results, "failed": failed }))?),
                "github" => {
                    print!("{}", claimcheck::render_text(&pkg, &results));
                    print!("{}", claimcheck::render_github(&pkg, &results, prefix.as_deref(), *strict));
                    if let Some(p) = std::env::var_os("GITHUB_STEP_SUMMARY") {
                        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
                        write!(f, "{}", claimcheck::render_markdown(&pkg, &results))?;
                    }
                }
                _ => print!("{}", claimcheck::render_text(&pkg, &results)),
            }
            if failed {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::ClaimsAtlas { list, out, summary, opts, limit, only, force, qnx_all, summary_only, reclaim } => {
            let crates: Vec<cratesio::CrateInfo> = serde_json::from_str(&std::fs::read_to_string(list).with_context(|| format!("reading {}", list.display()))?)?;
            if !*summary_only {
                let checker = make_checker(opts, &env)?;
                let done: std::collections::HashSet<String> = if *force {
                    Default::default()
                } else {
                    std::fs::read_to_string(out)
                        .unwrap_or_default()
                        .lines()
                        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                        .filter_map(|v| Some(format!("{}@{}", v["crate_name"].as_str()?, v["version"].as_str()?)))
                        .collect()
                };
                if let Some(d) = out.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out)?;
                let todo: Vec<_> = crates.iter().filter(|c| only.is_empty() || only.contains(&c.name)).take(limit.unwrap_or(usize::MAX)).collect();
                let n = todo.len();
                for (i, c) in todo.into_iter().enumerate() {
                    if done.contains(&format!("{}@{}", c.name, c.max_version)) {
                        continue;
                    }
                    let line = match claims_one(&checker, c, *qnx_all) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[{}/{n}] #{} {}: {e:#}", i + 1, c.rank, c.name);
                            continue;
                        }
                    };
                    let verdicts: Vec<String> =
                        line["results"].as_array().into_iter().flatten().map(|r| format!("{}={}", r["check"].as_str().unwrap_or(""), r["status"].as_str().unwrap_or(""))).collect();
                    println!("[{}/{n}] #{} {} {} claims={} {}", i + 1, c.rank, c.name, c.max_version, line["claims"].as_array().map(|a| a.len()).unwrap_or(0), verdicts.join(" "));
                    writeln!(f, "{line}")?;
                    f.flush()?;
                }
            }
            if *reclaim {
                let crates_dir = opts.work.join("crates");
                let mut text = String::new();
                for l in std::fs::read_to_string(out).unwrap_or_default().lines() {
                    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
                    let dir = crates_dir.join(format!("{}-{}", v["crate_name"].as_str().unwrap_or(""), v["version"].as_str().unwrap_or("")));
                    if let Ok(p) = claims::read_package(&dir) {
                        v["claims"] = serde_json::to_value(&p.claims)?;
                    }
                    text.push_str(&v.to_string());
                    text.push('\n');
                }
                std::fs::write(out, text)?;
            }
            let v = claimcheck::summarize(&crates, out)?;
            if let Some(d) = summary.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(summary, serde_json::to_string(&v)?)?;
            println!("{}", serde_json::to_string_pretty(&v["totals"])?);
            println!("wrote {}", summary.display());
            Ok(())
        }
        Cmd::Classify { stdout, stderr } => {
            let v = classify::classify(false, &std::fs::read_to_string(stdout)?, &std::fs::read_to_string(stderr)?);
            println!("{:?}", v);
            Ok(())
        }
    }
}

fn parse_checks(v: &[String]) -> Result<Vec<claims::CheckKind>> {
    v.iter()
        .map(|s| match s.as_str() {
            "bare-metal" | "no-std" | "embedded" => Ok(claims::CheckKind::BareMetal),
            "qnx" | "portable" => Ok(claims::CheckKind::Qnx),
            o => bail!("unknown check {o} (bare-metal, qnx)"),
        })
        .collect()
}

fn make_checker(o: &ClaimOpts, env: &dyn Fn() -> Result<qnxenv::QnxEnv>) -> Result<claimcheck::Checker> {
    let qnx = match o.qnx.as_str() {
        "nightly" => claimcheck::QnxMode::Nightly { toolchain: o.nightly.clone(), target: o.qnx_target.clone().unwrap_or_else(|| claimcheck::QNX_UPSTREAM_TARGET.into()) },
        "sdp" => claimcheck::QnxMode::Sdp { env: Box::new(env()?), target: o.qnx_target.clone().unwrap_or_else(|| qnxenv::TARGETS[0].into()) },
        "off" => claimcheck::QnxMode::Off,
        other => bail!("--qnx {other}: expected nightly, sdp or off"),
    };
    std::fs::create_dir_all(&o.work)?;
    Ok(claimcheck::Checker {
        bare_metal_target: o.bare_metal_target.clone(),
        bare_metal_toolchain: o.bare_metal_toolchain.clone(),
        qnx,
        work: std::path::absolute(&o.work)?,
        jobs: o.jobs,
        timeout: Duration::from_secs(o.timeout),
    })
}

/// Claims check for one published crate; one JSON line of the claims atlas.
fn claims_one(checker: &claimcheck::Checker, c: &cratesio::CrateInfo, qnx_all: bool) -> Result<serde_json::Value> {
    let dir = cratesio::fetch_source(&c.name, &c.max_version, &checker.work.join("crates"))?;
    let pkg = claims::read_package(&dir)?;
    let dep = claimcheck::Dep::Registry { name: c.name.clone(), version: c.max_version.clone() };
    let extra = if qnx_all { vec![claims::CheckKind::Qnx] } else { Vec::new() };
    let results = checker.run(&pkg, &dep, &extra)?;
    Ok(serde_json::json!({
        "crate_name": c.name,
        "rank": c.rank,
        "version": c.max_version,
        "downloads": c.downloads,
        "repository": c.repository,
        "proc_macro": pkg.proc_macro,
        "has_lib": pkg.has_lib,
        "claims": pkg.claims,
        "results": results,
    }))
}

fn doctor(env: &qnxenv::QnxEnv) -> Result<()> {
    println!("QNX SDP      {}", env.base.display());
    println!("QNX_HOST     {}", env.host.display());
    println!("QNX_TARGET   {}", env.target.display());
    let rv = env.rustc_version()?;
    println!("QNX rustc    {}", env.rustc.display());
    for l in rv.lines().filter(|l| l.starts_with("rustc ") || l.starts_with("LLVM")) {
        println!("             {l}");
    }
    let cv = env.cargo_version()?;
    println!("cargo        {cv} (toolchain {})", env.cargo_toolchain);
    let qcc = env.qcc();
    println!("qcc          {} {}", qcc.display(), if qcc.is_file() { "ok" } else { "MISSING" });
    println!("license      {}", if env.license_present() { "present in ~/.qnx/license" } else { "MISSING: activate with QNX Software Center" });
    for t in qnxenv::TARGETS {
        let cfg = env.target_cfg(t)?;
        let pick = |k: &str| cfg.iter().find(|l| l.starts_with(k)).cloned().unwrap_or_default();
        println!("{t:28} {} {} {}", pick("target_os="), pick("target_env="), pick("target_arch="));
    }
    if !qcc.is_file() || !env.license_present() {
        bail!("doctor found problems");
    }
    Ok(())
}
