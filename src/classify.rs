//! Turns one cargo build (JSON messages on stdout, human text on stderr) into a first error and a
//! failure class. The classes are chosen so each one maps to a different fix for someone porting to QNX.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirstError {
    /// crate that failed to compile (the blocker), may be a transitive dependency
    pub package: String,
    pub version: String,
    /// rustc error code such as E0412, if any
    pub code: Option<String>,
    /// one-line message
    pub message: String,
    /// primary span file, trimmed to the path inside the crate
    pub file: Option<String>,
    /// lib, proc-macro, custom-build, bin
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    /// libc 0.2.189+ gates QNX 8 on target_os = "qnx"; the QNX toolchain reports target_os = "nto"
    LibcQnx8Cfg,
    /// a crate calls a libc item that libc does not define for nto
    MissingLibcBinding,
    /// the crate's own cfg tree has no arm for QNX (missing item, unresolved import)
    NoQnxCfgArm,
    /// the crate rejects the target on purpose with compile_error!
    ExplicitlyUnsupported,
    /// a build script failed while compiling C/C++ (cc-rs)
    BuildScriptCc,
    /// a build script needed pkg-config, a system library or cmake
    BuildScriptSystemLib,
    /// any other build script failure
    BuildScript,
    /// needs a newer rustc than the 1.85 that QNX ships (MSRV, unstable feature)
    RustcTooOld,
    /// linking with qcc failed or a native library is missing
    Link,
    /// dependency resolution or download failed
    Resolve,
    /// the libc 0.2.186 pin conflicts with the crate's own libc requirement
    LibcPinConflict,
    /// rustc itself crashed (internal compiler error)
    CompilerCrash,
    /// package has no library target (binary-only crate), excluded from rates
    NoLibrary,
    /// the crate refuses to build with default features only (a probe artifact), excluded from rates
    NeedsFeature,
    Timeout,
    Other,
}

impl Class {
    pub fn slug(self) -> &'static str {
        match self {
            Class::LibcQnx8Cfg => "libc-qnx8-cfg",
            Class::MissingLibcBinding => "missing-libc-binding",
            Class::NoQnxCfgArm => "no-qnx-cfg-arm",
            Class::ExplicitlyUnsupported => "explicitly-unsupported",
            Class::BuildScriptCc => "build-script-cc",
            Class::BuildScriptSystemLib => "build-script-system-lib",
            Class::BuildScript => "build-script",
            Class::RustcTooOld => "rustc-too-old",
            Class::Link => "link",
            Class::Resolve => "resolve",
            Class::LibcPinConflict => "libc-pin-conflict",
            Class::CompilerCrash => "compiler-crash",
            Class::NoLibrary => "no-library",
            Class::NeedsFeature => "needs-feature",
            Class::Timeout => "timeout",
            Class::Other => "other",
        }
    }

    pub fn all() -> &'static [Class] {
        use Class::*;
        &[
            LibcQnx8Cfg, MissingLibcBinding, NoQnxCfgArm, ExplicitlyUnsupported, BuildScriptCc, BuildScriptSystemLib,
            BuildScript, RustcTooOld, Link, Resolve, LibcPinConflict, CompilerCrash, NoLibrary, NeedsFeature, Timeout, Other,
        ]
    }

    pub fn describe(self) -> &'static str {
        match self {
            Class::LibcQnx8Cfg => "libc 0.2.189+ only knows QNX 8 as target_os = \"qnx\"; the QNX toolchain says target_os = \"nto\", target_env = \"nto80\"",
            Class::MissingLibcBinding => "the crate uses a libc item that libc does not define for QNX",
            Class::NoQnxCfgArm => "the crate's platform code has no branch for QNX",
            Class::ExplicitlyUnsupported => "the crate stops the build on purpose for this target",
            Class::BuildScriptCc => "a build script failed compiling C or C++ for QNX",
            Class::BuildScriptSystemLib => "a build script looked for a system library, pkg-config or cmake",
            Class::BuildScript => "a build script failed",
            Class::RustcTooOld => "needs a newer rustc than the 1.85 QNX ships",
            Class::Link => "linking with qcc failed",
            Class::Resolve => "cargo could not resolve or download the dependency graph",
            Class::LibcPinConflict => "the crate requires libc 0.2.189 or newer, so the 0.2.186 pin cannot apply",
            Class::CompilerCrash => "the QNX rustc crashed (internal compiler error)",
            Class::NoLibrary => "binary-only package, nothing to link against",
            Class::NeedsFeature => "the crate needs a feature enabled to build at all; not a QNX problem",
            Class::Timeout => "build did not finish inside the time limit",
            Class::Other => "other compile error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub ok: bool,
    pub class: Option<Class>,
    pub first_error: Option<FirstError>,
}

