//! Holds a crate to what it claims. A no_std or embedded claim is built for a bare-metal target with
//! no operating system; a QNX or "portable" claim is built for QNX. A claim that does not build is a
//! failure with the exact first error, the crate that produced it, and (when a dependency is to blame)
//! the `cargo tree` path that dragged it in.

use crate::claims::{CheckKind, Claim, PackageClaims};
use crate::classify::{self, Class, FirstError};
use crate::probe::run_cmd;
use crate::qnxenv::QnxEnv;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const BARE_METAL_TARGET: &str = "thumbv7em-none-eabihf";
/// upstream Rust's QNX 8.0 target (tier 3, built with -Zbuild-std on nightly)
pub const QNX_UPSTREAM_TARGET: &str = "x86_64-pc-qnx";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pass,
    Fail,
    /// could not be decided on this host (missing C cross compiler, crate demands a feature choice)
    Inconclusive,
    /// the claim does not apply (proc-macro, no library)
    Skip,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Blame {
    /// the crate's own code
    Crate,
    /// a dependency the crate pulls in
    Dependency,
    /// the compiler or toolchain, not the crate (QNX's rustc vs libc target rename)
    Toolchain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: CheckKind,
    pub target: String,
    pub toolchain: String,
    pub status: Status,
    /// "default-features = false" or "default features"
    pub features: String,
    pub blame: Option<Blame>,
    /// needs-std, explicit-refusal, compile-error, libc-gap, no-qnx-cfg-arm, build-script,
    /// c-toolchain-missing, needs-feature, resolve, timeout
    pub reason: Option<String>,
    pub first_error: Option<FirstError>,
    /// `cargo tree -i` of the blamed dependency: who pulled it in and with which features
    pub why: Option<String>,
    pub seconds: f64,
}

#[derive(Debug, Clone)]
pub enum QnxMode {
    /// nightly rustc with -Zbuild-std for the upstream target; runs on any host, no QNX SDP needed
    Nightly {
        toolchain: String,
        target: String,
    },
    /// the QNX SDP 8.0 toolchain (rustc 1.85.1, x86_64-pc-nto-qnx800)
    Sdp {
        env: Box<QnxEnv>,
        target: String,
    },
    Off,
}

pub struct Checker {
    pub bare_metal_target: String,
    pub bare_metal_toolchain: String,
    pub qnx: QnxMode,
    pub work: PathBuf,
    pub jobs: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub enum Dep {
    Registry { name: String, version: String },
    Path { name: String, dir: PathBuf },
}

impl Dep {
    fn name(&self) -> &str {
        match self {
            Dep::Registry { name, .. } | Dep::Path { name, .. } => name,
        }
    }

