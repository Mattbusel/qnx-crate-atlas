//! On-target stage: for crates that build for x86_64 QNX, build their own unit tests with the QNX
//! toolchain, then run every test binary on a real QNX 8.0 kernel (QEMU/KVM inside WSL2).

use crate::classify::{self, libc_needs_pin};
use crate::cratesio::CrateInfo;
use crate::probe::{Outcome, LIBC_PIN};
use crate::qnxenv::QnxEnv;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TARGET: &str = "x86_64-pc-nto-qnx800";

pub struct Config {
    pub work: PathBuf,
    pub jobs: usize,
    pub distro: String,
    pub run: bool,
    pub out: PathBuf,
    /// patched copies (name, path) applied with [patch.crates-io]; without them libc is pinned to 0.2.186
    pub patches: Vec<(String, PathBuf)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRun {
    pub crate_name: String,
    pub version: String,
    pub rank: usize,
    /// built | build-failed | no-tests | ran
    pub stage: String,
    pub build_error: Option<String>,
    pub binaries: usize,
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
    /// libtest exit codes per binary ("0", "101", "timeout")
    pub exit_codes: Vec<String>,
    pub failed_tests: Vec<String>,
    pub uname: Option<String>,
}

pub fn load(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut latest: HashMap<String, TestRun> = HashMap::new();
    for l in text.lines() {
        if let Ok(t) = serde_json::from_str::<TestRun>(l) {
            latest.insert(t.crate_name.clone(), t);
        }
    }
    let mut v: Vec<TestRun> = latest.into_values().collect();
    v.sort_by_key(|t| t.rank);
    Some(serde_json::to_value(v).ok()?)
}

fn registry_src(name: &str, version: &str) -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| crate::qnxenv::home().join(".cargo"));
    let reg = home.join("registry").join("src");
    for d in std::fs::read_dir(reg).ok()?.flatten() {
        let p = d.path().join(format!("{name}-{version}"));
        if p.join("Cargo.toml").is_file() {
            return Some(p);
        }
    }
    None
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        let dst = to.join(e.file_name());
        if p.is_dir() {
            if e.file_name() == "target" {
                continue;
            }
            copy_dir(&p, &dst)?;
        } else {
            std::fs::copy(&p, &dst)?;
        }
    }
    Ok(())
}

fn to_wsl(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.len() > 2 && s.as_bytes()[1] == b':' {
        format!("/mnt/{}{}", s[..1].to_ascii_lowercase(), &s[2..])
    } else {
        s
    }
}

fn idle(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0040);
    }
    let _ = cmd;
}