/// Parse a cargo package id in either the old ("libc 0.2.1 (registry+...)") or new
/// ("registry+https://...#libc@0.2.1") format into (name, version).
pub fn parse_package_id(id: &str) -> (String, String) {
    if let Some(frag) = id.rsplit('#').next().filter(|_| id.contains('#')) {
        if let Some((n, v)) = frag.split_once('@') {
            return (n.to_string(), v.to_string());
        }
        // path dependencies: ".../atlas-probe#0.0.0" -> name is the last path segment
        let name = id.split('#').next().unwrap_or("").rsplit(['/', '\\']).next().unwrap_or("").to_string();
        return (name, frag.to_string());
    }
    let mut it = id.split_whitespace();
    (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
}

fn version_tuple(v: &str) -> (u64, u64, u64) {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let mut p = core.split('.').map(|x| x.parse::<u64>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0), p.next().unwrap_or(0))
}

pub fn libc_needs_pin(version: &str) -> bool {
    version_tuple(version) >= (0, 2, 189)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().chars().take(240).collect()
}

fn trim_file(f: &str) -> String {
    let f = f.replace('\\', "/");
    // registry paths look like .../index.crates.io-xxxx/foo-1.2.3/src/lib.rs
    if let Some(i) = f.find("index.crates.io-") {
        let rest = &f[i..];
        if let Some(j) = rest.find('/') {
            let rest = &rest[j + 1..];
            if let Some(k) = rest.find('/') {
                return rest[k + 1..].to_string();
            }
        }
    }
    f.rsplit("/src/").next().map(|t| format!("src/{t}")).unwrap_or(f)
}

/// Classify a compiler error that rustc reported for some crate.
fn class_for_compiler_error(fe: &FirstError, rendered: &str) -> Class {
    let msg = fe.message.as_str();
    let r = rendered;
    if fe.package == "libc" && libc_needs_pin(&fe.version) {
        return Class::LibcQnx8Cfg;
    }
    if msg.starts_with("linking with") || msg.contains("linker `") || msg.contains("could not find native static library") {
        return Class::Link;
    }
    let unstable = fe.code.as_deref() == Some("E0658")
        || msg.contains("is unstable")
        || msg.contains("unstable library feature")
        || msg.contains("requires rustc")
        || msg.contains("is experimental")
        || msg.contains("edition 2024");
    if unstable {
        return Class::RustcTooOld;
    }
    let lower = msg.to_ascii_lowercase();
    if fe.code.is_none() && (lower.contains("you need to specify at least one") || lower.contains("feature")) && (lower.contains("need to specify") || lower.contains("must be enabled") || lower.contains("at least one") || lower.contains("please enable") || lower.contains("must enable") || lower.contains("need to specify")) {
        return Class::NeedsFeature;
    }
    if fe.code.is_none() && (r.contains("compile_error!") || msg.contains("not supported") || msg.contains("unsupported") || msg.contains("Unsupported")) {
        return Class::ExplicitlyUnsupported;
    }
    let libc_ref = msg.contains("crate `libc`") || msg.contains("module `libc`") || msg.contains("`libc::") || r.contains("libc::");
    let missing = matches!(fe.code.as_deref(), Some("E0425" | "E0412" | "E0432" | "E0433" | "E0422" | "E0423" | "E0531" | "E0599" | "E0405"));
    if missing && libc_ref {
        return Class::MissingLibcBinding;
    }
    if missing {
        return Class::NoQnxCfgArm;
    }
    if fe.code.as_deref() == Some("E0463") {
        return Class::NoLibrary;
    }
    Class::Other
}

