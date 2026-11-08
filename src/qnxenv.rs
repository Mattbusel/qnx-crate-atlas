//! Finds a QNX SDP 8.0 install and the QNX-supplied Rust compiler, and builds the environment
//! that `qnxsdp-env.bat` / `qnxsdp-env.sh` would set, so cargo can be driven without a shell script.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TARGETS: [&str; 2] = ["x86_64-pc-nto-qnx800", "aarch64-unknown-nto-qnx800"];

#[derive(Debug, Clone)]
pub struct QnxEnv {
    /// SDP root, e.g. C:\Users\me\qnx\qnx800
    pub base: PathBuf,
    /// $QNX_HOST, e.g. <base>/host/win64/x86_64
    pub host: PathBuf,
    /// $QNX_TARGET, <base>/target/qnx
    pub target: PathBuf,
    /// QNX-supplied rustc (ships rust-std for the nto targets)
    pub rustc: PathBuf,
    /// rustup toolchain whose cargo drives the QNX rustc (must match its version closely)
    pub cargo_toolchain: String,
}

fn host_dir() -> &'static str {
    if cfg!(windows) {
        "host/win64/x86_64"
    } else if cfg!(target_os = "macos") {
        "host/darwin/x86_64"
    } else {
        "host/linux/x86_64"
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

impl QnxEnv {
    /// `sdp` comes from --sdp, else $QNX_BASE, else ~/qnx/qnx800.
    pub fn detect(sdp: Option<PathBuf>, rustc: Option<PathBuf>, cargo_toolchain: &str) -> Result<Self> {
        let base = match sdp.or_else(|| std::env::var_os("QNX_BASE").map(PathBuf::from)) {
            Some(p) => p,
            None => home().join("qnx").join("qnx800"),
        };
        if !base.join("target").join("qnx").is_dir() {
            bail!("no QNX SDP at {} (pass --sdp or set QNX_BASE)", base.display());
        }
        let host = host_dir().split('/').fold(base.clone(), |p, c| p.join(c));
        let target = base.join("target").join("qnx");
        let rustc = rustc.unwrap_or_else(|| host.join("usr").join("bin").join(exe("rustc")));
        if !rustc.is_file() {
            bail!(
                "QNX rustc not found at {}. Install the QNX Rust toolchain package from QNX Software Center",
                rustc.display()
            );
        }
        Ok(Self { base, host, target, rustc, cargo_toolchain: cargo_toolchain.to_string() })
    }

    /// Environment variables for a cargo child process.
    pub fn vars(&self) -> Vec<(String, String)> {
        let fwd = |p: &Path| p.display().to_string().replace('\\', "/");
        let mut path = vec![self.host.join("usr").join("bin"), self.base.join("host").join("common").join("bin")];
        if let Some(old) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&old));
        }
        let path = std::env::join_paths(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let qcfg = home().join(".qnx");
        vec![
            ("QNX_BASE".into(), fwd(&self.base)),
            ("QNX_HOST".into(), fwd(&self.host)),
            ("QNX_TARGET".into(), fwd(&self.target)),
            ("QNX_CONFIGURATION_EXCLUSIVE".into(), fwd(&qcfg)),
            ("QNX_CONFIGURATION".into(), fwd(&qcfg)),
            ("MAKEFLAGS".into(), format!("-I{}/usr/include", fwd(&self.target))),
            ("PATH".into(), path),
            ("RUSTC".into(), self.rustc.display().to_string()),
            ("RUSTUP_TOOLCHAIN".into(), self.cargo_toolchain.clone()),
        ]
    }

    /// A `cargo` command with the QNX environment applied and stray outer settings removed.
    pub fn cargo(&self) -> Command {
        let mut c = Command::new("cargo");
        for k in ["RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET"] {
            c.env_remove(k);
        }
        for (k, v) in self.vars() {
            c.env(k, v);
        }
        c.env("CARGO_TERM_COLOR", "never");
        c
    }

    pub fn rustc_version(&self) -> Result<String> {
        let out = Command::new(&self.rustc).arg("-vV").output().context("running QNX rustc")?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn cargo_version(&self) -> Result<String> {
        let out = self.cargo().arg("-V").output().context("running cargo")?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The `cfg` values the QNX compiler reports for a target, e.g. target_os="nto", target_env="nto80".
    pub fn target_cfg(&self, triple: &str) -> Result<Vec<String>> {
        let out = Command::new(&self.rustc).args(["--print", "cfg", "--target", triple]).output()?;
        if !out.status.success() {
            bail!("{} --print cfg --target {triple}: {}", self.rustc.display(), String::from_utf8_lossy(&out.stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
    }

    /// The C compiler driver for a target (qcc), used by the linker step in the nto target specs.
    pub fn qcc(&self) -> PathBuf {
        self.host.join("usr").join("bin").join(exe("qcc"))
    }

    /// Whether a QNX license file is present for this user (QNX tools refuse to run without one).
    pub fn license_present(&self) -> bool {
        let dir = home().join(".qnx").join("license");
        std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false)
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}