/// Build the crate's lib unit tests for QNX and return the test executables.
fn build_tests(env: &QnxEnv, cfg: &Config, name: &str, version: &str) -> Result<std::result::Result<Vec<PathBuf>, String>> {
    let src = registry_src(name, version).with_context(|| format!("{name}-{version} not in the cargo registry cache (run the matrix first)"))?;
    let dir = cfg.work.join("ontarget").join(format!("{name}-{version}"));
    if !dir.join("Cargo.toml").is_file() {
        copy_dir(&src, &dir)?;
        // a standalone workspace, so cargo never walks up into another one
        let mut m = std::fs::read_to_string(dir.join("Cargo.toml"))?;
        if !m.contains("\n[workspace]") {
            m.push_str("\n[workspace]\n");
        }
        if !cfg.patches.is_empty() && !m.contains("[patch.crates-io]") {
            m.push_str("\n[patch.crates-io]\n");
            for (n, p) in &cfg.patches {
                // a crate never patches itself
                if n != name {
                    m.push_str(&format!("{n} = {{ path = '{}' }}\n", p.display().to_string().replace('\\', "/")));
                }
            }
        }
        std::fs::write(dir.join("Cargo.toml"), m)?;
        let _ = std::fs::remove_file(dir.join("Cargo.lock"));
    }
    let tdir = cfg.work.join("target-tests");
    if !dir.join("Cargo.lock").is_file() {
        let mut c = env.cargo();
        c.current_dir(&dir).arg("generate-lockfile").env("CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS", "fallback");
        let o = c.output()?;
        if !o.status.success() {
            return Ok(Err(String::from_utf8_lossy(&o.stderr).lines().find(|l| l.starts_with("error")).unwrap_or("resolve failed").to_string()));
        }
        let lock = std::fs::read_to_string(dir.join("Cargo.lock")).unwrap_or_default();
        let newest_libc = lock
            .split("[[package]]")
            .filter(|b| b.contains("name = \"libc\""))
            .filter_map(|b| b.lines().find(|l| l.starts_with("version = ")).map(|l| l.trim_start_matches("version = ").trim_matches('"').to_string()))
            .any(|v| libc_needs_pin(&v));
        if newest_libc && cfg.patches.is_empty() {
            let mut c = env.cargo();
            c.current_dir(&dir).args(["update", "-p", "libc", "--precise", LIBC_PIN]);
            let _ = c.output();
        }
    }
    let mut c = env.cargo();
    c.current_dir(&dir)
        .args(["test", "--lib", "--no-run", "--message-format=json", "--target", TARGET])
        .env("CARGO_TARGET_DIR", &tdir)
        .env("CARGO_BUILD_JOBS", cfg.jobs.to_string())
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .stdin(Stdio::null());
    idle(&mut c);
    let o = c.output()?;
    let out = String::from_utf8_lossy(&o.stdout);
    if !o.status.success() {
        let v = classify::classify(false, &out, &String::from_utf8_lossy(&o.stderr));
        let msg = v
            .first_error
            .map(|e| format!("{}: {}{}", if e.package.is_empty() { name.to_string() } else { e.package }, e.code.map(|c| format!("{c} ")).unwrap_or_default(), e.message))
            .unwrap_or_else(|| "test build failed".into());
        return Ok(Err(msg));
    }
    let mut exes = Vec::new();
    for l in out.lines() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        // proc-macro test binaries are built for the host and run there, not on QNX
        let proc_macro = v["target"]["kind"].as_array().map(|k| k.iter().any(|x| x == "proc-macro")).unwrap_or(false);
        if v["reason"] == "compiler-artifact" && v["profile"]["test"] == true && !proc_macro {
            if let Some(e) = v["executable"].as_str() {
                exes.push(PathBuf::from(e));
            }
        }
    }
    Ok(Ok(exes))
}

/// libtest summary line: "test result: ok. 12 passed; 0 failed; 1 ignored; ..."
fn parse_summary(text: &str) -> (u64, u64, u64, Vec<String>) {
    let (mut p, mut f, mut i) = (0, 0, 0);
    let mut failed = Vec::new();
    for l in text.lines() {
        if let Some(rest) = l.trim().strip_prefix("test result: ") {
            for part in rest.split(';') {
                let w: Vec<&str> = part.split_whitespace().collect();
                let num = |idx: usize| w.get(idx).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
                if part.contains(" passed") {
                    p += num(w.len().saturating_sub(2));
                } else if part.contains(" failed") {
                    f += num(0);
                } else if part.contains(" ignored") {
                    i += num(0);
                }
            }
        }
        if let Some(t) = l.trim().strip_prefix("test ") {
            if let Some(name) = t.strip_suffix(" ... FAILED") {
                failed.push(name.to_string());
            }
        }
    }
    (p, f, i, failed)
}