fn build_script_class(text: &str) -> Class {
    let t = text;
    let cc = t.contains("cc-rs") || t.contains("ToolExecError") || t.contains("failed to find tool") || t.contains("qcc") && t.contains("error:");
    let sys = t.contains("pkg-config") || t.contains("PKG_CONFIG") || t.contains("cmake") || t.contains("CMake") || t.contains("Could not find directory of OpenSSL") || t.contains("vcpkg");
    if sys {
        Class::BuildScriptSystemLib
    } else if cc {
        Class::BuildScriptCc
    } else {
        Class::BuildScript
    }
}

/// The most telling line of a failed build script: the C compiler's own complaint beats cc-rs's
/// "command did not execute successfully", which beats a panic message.
fn build_script_message(tail: &str) -> String {
    let lines: Vec<&str> = tail.lines().map(|l| l.trim().trim_start_matches("cargo:warning=").trim()).collect();
    let useful = |l: &&&str| !l.starts_with("ToolExecError") && !l.starts_with("error: failed to run custom build command") && !l.starts_with("Caused by");
    let pick = |pred: &dyn Fn(&str) -> bool| lines.iter().filter(useful).find(|l| pred(l)).map(|l| first_line(l));
    pick(&|l| l.contains("unknown option") || l.contains("fatal error") || l.contains("#error") || l.contains(": error:") || l.contains("No such file or directory"))
        .or_else(|| {
            // a build script panic: the message is on the line after "panicked at <file>"
            let i = lines.iter().position(|l| l.contains("panicked at"))?;
            lines.get(i + 1).filter(|l| !l.is_empty()).map(|l| first_line(l))
        })
        .or_else(|| pick(&|l| l.contains("not found") || l.contains("could not") || l.contains("Could not")))
        .or_else(|| pick(&|l| l.to_ascii_lowercase().contains("error")))
        .unwrap_or_else(|| "build script failed".to_string())
}

