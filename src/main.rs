//! QNX Crate Atlas: which of the most downloaded crates.io crates build for QNX 8.0, and what breaks
//! the ones that do not.

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
            let crates: Vec<_> = crates
                .into_iter()
                .filter(|c| only.is_empty() || only.contains(&c.name))
                .take(limit.unwrap_or(usize::MAX))
                .collect();
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
            let mut todo: Vec<&probe::Outcome> = res
                .values()
                .filter(|o| o.status != "pass" && o.status != "skip")
                .filter(|o| !done.contains_key(&(o.crate_name.clone(), o.target.clone())))
                .collect();
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
                println!(
                    "{t}: attempted {} pass {} pass-with-libc-pin {} fail {} skip {}",
                    s["attempted"], s["pass"], s["pass_with_libc_pin"], s["fail"], s["skip"]
                );
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
        Cmd::Classify { stdout, stderr } => {
            let v = classify::classify(false, &std::fs::read_to_string(stdout)?, &std::fs::read_to_string(stderr)?);
            println!("{:?}", v);
            Ok(())
        }
    }
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