pub fn run(env: &QnxEnv, cfg: &Config, crates: &[CrateInfo], res: &HashMap<(String, String), Outcome>, limit: usize, only: &[String]) -> Result<()> {
    let mut picked = Vec::new();
    for c in crates {
        if picked.len() >= limit {
            break;
        }
        if !only.is_empty() && !only.contains(&c.name) {
            continue;
        }
        let Some(o) = res.get(&(c.name.clone(), TARGET.to_string())) else { continue };
        if !o.status.starts_with("pass") {
            continue;
        }
        let Some(v) = o.version.clone() else { continue };
        picked.push((c, v));
    }
    let mut runs: Vec<(TestRun, Vec<PathBuf>)> = Vec::new();
    for (c, v) in &picked {
        let mut t = TestRun {
            crate_name: c.name.clone(),
            version: v.clone(),
            rank: c.rank,
            stage: "built".into(),
            build_error: None,
            binaries: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            exit_codes: vec![],
            failed_tests: vec![],
            uname: None,
        };
        match build_tests(env, cfg, &c.name, v) {
            Ok(Ok(exes)) if exes.is_empty() => t.stage = "no-tests".into(),
            Ok(Ok(exes)) => {
                t.binaries = exes.len();
                println!("built tests  {} {} ({} binaries)", c.name, v, exes.len());
                runs.push((t, exes));
                continue;
            }
            Ok(Err(e)) => {
                t.stage = "build-failed".into();
                t.build_error = Some(e.chars().take(300).collect());
            }
            Err(e) => {
                t.stage = "build-failed".into();
                t.build_error = Some(format!("{e:#}"));
            }
        }
        println!("{:12} {} {} {}", t.stage, c.name, v, t.build_error.clone().unwrap_or_default());
        runs.push((t, vec![]));
    }

    if cfg.run {
        let manifest = cfg.work.join("ontarget-manifest.txt");
        let mut m = String::new();
        for (t, exes) in &runs {
            for (k, e) in exes.iter().enumerate() {
                m.push_str(&format!("{}#{} {}\n", t.crate_name, k, to_wsl(e)));
            }
        }
        std::fs::write(&manifest, m)?;
        let script = std::path::absolute("scripts/ontarget_session.sh")?;
        println!("booting QNX 8.0 VM in WSL ({}) and running {} test binaries", cfg.distro, runs.iter().map(|r| r.1.len()).sum::<usize>());
        let out = Command::new("wsl")
            .args(["-d", &cfg.distro, "-u", "root", "--", "bash", &to_wsl(&script), &to_wsl(&manifest)])
            .stdin(Stdio::null())
            .output()
            .context("running wsl")?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        std::fs::write(cfg.work.join("ontarget-session.log"), &text)?;
        let uname = text.lines().find_map(|l| l.strip_prefix("@@VM ")).map(str::to_string);
        // split per binary
        let mut blocks: HashMap<String, Vec<String>> = HashMap::new();
        let mut cur: Option<String> = None;
        for l in text.lines() {
            if let Some(label) = l.strip_prefix("@@BEGIN ") {
                cur = Some(label.trim().to_string());
                continue;
            }
            if l.starts_with("@@END ") {
                cur = None;
                continue;
            }
            if let Some(c) = &cur {
                blocks.entry(c.clone()).or_default().push(l.to_string());
            }
        }
        for (t, exes) in runs.iter_mut() {
            if exes.is_empty() {
                continue;
            }
            t.stage = "ran".into();
            t.uname = uname.clone();
            for k in 0..exes.len() {
                let lines = blocks.get(&format!("{}#{}", t.crate_name, k)).cloned().unwrap_or_default();
                let body = lines.join("\n");
                let (p, f, i, failed) = parse_summary(&body);
                t.passed += p;
                t.failed += f;
                t.ignored += i;
                t.failed_tests.extend(failed.into_iter().take(20));
                let rc = lines.iter().rev().find_map(|l| l.strip_prefix("@@RC=")).unwrap_or("missing").to_string();
                t.exit_codes.push(rc);
            }
            println!("ran on QNX   {} {}: {} passed, {} failed, {} ignored, exit {:?}", t.crate_name, t.version, t.passed, t.failed, t.ignored, t.exit_codes);
        }
    }

    if let Some(d) = cfg.out.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&cfg.out)?;
    for (t, _) in &runs {
        if cfg.run || t.stage != "built" {
            writeln!(f, "{}", serde_json::to_string(t)?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libtest_summary() {
        let out = "running 3 tests\ntest a ... ok\ntest b ... FAILED\ntest c ... ignored\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        let (p, f, i, names) = parse_summary(out);
        assert_eq!((p, f, i), (1, 1, 1));
        assert_eq!(names, vec!["b".to_string()]);
    }

    #[test]
    fn wsl_paths() {
        assert_eq!(to_wsl(Path::new("C:\\Users\\me\\x")), "/mnt/c/Users/me/x");
    }
}