/// `stdout` is cargo's `--message-format=json` stream, `stderr` its human output.
pub fn classify(success: bool, stdout: &str, stderr: &str) -> Verdict {
    if success {
        return Verdict { ok: true, class: None, first_error: None };
    }

    // 1. cargo-level errors that happen before or instead of rustc diagnostics.
    let lines: Vec<&str> = stderr.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim();
        // cargo 1.85: "rustc 1.85.1-dev is not supported by the following package:", then "foo@1.2 requires rustc 1.87"
        if l.contains("is not supported by the following package") {
            let next = lines.get(i + 1).map(|x| x.trim()).unwrap_or("");
            let pkg = next.split_whitespace().next().unwrap_or("");
            let (name, version) = pkg.split_once('@').unwrap_or((pkg, ""));
            let fe = FirstError { package: name.into(), version: version.into(), code: None, message: first_line(next), file: None, kind: None };
            return Verdict { ok: false, class: Some(Class::RustcTooOld), first_error: Some(fe) };
        }
        if l.contains("the compiler unexpectedly panicked") {
            let pkg = stderr.lines().rev().find_map(|x| x.trim().strip_prefix("Compiling ")).unwrap_or("");
            let mut it = pkg.split_whitespace();
            let fe = FirstError { package: it.next().unwrap_or("").into(), version: it.next().unwrap_or("").trim_start_matches('v').into(), code: None, message: "internal compiler error".into(), file: None, kind: None };
            return Verdict { ok: false, class: Some(Class::CompilerCrash), first_error: Some(fe) };
        }
    }
    for line in stderr.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("error: package `") {
            if l.contains("requires rustc") {
                let pkg = rest.split('`').next().unwrap_or("");
                let mut it = pkg.split_whitespace();
                let name = it.next().unwrap_or("").to_string();
                let version = it.next().unwrap_or("").trim_start_matches('v').to_string();
                let fe = FirstError { package: name, version, code: None, message: first_line(l.trim_start_matches("error: ")), file: None, kind: None };
                return Verdict { ok: false, class: Some(Class::RustcTooOld), first_error: Some(fe) };
            }
        }
        if l.contains("which is missing a lib target") {
            let fe = FirstError { package: String::new(), version: String::new(), code: None, message: first_line(l), file: None, kind: None };
            return Verdict { ok: false, class: Some(Class::NoLibrary), first_error: Some(fe) };
        }
        if l.starts_with("error: failed to select a version") || l.starts_with("error: failed to download") || l.starts_with("error: no matching package") || l.starts_with("error: failed to get") {
            let fe = FirstError { package: String::new(), version: String::new(), code: None, message: first_line(l.trim_start_matches("error: ")), file: None, kind: None };
            return Verdict { ok: false, class: Some(Class::Resolve), first_error: Some(fe) };
        }
    }

    // 2. the earliest rustc error from the JSON stream
    for line in stdout.lines() {
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["reason"] != "compiler-message" || v["message"]["level"] != "error" {
            continue;
        }
        let m = &v["message"];
        let text = m["message"].as_str().unwrap_or("");
        if text.starts_with("aborting due to") || text.starts_with("could not compile") {
            continue;
        }
        let (package, version) = parse_package_id(v["package_id"].as_str().unwrap_or(""));
        let file = m["spans"]
            .as_array()
            .and_then(|s| s.iter().find(|x| x["is_primary"] == true).or(s.first()))
            .and_then(|s| s["file_name"].as_str())
            .map(trim_file);
        let kind = v["target"]["kind"].as_array().and_then(|k| k.first()).and_then(|k| k.as_str()).map(str::to_string);
        let fe = FirstError {
            package,
            version,
            code: m["code"]["code"].as_str().map(str::to_string),
            message: first_line(text),
            file,
            kind,
        };
        let rendered = m["rendered"].as_str().unwrap_or("");
        let class = class_for_compiler_error(&fe, rendered);
        return Verdict { ok: false, class: Some(class), first_error: Some(fe) };
    }

    // 3. build script failures are only in stderr
    if let Some(i) = stderr.find("failed to run custom build command for `") {
        let rest = &stderr[i + "failed to run custom build command for `".len()..];
        let pkg = rest.split('`').next().unwrap_or("");
        let mut it = pkg.split_whitespace();
        let name = it.next().unwrap_or("").to_string();
        let version = it.next().unwrap_or("").trim_start_matches('v').to_string();
        let tail = &stderr[i..];
        let msg = build_script_message(tail);
        let fe = FirstError { package: name, version, code: None, message: msg, file: Some("build.rs".into()), kind: Some("custom-build".into()) };
        return Verdict { ok: false, class: Some(build_script_class(tail)), first_error: Some(fe) };
    }

    // 4. anything else cargo printed
    let msg = stderr.lines().map(str::trim).find(|l| l.starts_with("error")).map(first_line).unwrap_or_else(|| "build failed".into());
    let mut package = String::new();
    let mut version = String::new();
    if let Some(i) = stderr.find("could not compile `") {
        let rest = &stderr[i + "could not compile `".len()..];
        package = rest.split('`').next().unwrap_or("").to_string();
        if let Some(v) = rest.split('`').nth(1).and_then(|s| s.split_whitespace().find(|w| w.starts_with('v') && w[1..].starts_with(|c: char| c.is_ascii_digit()))) {
            version = v.trim_start_matches('v').trim_end_matches(')').to_string();
        }
    }
    let class = if msg.contains("linker") || msg.contains("linking") { Class::Link } else { Class::Other };
    Verdict { ok: false, class: Some(class), first_error: Some(FirstError { package, version, code: None, message: msg, file: None, kind: None }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(pkg: &str, code: Option<&str>, text: &str, file: &str, kind: &str) -> String {
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": pkg,
            "target": {"kind": [kind], "name": "x"},
            "message": {
                "level": "error",
                "message": text,
                "code": code.map(|c| serde_json::json!({"code": c})),
                "rendered": format!("error: {text}"),
                "spans": [{"file_name": file, "is_primary": true}]
            }
        })
        .to_string()
    }

    #[test]
    fn package_ids_both_formats() {
        assert_eq!(parse_package_id("registry+https://github.com/rust-lang/crates.io-index#libc@0.2.191"), ("libc".into(), "0.2.191".into()));
        assert_eq!(parse_package_id("libc 0.2.191 (registry+https://github.com/rust-lang/crates.io-index)"), ("libc".into(), "0.2.191".into()));
    }

    #[test]
    fn libc_cfg_rename_is_its_own_class() {
        let out = msg(
            "registry+https://github.com/rust-lang/crates.io-index#libc@0.2.191",
            Some("E0412"),
            "cannot find type `in_addr` in this scope",
            "C:\\Users\\me\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\libc-0.2.191\\src\\unix\\nto\\mod.rs",
            "lib",
        );
        let v = classify(false, &out, "error: could not compile `libc` (lib) due to 40 previous errors");
        assert_eq!(v.class, Some(Class::LibcQnx8Cfg));
        let fe = v.first_error.unwrap();
        assert_eq!(fe.package, "libc");
        assert_eq!(fe.file.as_deref(), Some("src/unix/nto/mod.rs"));
    }

    #[test]
    fn old_libc_error_is_not_the_cfg_class() {
        let out = msg("registry+https://github.com/rust-lang/crates.io-index#libc@0.2.186", Some("E0412"), "cannot find type `foo`", "src/lib.rs", "lib");
        assert_eq!(classify(false, &out, "").class, Some(Class::NoQnxCfgArm));
    }

    #[test]
    fn missing_libc_binding() {
        let out = msg("registry+https://github.com/rust-lang/crates.io-index#nix@0.29.0", Some("E0425"), "cannot find value `SO_PEERCRED` in crate `libc`", "src/sys/socket.rs", "lib");
        assert_eq!(classify(false, &out, "").class, Some(Class::MissingLibcBinding));
    }

    #[test]
    fn rustc_too_old_from_cargo() {
        let err = "error: package `foo v1.4.0` cannot be built because it requires rustc 1.88 or newer, while the currently active rustc version is 1.85.1-dev";
        let v = classify(false, "", err);
        assert_eq!(v.class, Some(Class::RustcTooOld));
        assert_eq!(v.first_error.unwrap().package, "foo");
    }

    #[test]
    fn unstable_feature_is_rustc_too_old() {
        let out = msg("registry+https://github.com/rust-lang/crates.io-index#bar@2.0.0", Some("E0658"), "`let` expressions in this position are unstable", "src/lib.rs", "lib");
        assert_eq!(classify(false, &out, "").class, Some(Class::RustcTooOld));
    }

    #[test]
    fn compile_error_is_explicit() {
        let out = msg("registry+https://github.com/rust-lang/crates.io-index#getrandom@0.2.15", None, "target is not supported, for more information see: https://docs.rs/getrandom", "src/lib.rs", "lib");
        assert_eq!(classify(false, &out, "").class, Some(Class::ExplicitlyUnsupported));
    }

    #[test]
    fn build_script_cc() {
        let err = "error: failed to run custom build command for `ring v0.17.8`\n\nCaused by:\n  process didn't exit successfully\n  --- stderr\n  error occurred in cc-rs: command did not execute successfully\n";
        let v = classify(false, "", err);
        assert_eq!(v.class, Some(Class::BuildScriptCc));
        let fe = v.first_error.unwrap();
        assert_eq!((fe.package.as_str(), fe.version.as_str()), ("ring", "0.17.8"));
    }

    #[test]
    fn build_script_prefers_compiler_complaint() {
        let err = "error: failed to run custom build command for `aws-lc-sys v0.47.0`
  cargo:warning=cc: unknown option: '--include=x.h'
  cargo:warning=ToolExecError: command did not execute successfully
  error occurred in cc-rs: command did not execute successfully";
        let v = classify(false, "", err);
        assert_eq!(v.class, Some(Class::BuildScriptCc));
        assert_eq!(v.first_error.unwrap().message, "cc: unknown option: '--include=x.h'");
    }

    #[test]
    fn build_script_pkg_config() {
        let err = "error: failed to run custom build command for `openssl-sys v0.9.103`\n  --- stderr\n  Could not find directory of OpenSSL installation\n  pkg-config exited with status code 1";
        assert_eq!(classify(false, "", err).class, Some(Class::BuildScriptSystemLib));
    }

    #[test]
    fn link_error() {
        let out = msg("path+file:///C:/work/atlas-probe#0.0.0", None, "linking with `qcc` failed: exit code: 1", "", "bin");
        assert_eq!(classify(false, &out, "").class, Some(Class::Link));
    }

    #[test]
    fn success_is_ok() {
        assert!(classify(true, "", "").ok);
    }

    #[test]
    fn libc_pin_threshold() {
        assert!(!libc_needs_pin("0.2.186"));
        assert!(libc_needs_pin("0.2.189"));
        assert!(libc_needs_pin("0.2.191"));
        assert!(libc_needs_pin("0.3.0"));
    }
}