    fn spec(&self, default_features: bool, features: &[String]) -> String {
        let mut df = if default_features { String::new() } else { ", default-features = false".to_string() };
        if !features.is_empty() {
            df.push_str(&format!(", features = [{}]", features.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ")));
        }
        match self {
            Dep::Registry { name, version } => format!("probe_dep = {{ package = \"{name}\", version = \"={version}\"{df} }}"),
            Dep::Path { name, dir } => format!("probe_dep = {{ package = \"{name}\", path = '{}'{df} }}", dir.display().to_string().replace('\\', "/")),
        }
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Features a crate offers for no_std use, turned on together for the most generous no_std build.
pub fn no_std_features(features: &[String]) -> Vec<String> {
    let mut v: Vec<String> = features
        .iter()
        .filter(|f| {
            let l = f.to_ascii_lowercase();
            l == "alloc" || l == "libm" || l.contains("no_std") || l.contains("no-std") || l.contains("nostd") || l == "spin" || l == "critical-section"
        })
        .cloned()
        .collect();
    v.sort();
    v
}

fn features_label(default_features: bool, features: &[String]) -> String {
    let mut s = if default_features { "default features".to_string() } else { "default-features = false".to_string() };
    if !features.is_empty() {
        s.push_str(&format!(", features = [{}]", features.join(", ")));
    }
    s
}

/// Reason, blame and status for a failed bare-metal build.
pub fn judge_bare_metal(crate_name: &str, v: &classify::Verdict, stderr: &str) -> (Status, Option<String>, Option<Blame>) {
    let fe = v.first_error.as_ref();
    let pkg = fe.map(|f| f.package.as_str()).unwrap_or("");
    let blame = if pkg.is_empty() || pkg == crate_name || pkg == "atlas-claim-probe" { Blame::Crate } else { Blame::Dependency };
    let msg = fe.map(|f| f.message.as_str()).unwrap_or("");
    let needs_std =
        msg.contains("can't find crate for `std`") || msg.contains("crate `std`") || msg.contains("unlinked crate `std`") || msg.contains("module or unlinked crate `std`");
    if needs_std {
        return (Status::Fail, Some("needs-std".into()), Some(blame));
    }
    if needs_nightly(msg) {
        return (Status::Inconclusive, Some("needs-nightly".into()), None);
    }
    match v.class {
        Some(Class::BuildScriptCc) | Some(Class::BuildScript) | Some(Class::BuildScriptSystemLib) => {
            if c_toolchain_missing(stderr) {
                (Status::Inconclusive, Some("c-toolchain-missing".into()), None)
            } else if needs_config(fe.map(|f| f.message.as_str()).unwrap_or("")) {
                (Status::Inconclusive, Some("needs-config".into()), None)
            } else {
                (Status::Fail, Some("build-script".into()), Some(blame))
            }
        }
        Some(Class::ExplicitlyUnsupported) => (Status::Fail, Some("explicit-refusal".into()), Some(blame)),
        Some(Class::NeedsFeature) => (Status::Inconclusive, Some("needs-feature".into()), None),
        Some(Class::Resolve) => (Status::Inconclusive, Some("resolve".into()), None),
        Some(Class::Timeout) => (Status::Inconclusive, Some("timeout".into()), None),
        Some(Class::RustcTooOld) => (Status::Inconclusive, Some("needs-nightly".into()), None),
        _ => (Status::Fail, Some("compile-error".into()), Some(blame)),
    }
}

/// Reason, blame and status for a failed QNX build.
pub fn judge_qnx(crate_name: &str, v: &classify::Verdict, stderr: &str) -> (Status, Option<String>, Option<Blame>) {
    let fe = v.first_error.as_ref();
    let pkg = fe.map(|f| f.package.as_str()).unwrap_or("");
    let blame = if pkg.is_empty() || pkg == crate_name || pkg == "atlas-claim-probe" { Blame::Crate } else { Blame::Dependency };
    if needs_nightly(fe.map(|f| f.message.as_str()).unwrap_or("")) {
        return (Status::Inconclusive, Some("needs-nightly".into()), None);
    }
    match v.class {
        Some(Class::LibcQnx8Cfg) => (Status::Fail, Some("libc-target-rename".into()), Some(Blame::Toolchain)),
        Some(Class::MissingLibcBinding) => (Status::Fail, Some("libc-gap".into()), Some(blame)),
        Some(Class::NoQnxCfgArm) => (Status::Fail, Some("no-qnx-cfg-arm".into()), Some(blame)),
        Some(Class::ExplicitlyUnsupported) => (Status::Fail, Some("explicit-refusal".into()), Some(blame)),
        Some(Class::BuildScriptCc) | Some(Class::BuildScript) | Some(Class::BuildScriptSystemLib) => {
            if c_toolchain_missing(stderr) {
                (Status::Inconclusive, Some("c-toolchain-missing".into()), None)
            } else if needs_config(fe.map(|f| f.message.as_str()).unwrap_or("")) {
                (Status::Inconclusive, Some("needs-config".into()), None)
            } else {
                (Status::Fail, Some("build-script".into()), Some(blame))
            }
        }
        Some(Class::NeedsFeature) => (Status::Inconclusive, Some("needs-feature".into()), None),
        Some(Class::Resolve) => (Status::Inconclusive, Some("resolve".into()), None),
        Some(Class::Timeout) => (Status::Inconclusive, Some("timeout".into()), None),
        Some(Class::RustcTooOld) => (Status::Inconclusive, Some("rustc-too-old".into()), Some(Blame::Toolchain)),
        Some(Class::CompilerCrash) => (Status::Inconclusive, Some("compiler-crash".into()), Some(Blame::Toolchain)),
        _ => (Status::Fail, Some("compile-error".into()), Some(blame)),
    }
}

/// The feature set chosen turns on unstable Rust (`#![feature]` on a stable compiler, E0554).
pub fn needs_nightly(msg: &str) -> bool {
    msg.contains("may not be used on the stable release channel")
}

/// A build script stopped to ask for configuration (an environment variable, a feature choice).
pub fn needs_config(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("must be specified") || m.contains("environment variable must") || m.contains("please set") || m.contains("cross_python")
}

/// The host has no C compiler for the target: the build script could not even start compiling.
pub fn c_toolchain_missing(stderr: &str) -> bool {
    let s = stderr;
    s.contains("failed to find tool")
        || (s.contains("arm-none-eabi-gcc") && (s.contains("not found") || s.contains("cannot find") || s.contains("program not found")))
        || (s.contains("qcc") && (s.contains("program not found") || s.contains("not found") && s.contains("is `qcc` installed")))
        || s.contains("Is `arm-none-eabi-gcc` installed")
}

impl Checker {
    fn cargo(&self, toolchain: Option<&str>, sdp: Option<&QnxEnv>) -> Command {
        let mut c = match sdp {
            Some(env) => env.cargo(),
            None => {
                let mut c = Command::new("cargo");
                for k in ["RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET"] {
                    c.env_remove(k);
                }
                if let Some(t) = toolchain {
                    c.env("RUSTUP_TOOLCHAIN", t);
                }
                c.env("CARGO_TERM_COLOR", "never");
                c
            }
        };
        c.env("CARGO_BUILD_JOBS", self.jobs.to_string()).env("CARGO_INCREMENTAL", "0").env("CARGO_PROFILE_DEV_DEBUG", "0");
        c
    }

    fn write_probe(dir: &Path, dep: &Dep, default_features: bool, features: &[String], no_std: bool, msrv: Option<&str>) -> Result<()> {
        std::fs::create_dir_all(dir.join("src"))?;
        let msrv = msrv.map(|v| format!("rust-version = \"{v}\"\nresolver = \"3\"\n")).unwrap_or_default();
        let manifest = format!(
            "[package]\nname = \"atlas-claim-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n{msrv}\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\n{}\n\n[workspace]\n",
            dep.spec(default_features, features)
        );
        std::fs::write(dir.join("Cargo.toml"), manifest)?;
        let head = if no_std { "#![no_std]\n" } else { "" };
        std::fs::write(dir.join("src").join("lib.rs"), format!("{head}// atlas claims probe for `{}`\nextern crate probe_dep;\n", dep.name()))?;
        Ok(())
    }

    /// Build the probe; returns (verdict, stderr).
    fn build(&self, dir: &Path, target: &str, toolchain: Option<&str>, sdp: Option<&QnxEnv>, build_std: bool) -> Result<(classify::Verdict, String)> {
        let lock = {
            let mut c = self.cargo(toolchain, sdp);
            c.current_dir(dir).arg("generate-lockfile");
            run_cmd(c, self.timeout)?
        };
        if !lock.0 {
            return Ok((classify::classify(false, "", &lock.2), lock.2));
        }
        let mut c = self.cargo(toolchain, sdp);
        c.current_dir(dir).args(["build", "--lib", "--message-format=json", "--target", target]);
        if build_std {
            c.arg("-Zbuild-std=std,panic_abort");
        }
        c.env("CARGO_TARGET_DIR", self.work.join("claims-target").join(sanitize(&format!("{}-{target}", toolchain.unwrap_or("sdp")))));
        let (ok, out, err, to) = run_cmd(c, self.timeout)?;
        if to {
            return Ok((classify::Verdict { ok: false, class: Some(Class::Timeout), first_error: None }, err));
        }
        Ok((classify::classify(ok, &out, &err), err))
    }

    fn why(&self, dir: &Path, target: &str, toolchain: Option<&str>, sdp: Option<&QnxEnv>, fe: &FirstError) -> Option<String> {
        if fe.package.is_empty() {
            return None;
        }
        let spec = if fe.version.is_empty() { fe.package.clone() } else { format!("{}@{}", fe.package, fe.version) };
        let mut c = self.cargo(toolchain, sdp);
        c.current_dir(dir).args(["tree", "-i", &spec, "-e", "normal,features", "--target", target, "--charset", "ascii"]);
        let (ok, out, _e, _) = run_cmd(c, Duration::from_secs(120)).ok()?;
        if !ok || out.trim().is_empty() {
            return None;
        }
        Some(tidy_tree(&out))
    }

    fn skip(check: CheckKind, target: &str, why: &str) -> CheckResult {
        CheckResult {
            check,
            target: target.into(),
            toolchain: String::new(),
            status: Status::Skip,
            features: String::new(),
            blame: None,
            reason: Some(why.into()),
            first_error: None,
            why: None,
            seconds: 0.0,
        }
    }

    pub fn check_bare_metal(&self, pkg: &PackageClaims, dep: &Dep) -> Result<CheckResult> {
        let target = self.bare_metal_target.as_str();
        if pkg.proc_macro {
            return Ok(Self::skip(CheckKind::BareMetal, target, "proc-macro (runs on the host)"));
        }
        if !pkg.has_lib {
            return Ok(Self::skip(CheckKind::BareMetal, target, "no library target"));
        }
        if let Some(scope) = &pkg.platform_scope {
            return Ok(Self::skip(CheckKind::BareMetal, target, &format!("OS-specific crate ({scope}); a bare-metal build does not test its claim")));
        }
        let start = Instant::now();
        let tc = Some(self.bare_metal_toolchain.as_str());
        let base = self.work.join("claims").join(sanitize(&format!("{}-{}", pkg.name, pkg.version)));
        // 1. default-features = false, the documented no_std setup; 2. plus the crate's own no_std
        // features (alloc, no_std, libm, ...), the most generous no_std setup; 3. default features,
        // for crates that are no_std by default. A pass in any of them passes the claim; a failure
        // is reported from the most generous no_std setup that ran.
        let mut nsf = no_std_features(&pkg.features);
        for f in &pkg.no_std_features {
            if !nsf.contains(f) && pkg.features.contains(f) {
                nsf.push(f.clone());
            }
        }
        let mut variants: Vec<(bool, Vec<String>, &str)> = vec![(false, Vec::new(), "bare-metal-no-default")];
        if !nsf.is_empty() {
            variants.push((false, nsf.clone(), "bare-metal-no-std-features"));
        }
        if pkg.has_default_features {
            variants.push((true, Vec::new(), "bare-metal-default"));
        }
        let mut report: Option<(classify::Verdict, String, PathBuf, String)> = None;
        for (df, feats, sub) in variants {
            let dir = base.join(sub);
            Self::write_probe(&dir, dep, df, &feats, true, None)?;
            let (v, err) = self.build(&dir, target, tc, None, false)?;
            let label = features_label(df, &feats);
            if v.ok {
                return Ok(CheckResult {
                    check: CheckKind::BareMetal,
                    target: target.into(),
                    toolchain: self.bare_metal_toolchain.clone(),
                    status: Status::Pass,
                    features: label,
                    blame: None,
                    reason: None,
                    first_error: None,
                    why: None,
                    seconds: start.elapsed().as_secs_f64(),
                });
            }
            // keep the first failure, replaced by the no_std-features failure when that ran
            if report.is_none() || (!df && !feats.is_empty()) {
                report = Some((v, err, dir, label));
            }
        }
        let (v, err, dir, label) = report.expect("at least one variant ran");
        let (status, reason, blame) = judge_bare_metal(&pkg.name, &v, &err);
        let why = match (&blame, &v.first_error) {
            (Some(Blame::Dependency), Some(fe)) => self.why(&dir, target, tc, None, fe),
            _ => None,
        };
        Ok(CheckResult {
            check: CheckKind::BareMetal,
            target: target.into(),
            toolchain: self.bare_metal_toolchain.clone(),
            status,
            features: label,
            blame,
            reason,
            first_error: v.first_error,
            why,
            seconds: start.elapsed().as_secs_f64(),
        })
    }

    pub fn check_qnx(&self, pkg: &PackageClaims, dep: &Dep) -> Result<CheckResult> {
        let (target, toolchain, sdp, build_std, msrv) = match &self.qnx {
            QnxMode::Off => return Ok(Self::skip(CheckKind::Qnx, "", "QNX check turned off")),
            QnxMode::Nightly { toolchain, target } => (target.clone(), Some(toolchain.as_str()), None, true, None),
            QnxMode::Sdp { env, target } => (target.clone(), None, Some(env.as_ref()), false, Some("1.85")),
        };
        if pkg.proc_macro {
            return Ok(Self::skip(CheckKind::Qnx, &target, "proc-macro (runs on the host)"));
        }
        if !pkg.has_lib {
            return Ok(Self::skip(CheckKind::Qnx, &target, "no library target"));
        }
        let start = Instant::now();
        let dir = self.work.join("claims").join(sanitize(&format!("{}-{}", pkg.name, pkg.version))).join(sanitize(&format!("qnx-{target}")));
        Self::write_probe(&dir, dep, true, &[], false, msrv)?;
        let (v, err) = self.build(&dir, &target, toolchain, sdp, build_std)?;
        let tc_name = toolchain.map(str::to_string).unwrap_or_else(|| "qnx-sdp-8.0".into());
        if v.ok {
            return Ok(CheckResult {
                check: CheckKind::Qnx,
                target,
                toolchain: tc_name,
                status: Status::Pass,
                features: "default features".into(),
                blame: None,
                reason: None,
                first_error: None,
                why: None,
                seconds: start.elapsed().as_secs_f64(),
            });
        }
        let (status, reason, blame) = judge_qnx(&pkg.name, &v, &err);
        let why = match (&blame, &v.first_error) {
            (Some(Blame::Dependency), Some(fe)) => self.why(&dir, &target, toolchain, sdp, fe),
            _ => None,
        };
        Ok(CheckResult {
            check: CheckKind::Qnx,
            target,
            toolchain: tc_name,
            status,
            features: "default features".into(),
            blame,
            reason,
            first_error: v.first_error,
            why,
            seconds: start.elapsed().as_secs_f64(),
        })
    }

    pub fn run(&self, pkg: &PackageClaims, dep: &Dep, extra: &[CheckKind]) -> Result<Vec<CheckResult>> {
        let mut checks = pkg.checks();
        for c in extra {
            if !checks.contains(c) {
                checks.push(*c);
            }
        }
        checks.sort();
        let mut out = Vec::new();
        for c in checks {
            out.push(match c {
                CheckKind::BareMetal => self.check_bare_metal(pkg, dep)?,
                CheckKind::Qnx => self.check_qnx(pkg, dep)?,
            });
        }
        Ok(out)
    }
}

/// `cargo tree -i` output without the probe package itself, capped at 24 lines.
pub fn tidy_tree(out: &str) -> String {
    let lines: Vec<String> = out
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.contains("atlas-claim-probe feature"))
        .map(|l| match l.find("atlas-claim-probe v0.0.0") {
            Some(i) => format!("{}(the crate's user)", &l[..i]),
            None => l.to_string(),
        })
        .collect();
    let mut s = lines.iter().take(24).cloned().collect::<Vec<_>>().join("\n");
    if lines.len() > 24 {
        s.push_str(&format!("\n... ({} more lines)", lines.len() - 24));
    }
    s
}

/// One line saying what the claim is and where the build broke.
pub fn headline(pkg: &PackageClaims, r: &CheckResult) -> String {
    let claim = pkg.for_check(r.check).first().map(|c| claim_label(c)).unwrap_or_else(|| "(no claim, checked on request)".into());
    let fe = r.first_error.as_ref();
    let at = fe.map(error_location).unwrap_or_default();
    let blame = match r.blame {
        Some(Blame::Crate) => "in this crate".to_string(),
        Some(Blame::Dependency) => format!("in dependency {}", fe.map(|f| format!("{} {}", f.package, f.version)).unwrap_or_default()),
        Some(Blame::Toolchain) => "in the toolchain, not this crate".into(),
        None => String::new(),
    };
    format!(
        "{} claims {} ({}) but does not build for {} with {}: {} {}{}",
        pkg.name,
        pkg.for_check(r.check).first().map(|c| c.kind.slug()).unwrap_or("-"),
        claim,
        r.target,
        r.features,
        r.reason.as_deref().unwrap_or("error"),
        blame,
        if at.is_empty() { String::new() } else { format!(": {at}") }
    )
}

pub fn claim_label(c: &Claim) -> String {
    match c.line {
        Some(l) => format!("{}:{l} {}", c.file, c.text),
        None => format!("{} {}", c.file, c.text),
    }
}

pub fn error_location(fe: &FirstError) -> String {
    let code = fe.code.as_deref().map(|c| format!("error[{c}]")).unwrap_or_else(|| "error".into());
    let loc = match (&fe.file, fe.line) {
        (Some(f), Some(l)) => format!(" at {}-{}/{f}:{l}", fe.package, fe.version),
        (Some(f), None) => format!(" at {}-{}/{f}", fe.package, fe.version),
        _ if !fe.package.is_empty() => format!(" in {} {}", fe.package, fe.version),
        _ => String::new(),
    };
    format!("{code}: {}{loc}", fe.message)
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "PASS",
        Status::Fail => "FAIL",
        Status::Inconclusive => "????",
        Status::Skip => "skip",
    }
}

fn check_word(c: CheckKind) -> &'static str {
    match c {
        CheckKind::BareMetal => "bare-metal",
        CheckKind::Qnx => "qnx",
    }
}

/// Human report: every claim with its file:line, every check with the first error.
pub fn render_text(pkg: &PackageClaims, results: &[CheckResult]) -> String {
    let mut s = String::new();
    if pkg.claims.is_empty() && results.is_empty() {
        s.push_str(&format!("{} {}: claims no no_std, embedded, QNX or portable support; nothing to hold it to.\n", pkg.name, pkg.version));
        return s;
    }
    s.push_str(&format!("{} {}: {} portability claim(s)\n", pkg.name, pkg.version, pkg.claims.len()));
    for c in &pkg.claims {
        s.push_str(&format!("  claim {:9} {}\n", c.kind.slug(), claim_label(c)));
    }
    for r in results {
        let tc = if r.toolchain.is_empty() { String::new() } else { format!(" [{}]", r.toolchain) };
        let feats = if r.features.is_empty() { String::new() } else { format!(" with {}", r.features) };
        s.push_str(&format!("  {} {:10} {}{tc}{feats}", status_word(r.status), check_word(r.check), r.target));
        if let Some(reason) = &r.reason {
            let blame = match r.blame {
                Some(Blame::Crate) => ", in this crate",
                Some(Blame::Dependency) => ", in a dependency",
                Some(Blame::Toolchain) => ", in the toolchain",
                None => "",
            };
            s.push_str(&format!(": {reason}{blame}"));
        }
        s.push('\n');
        if let Some(fe) = &r.first_error {
            s.push_str(&format!("       {}\n", error_location(fe)));
        }
        if let Some(w) = &r.why {
            s.push_str("       pulled in by (cargo tree -i, with features):\n");
            for l in w.lines() {
                s.push_str(&format!("         {l}\n"));
            }
        }
    }
    let broken = results.iter().filter(|r| r.status == Status::Fail).count();
    let unknown = results.iter().filter(|r| r.status == Status::Inconclusive).count();
    if broken > 0 {
        s.push_str(&format!("result: {broken} claim check(s) FAILED. The crate says it runs there; the compiler says it does not.\n"));
    } else if unknown > 0 {
        s.push_str(&format!("result: no broken claim, {unknown} check(s) could not be decided on this host\n"));
    } else {
        s.push_str("result: every claim builds\n");
    }
    s
}

fn esc_data(s: &str) -> String {
    s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

fn esc_prop(s: &str) -> String {
    esc_data(s).replace(':', "%3A").replace(',', "%2C")
}

/// GitHub Actions workflow commands: an error annotation on the line that makes the claim.
/// `prefix` is the package directory inside the checked-out repository; `None` (a published crate
/// checked with `--crate`) annotates without a file, since the claim's file is not in the repository.
pub fn render_github(pkg: &PackageClaims, results: &[CheckResult], prefix: Option<&Path>, strict: bool) -> String {
    let mut s = String::new();
    for r in results {
        let level = match r.status {
            Status::Fail => "error",
            Status::Inconclusive if strict => "error",
            Status::Inconclusive => "warning",
            _ => continue,
        };
        let claim = pkg.for_check(r.check).into_iter().next();
        let file = claim.and_then(|c| match prefix {
            Some(p) if p != Path::new(".") => Some(format!("{}/{}", p.display().to_string().replace('\\', "/").trim_end_matches('/'), c.file)),
            Some(_) => Some(c.file.clone()),
            None => None,
        });
        let mut props = Vec::new();
        if let Some(f) = &file {
            props.push(format!("file={}", esc_prop(f)));
            if let Some(l) = claim.and_then(|c| c.line) {
                props.push(format!("line={l}"));
            }
        }
        let kind = claim.map(|c| c.kind.slug()).unwrap_or(check_word(r.check));
        let title = match r.status {
            Status::Fail => format!("Broken {kind} claim"),
            _ => format!("Unverified {kind} claim"),
        };
        props.push(format!("title={}", esc_prop(&title)));
        let mut msg = headline(pkg, r);
        if let Some(w) = &r.why {
            msg.push_str("\nPulled in by:\n");
            msg.push_str(w);
        }
        s.push_str(&format!("::{level} {}::{}\n", props.join(","), esc_data(&msg)));
    }
    s
}

/// Markdown table for $GITHUB_STEP_SUMMARY.
pub fn render_markdown(pkg: &PackageClaims, results: &[CheckResult]) -> String {
    let mut s = format!("### Portability claims: {} {}\n\n", pkg.name, pkg.version);
    if results.is_empty() {
        s.push_str("No no_std, embedded, QNX or portable claim found.\n\n");
        return s;
    }
    s.push_str("| Check | Target | Result | Claim | First error |\n|---|---|---|---|---|\n");
    for r in results {
        let claim = pkg.for_check(r.check).first().map(|c| format!("`{}`", claim_label(c).replace('|', "\\|"))).unwrap_or_default();
        let err = r.first_error.as_ref().map(|f| format!("`{}`", error_location(f).replace('|', "\\|"))).unwrap_or_default();
        let res = match (r.status, &r.reason) {
            (Status::Pass, _) => "pass".to_string(),
            (st, Some(reason)) => format!("{} ({reason})", status_word(st).to_ascii_lowercase()),
            (st, None) => status_word(st).to_ascii_lowercase(),
        };
        s.push_str(&format!("| {} | `{}` | {res} | {claim} | {err} |\n", check_word(r.check), r.target));
    }
    s.push('\n');
    s
}

fn tool_version(toolchain: &str) -> String {
    Command::new("rustc").env("RUSTUP_TOOLCHAIN", toolchain).arg("-V").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn claims_check(kind: &str, check: &str) -> bool {
    match check {
        "bare-metal" => kind == "no-std" || kind == "embedded",
        _ => kind == "qnx" || kind == "portable",
    }
}

/// Fold the claims atlas lines into the published summary: totals, every broken claim, and the
/// packages that break the most claims of other crates.
pub fn summarize(list: &[crate::cratesio::CrateInfo], jsonl: &Path) -> Result<serde_json::Value> {
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, HashMap};
    let ranks: HashMap<&str, usize> = list.iter().map(|c| (c.name.as_str(), c.rank)).collect();
    let mut latest: BTreeMap<usize, Value> = BTreeMap::new();
    for l in std::fs::read_to_string(jsonl).unwrap_or_default().lines() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let Some(&rank) = v["crate_name"].as_str().and_then(|n| ranks.get(n)) else { continue };
        latest.insert(rank, v);
    }
    let mut claim_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_check: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut unclaimed_qnx: BTreeMap<String, usize> = BTreeMap::new();
    let mut blockers: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut broken = Vec::new();
    let mut with_claims = 0;
    let mut crates_broken = 0;
    let mut crates_hard = 0;
    let mut crates_refuse = 0;
    for v in latest.values() {
        let claims = v["claims"].as_array().cloned().unwrap_or_default();
        let mut kinds: Vec<String> = claims.iter().filter_map(|c| c["kind"].as_str().map(str::to_string)).collect();
        kinds.sort();
        kinds.dedup();
        if !kinds.is_empty() {
            with_claims += 1;
        }
        for k in &kinds {
            *claim_kinds.entry(k.clone()).or_default() += 1;
        }
        let mut any_broken = false;
        let mut any_hard = false;
        let mut any_refuse = false;
        for r in v["results"].as_array().into_iter().flatten() {
            let check = r["check"].as_str().unwrap_or("").to_string();
            let status = r["status"].as_str().unwrap_or("").to_string();
            let claim = claims.iter().find(|c| claims_check(c["kind"].as_str().unwrap_or(""), &check)).cloned();
            let Some(claim) = claim else {
                *unclaimed_qnx.entry(status).or_default() += 1;
                continue;
            };
            *per_check.entry(check.clone()).or_default().entry(status.clone()).or_default() += 1;
            if status != "fail" {
                continue;
            }
            any_broken = true;
            if r["reason"] == "explicit-refusal" {
                any_refuse = true;
            } else {
                any_hard = true;
            }
            let fe = &r["first_error"];
            let culprit = fe["package"].as_str().unwrap_or("").to_string();
            if !culprit.is_empty() {
                let e = blockers.entry(culprit).or_default();
                e.0 += 1;
                e.1.push(v["crate_name"].as_str().unwrap_or("").to_string());
            }
            broken.push(json!({
                "rank": v["rank"], "crate_name": v["crate_name"], "version": v["version"], "downloads": v["downloads"], "repository": v["repository"],
                "check": check, "target": r["target"], "toolchain": r["toolchain"], "features": r["features"],
                "claim": claim, "reason": r["reason"], "blame": r["blame"], "first_error": fe, "why": r["why"],
            }));
        }
        crates_broken += usize::from(any_broken);
        crates_hard += usize::from(any_hard);
        crates_refuse += usize::from(any_refuse && !any_hard);
    }
    let mut top_blockers: Vec<_> = blockers.into_iter().map(|(k, (n, c))| json!({ "package": k, "broken_claims": n, "crates": c })).collect();
    top_blockers.sort_by(|a, b| b["broken_claims"].as_u64().cmp(&a["broken_claims"].as_u64()));
    Ok(json!({
        "meta": {
            "generated_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            "list": "top crates.io crates by all-time downloads",
            "list_size": list.len(),
            "bare_metal_target": BARE_METAL_TARGET,
            "qnx_target": QNX_UPSTREAM_TARGET,
            "stable": tool_version("stable"),
            "nightly": tool_version("nightly"),
        },
        "totals": {
            "crates_scanned": latest.len(),
            "crates_with_claims": with_claims,
            "crates_with_a_broken_claim": crates_broken,
            "crates_where_a_claim_fails_to_compile": crates_hard,
            "crates_that_refuse_the_target_on_purpose": crates_refuse,
            "claims_by_kind": claim_kinds,
            "claimed_checks": per_check,
            "qnx_unclaimed": unclaimed_qnx,
        },
        "broken": broken,
        "blockers": top_blockers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(pkg: &str, code: Option<&str>, msg: &str, class: Class) -> classify::Verdict {
        classify::Verdict {
            ok: false,
            class: Some(class),
            first_error: Some(FirstError {
                package: pkg.into(),
                version: "1.0.0".into(),
                code: code.map(str::to_string),
                message: msg.into(),
                file: Some("src/lib.rs".into()),
                kind: Some("lib".into()),
                line: Some(1),
            }),
        }
    }

    #[test]
    fn std_in_a_dependency_blames_the_dependency() {
        let v = verdict("memchr", Some("E0463"), "can't find crate for `std`", Class::NoLibrary);
        assert_eq!(judge_bare_metal("mycrate", &v, ""), (Status::Fail, Some("needs-std".into()), Some(Blame::Dependency)));
        let v = verdict("mycrate", Some("E0433"), "failed to resolve: use of unresolved module or unlinked crate `std`", Class::NoQnxCfgArm);
        assert_eq!(judge_bare_metal("mycrate", &v, ""), (Status::Fail, Some("needs-std".into()), Some(Blame::Crate)));
    }

    #[test]
    fn missing_cross_compiler_is_inconclusive() {
        let v = verdict("ring", None, "failed to find tool \"arm-none-eabi-gcc\"", Class::BuildScriptCc);
        assert_eq!(judge_bare_metal("ring", &v, "error occurred in cc-rs: failed to find tool \"arm-none-eabi-gcc\": program not found").0, Status::Inconclusive);
        let v = verdict("ring", None, "fatal error: 'stdint.h' file not found", Class::BuildScriptCc);
        assert_eq!(judge_bare_metal("ring", &v, "fatal error: 'stdint.h' file not found").0, Status::Fail);
    }

    #[test]
    fn qnx_libc_gap_blames_the_crate_that_uses_it() {
        let v = verdict("socket2", Some("E0432"), "unresolved imports `libc::IPV6_ADD_MEMBERSHIP`", Class::MissingLibcBinding);
        assert_eq!(judge_qnx("socket2", &v, ""), (Status::Fail, Some("libc-gap".into()), Some(Blame::Crate)));
        let v = verdict("libc", Some("E0412"), "cannot find type `in_addr`", Class::LibcQnx8Cfg);
        assert_eq!(judge_qnx("tokio", &v, "").2, Some(Blame::Toolchain));
    }

    #[test]
    fn registry_and_path_specs() {
        let d = Dep::Registry { name: "serde".into(), version: "1.0.228".into() };
        assert_eq!(d.spec(false, &[]), "probe_dep = { package = \"serde\", version = \"=1.0.228\", default-features = false }");
        assert_eq!(d.spec(false, &["alloc".to_string()]), "probe_dep = { package = \"serde\", version = \"=1.0.228\", default-features = false, features = [\"alloc\"] }");
        let d = Dep::Path { name: "x".into(), dir: PathBuf::from("C:\\a\\b") };
        assert_eq!(d.spec(true, &[]), "probe_dep = { package = \"x\", path = 'C:/a/b' }");
    }

    #[test]
    fn error_location_has_file_and_line() {
        let fe = FirstError {
            package: "socket2".into(),
            version: "0.6.5".into(),
            code: Some("E0432".into()),
            message: "unresolved imports".into(),
            file: Some("src/sys/unix.rs".into()),
            kind: None,
            line: Some(281),
        };
        assert_eq!(error_location(&fe), "error[E0432]: unresolved imports at socket2-0.6.5/src/sys/unix.rs:281");
    }

    #[test]
    fn no_std_feature_pick() {
        let f: Vec<String> = ["std", "alloc", "serde", "spin_no_std", "default", "libm"].iter().map(|s| s.to_string()).collect();
        assert_eq!(no_std_features(&f), vec!["alloc", "libm", "spin_no_std"]);
    }

    #[test]
    fn tree_drops_the_probe() {
        let t = "memchr v2.8.3\n`-- memchr feature \"default\"\n    `-- atlas-claim-probe v0.0.0 (C:\\w\\p)\n        `-- atlas-claim-probe feature \"default\" (command-line)\n";
        assert_eq!(tidy_tree(t), "memchr v2.8.3\n`-- memchr feature \"default\"\n    `-- (the crate's user)");
    }

    #[test]
    fn github_annotations_point_at_the_claim() {
        let claim = Claim {
            kind: crate::claims::ClaimKind::NoStd,
            source: crate::claims::Source::Category,
            file: "Cargo.toml".into(),
            line: Some(8),
            text: "categories = [\"no-std\"]".into(),
        };
        let pkg = PackageClaims { name: "x".into(), version: "0.1.0".into(), claims: vec![claim], has_lib: true, ..Default::default() };
        let r = CheckResult {
            check: CheckKind::BareMetal,
            target: BARE_METAL_TARGET.into(),
            toolchain: "stable".into(),
            status: Status::Fail,
            features: "default-features = false".into(),
            blame: Some(Blame::Crate),
            reason: Some("needs-std".into()),
            first_error: None,
            why: None,
            seconds: 0.0,
        };
        let local = render_github(&pkg, std::slice::from_ref(&r), Some(Path::new("crates/x")), false);
        assert!(local.starts_with("::error file=crates/x/Cargo.toml,line=8,title=Broken no-std claim::"), "{local}");
        let published = render_github(&pkg, &[r], None, false);
        assert!(published.starts_with("::error title=Broken no-std claim::"), "{published}");
    }
}
