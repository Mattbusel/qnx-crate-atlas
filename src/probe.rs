//! Builds one crate for one QNX target inside a throwaway "probe" package, the way a QNX user would
//! pull it in: as a dependency of a binary that is compiled and linked with qcc.
//!
//! Pass 1 builds the graph exactly as cargo resolves it (newest versions that support rustc 1.85).
//! If libc 0.2.189 or newer is in the graph and the build fails, pass 2 pins libc to 0.2.186, the
//! last release that still carries the `target_env = "nto80"` cfg, and builds again.

use crate::classify::{self, Class, FirstError};
use crate::qnxenv::QnxEnv;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const LIBC_PIN: &str = "0.2.186";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub crate_name: String,
    pub rank: usize,
    pub target: String,
    /// version cargo selected (newest compatible with rust-version 1.85)
    pub version: Option<String>,
    pub latest: String,
    /// pass | pass-with-libc-pin | fail | skip
    pub status: String,
    pub class: Option<Class>,
    pub first_error: Option<FirstError>,
    /// class seen on pass 1 when pass 2 changed the result
    pub pass1_class: Option<Class>,
    pub libc: Option<String>,
    pub packages: usize,
    pub seconds: f64,
    pub at: String,
}

pub struct Runner<'a> {
    pub env: &'a QnxEnv,
    pub work: PathBuf,
    pub jobs: usize,
    pub timeout: Duration,
    pub keep_logs: bool,
}

fn ident(name: &str) -> String {
    name.replace('-', "_")
}

fn now_iso() -> String {
    // seconds since epoch is enough to order runs; keep the format stable and dependency free
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{s}")
}

impl<'a> Runner<'a> {
    fn probe_dir(&self, name: &str, pass: &str) -> PathBuf {
        self.work.join("probes").join(name).join(pass)
    }

    fn target_dir(&self, target: &str) -> PathBuf {
        self.work.join("target").join(target)
    }

    fn write_probe(&self, dir: &Path, name: &str) -> Result<()> {
        std::fs::create_dir_all(dir.join("src"))?;
        let manifest = format!(
            "[package]\nname = \"atlas-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.85\"\nresolver = \"3\"\npublish = false\n\n\
             [dependencies]\nprobe_dep = {{ package = \"{name}\", version = \"*\" }}\n\n[workspace]\n"
        );
        std::fs::write(dir.join("Cargo.toml"), manifest)?;
        std::fs::write(
            dir.join("src").join("main.rs"),
            format!("// QNX Crate Atlas probe for `{name}`: link the crate into a QNX binary.\nextern crate probe_dep;\n\nfn main() {{\n    println!(\"{} linked for QNX\");\n}}\n", ident(name)),
        )?;
        Ok(())
    }

    /// Run a command with a time limit, capturing stdout and stderr. Returns (success, stdout, stderr, timed_out).
    fn run(&self, mut cmd: Command) -> Result<(bool, String, String, bool)> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // IDLE_PRIORITY_CLASS: children (rustc, qcc) inherit it, so the PC stays responsive
            cmd.creation_flags(0x0000_0040);
        }
        let mut child = cmd.spawn().context("spawning cargo")?;
        let mut so = child.stdout.take().unwrap();
        let mut se = child.stderr.take().unwrap();
        let t1 = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = so.read_to_string(&mut s);
            s
        });
        let t2 = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = se.read_to_string(&mut s);
            s
        });
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(st) = child.try_wait()? {
                break Some(st);
            }
            if start.elapsed() > self.timeout {
                timed_out = true;
                kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let out = t1.join().unwrap_or_default();
        let err = t2.join().unwrap_or_default();
        Ok((status.map(|s| s.success()).unwrap_or(false), out, err, timed_out))
    }

    /// (version of the probed crate, newest libc in the graph, number of packages excluding the probe)
    fn lock_info(dir: &Path) -> (Option<String>, Option<String>, usize) {
        let Ok(text) = std::fs::read_to_string(dir.join("Cargo.lock")) else { return (None, None, 0) };
        let Ok(v) = text.parse::<toml::Table>() else { return (None, None, 0) };
        let pkgs = v.get("package").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        let field = |p: &toml::Value, k: &str| p.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let vkey = |v: &String| v.split(['.', '-', '+']).map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
        let libc = pkgs.iter().filter(|p| field(p, "name").as_deref() == Some("libc")).filter_map(|p| field(p, "version")).max_by_key(vkey);
        // the probe's only dependency entry is "name" or "name version" (when two versions are in the graph)
        let dep = pkgs
            .iter()
            .find(|p| field(p, "name").as_deref() == Some("atlas-probe"))
            .and_then(|p| p.get("dependencies"))
            .and_then(|d| d.as_array())
            .and_then(|d| d.first())
            .and_then(|d| d.as_str())
            .map(str::to_string);
        let version = dep.and_then(|d| {
            let mut it = d.split_whitespace();
            let name = it.next()?.to_string();
            if let Some(v) = it.next() {
                return Some(v.to_string());
            }
            pkgs.iter().find(|p| field(p, "name").as_deref() == Some(name.as_str())).and_then(|p| field(p, "version"))
        });
        (version, libc, pkgs.len().saturating_sub(1))
    }

    fn generate_lock(&self, dir: &Path) -> Result<(bool, String)> {
        let mut c = self.env.cargo();
        c.current_dir(dir).arg("generate-lockfile");
        let (ok, _o, e, _) = self.run(c)?;
        Ok((ok, e))
    }

    fn build(&self, dir: &Path, target: &str) -> Result<(bool, String, String, bool)> {
        let mut c = self.env.cargo();
        c.current_dir(dir)
            .args(["build", "--locked", "--message-format=json", "--target", target])
            .env("CARGO_TARGET_DIR", self.target_dir(target))
            .env("CARGO_BUILD_JOBS", self.jobs.to_string())
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_PROFILE_DEV_DEBUG", "0");
        self.run(c)
    }

    fn save_log(&self, dir: &Path, target: &str, out: &str, err: &str) {
        if self.keep_logs {
            let _ = std::fs::write(dir.join(format!("{target}.stdout.jsonl")), out);
            let _ = std::fs::write(dir.join(format!("{target}.stderr.txt")), err);
        }
    }

    /// Prepare both passes' probe dirs and lockfiles once per crate; returns any resolve failure.
    pub fn prepare(&self, name: &str) -> Result<Option<FirstError>> {
        let p1 = self.probe_dir(name, "as-resolved");
        if p1.join("Cargo.lock").is_file() {
            return Ok(None);
        }
        self.write_probe(&p1, name)?;
        let (ok, err) = self.generate_lock(&p1)?;
        if !ok {
            let v = classify::classify(false, "", &err);
            let mut fe = v.first_error.unwrap_or(FirstError { package: name.into(), version: String::new(), code: None, message: "resolve failed".into(), file: None, kind: None });
            if fe.package.is_empty() {
                fe.package = name.into();
            }
            let _ = std::fs::remove_file(p1.join("Cargo.lock"));
            return Ok(Some(fe));
        }
        Ok(None)
    }

    pub fn probe(&self, name: &str, rank: usize, latest: &str, target: &str) -> Result<Outcome> {
        let start = Instant::now();
        let mut o = Outcome {
            crate_name: name.into(),
            rank,
            target: target.into(),
            version: None,
            latest: latest.into(),
            status: "fail".into(),
            class: None,
            first_error: None,
            pass1_class: None,
            libc: None,
            packages: 0,
            seconds: 0.0,
            at: now_iso(),
        };
        if let Some(fe) = self.prepare(name)? {
            o.class = Some(Class::Resolve);
            o.first_error = Some(fe);
            o.seconds = start.elapsed().as_secs_f64();
            return Ok(o);
        }
        let p1 = self.probe_dir(name, "as-resolved");
        let (ver, libc, n) = Self::lock_info(&p1);
        o.version = ver;
        o.libc = libc.clone();
        o.packages = n;

        let (ok, out, err, to) = self.build(&p1, target)?;
        self.save_log(&p1, target, &out, &err);
        let v = if to { classify::Verdict { ok: false, class: Some(Class::Timeout), first_error: None } } else { classify::classify(ok, &out, &err) };
        if v.ok {
            o.status = "pass".into();
            o.seconds = start.elapsed().as_secs_f64();
            return Ok(o);
        }
        o.class = v.class;
        o.first_error = v.first_error;

        let libc_new = libc.as_deref().map(classify::libc_needs_pin).unwrap_or(false);
        if libc_new && v.class != Some(Class::Timeout) {
            let p2 = self.probe_dir(name, "libc-pin");
            if !p2.join("Cargo.lock").is_file() {
                std::fs::create_dir_all(&p2)?;
                self.write_probe(&p2, name)?;
                std::fs::copy(p1.join("Cargo.lock"), p2.join("Cargo.lock"))?;
                let mut c = self.env.cargo();
                c.current_dir(&p2).args(["update", "-p", "libc", "--precise", LIBC_PIN]);
                let (ok, _o, e, _) = self.run(c)?;
                if !ok {
                    let _ = std::fs::remove_file(p2.join("Cargo.lock"));
                    o.pass1_class = o.class;
                    o.class = Some(Class::LibcPinConflict);
                    o.first_error = Some(FirstError {
                        package: "libc".into(),
                        version: libc.clone().unwrap_or_default(),
                        code: None,
                        message: e.lines().map(str::trim).find(|l| l.starts_with("error")).unwrap_or("libc pin failed").chars().take(240).collect(),
                        file: None,
                        kind: None,
                    });
                    o.seconds = start.elapsed().as_secs_f64();
                    return Ok(o);
                }
            }
            let (v2ver, v2libc, _) = Self::lock_info(&p2);
            if v2ver.is_some() {
                o.version = v2ver;
            }
            o.libc = v2libc;
            let (ok2, out2, err2, to2) = self.build(&p2, target)?;
            self.save_log(&p2, target, &out2, &err2);
            let v2 = if to2 { classify::Verdict { ok: false, class: Some(Class::Timeout), first_error: None } } else { classify::classify(ok2, &out2, &err2) };
            o.pass1_class = o.class;
            if v2.ok {
                o.status = "pass-with-libc-pin".into();
                o.class = None;
                o.first_error = None;
            } else {
                o.class = v2.class;
                o.first_error = v2.first_error;
            }
        }
        if matches!(o.class, Some(Class::NoLibrary | Class::NeedsFeature)) {
            o.status = "skip".into();
        }
        o.seconds = start.elapsed().as_secs_f64();
        Ok(o)
    }

    /// Pass 3: same graph as pass 1, with patched copies swapped in through `[patch.crates-io]`:
    /// libc from patches/make_libc_compat.py, plus any extra (name, path) fixes such as the
    /// aws-lc-sys and target-lexicon changes sent upstream. Returns (status, class, first error).
    pub fn probe_compat(&self, name: &str, target: &str, patches: &[(String, PathBuf)]) -> Result<(String, Option<Class>, Option<FirstError>)> {
        let p1 = self.probe_dir(name, "as-resolved");
        let p3 = self.probe_dir(name, "libc-compat");
        if !p3.join("Cargo.lock").is_file() {
            std::fs::create_dir_all(&p3)?;
            self.write_probe(&p3, name)?;
            let mut m = std::fs::read_to_string(p3.join("Cargo.toml"))?;
            m.push_str("\n[patch.crates-io]\n");
            for (n, p) in patches {
                let path = p.display().to_string().replace('\\', "/");
                m.push_str(&format!("{n} = {{ path = '{path}' }}\n"));
            }
            std::fs::write(p3.join("Cargo.toml"), m)?;
            std::fs::copy(p1.join("Cargo.lock"), p3.join("Cargo.lock"))?;
            let lock = std::fs::read_to_string(p3.join("Cargo.lock")).unwrap_or_default();
            let present: Vec<&str> = patches.iter().map(|(n, _)| n.as_str()).filter(|n| lock.contains(&format!("name = \"{n}\""))).collect();
            // re-point the patched packages at the path copies; nothing else in the graph moves
            let mut c = self.env.cargo();
            c.current_dir(&p3).arg("update").arg("--offline");
            for n in &present {
                c.args(["-p", n]);
            }
            if present.is_empty() {
                // none of the patched crates is in this graph, so the result cannot change
                let _ = std::fs::remove_dir_all(&p3);
                return Ok(("unchanged".into(), None, None));
            }
            let (ok, _o, e, _) = self.run(c)?;
            if !ok {
                let mut c = self.env.cargo();
                c.current_dir(&p3).arg("generate-lockfile");
                let (ok2, _o, e2, _) = self.run(c)?;
                if !ok2 {
                    let _ = std::fs::remove_file(p3.join("Cargo.lock"));
                    let msg: String = format!("{e}{e2}").lines().map(str::trim).find(|l| l.starts_with("error")).unwrap_or("compat resolve failed").chars().take(240).collect();
                    return Ok(("fail".into(), Some(Class::Resolve), Some(FirstError { package: "libc".into(), version: String::new(), code: None, message: msg, file: None, kind: None })));
                }
            }
        }
        let (ok, out, err, to) = self.build(&p3, target)?;
        self.save_log(&p3, target, &out, &err);
        if to {
            return Ok(("fail".into(), Some(Class::Timeout), None));
        }
        let v = classify::classify(ok, &out, &err);
        if v.ok {
            return Ok(("pass".into(), None, None));
        }
        Ok(("fail".into(), v.class, v.first_error))
    }
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("pkill").args(["-TERM", "-P", &pid.to_string()]).status();
    }
}
