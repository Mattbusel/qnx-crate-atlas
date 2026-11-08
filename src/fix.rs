//! `atlas fix`: apply the atlas's known QNX 8.0 fixes to a user's Cargo project, print what changed,
//! and verify by building the project for both QNX targets.
//!
//! The fixes are the same ones `atlas compat` measured on the top 500 crates:
//!   libc 0.2.189+        -> path patch to the libc compatibility copy (patches/make_libc_compat.py),
//!                           or `--libc-pin` to lock libc 0.2.186 instead
//!   aws-lc-sys 0.47      -> path patch to the qcc fix (aws/aws-lc-rs#1292, merged upstream)
//!   target-lexicon 0.13  -> path patch adding QNX triples (bytecodealliance/target-lexicon#138)
//! Only crates that are actually in the project's lockfile are patched.

use crate::classify;
use crate::probe::LIBC_PIN;
use crate::qnxenv::QnxEnv;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// (crate, directory under the patch store, why)
pub const KNOWN: [(&str, &str, &str); 3] = [
    ("libc", "libc-qnx8", "libc 0.2.189+ only knows QNX 8 as target_os=\"qnx\"; the QNX toolchain reports \"nto\"/\"nto80\""),
    ("aws-lc-sys", "awslc-rs-main/aws-lc-sys|aws-lc-sys-qnx", "qcc rejects --include= and QNX headers need _QNX_SOURCE (aws/aws-lc-rs#1292); the awslc-rs-main copy adds QNX aarch64 CPU detection (aws/aws-lc#3780)"),
    ("target-lexicon", "target-lexicon-qnx", "cannot parse QNX triples, build scripts panic (bytecodealliance/target-lexicon#138)"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// add `[patch.crates-io] name = { path }`
    Patch { name: String, locked: String, copy: String, path: PathBuf, why: String },
    /// `cargo update -p libc --precise 0.2.186`
    PinLibc { locked: String },
    /// in the graph but the patched copy is not semver compatible with the locked version
    Skip { name: String, locked: String, copy: String },
}

fn vt(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+']).take(3).map(|x| x.parse().unwrap_or(0)).collect()
}

/// Cargo's caret rule: same leftmost non-zero component.
pub fn semver_compatible(a: &str, b: &str) -> bool {
    let (a, b) = (vt(a), vt(b));
    match (a.as_slice(), b.as_slice()) {
        ([0, 0, x, ..], [0, 0, y, ..]) => x == y,
        ([0, x, ..], [0, y, ..]) => x == y && *x != 0,
        ([x, ..], [y, ..]) => x == y && *x != 0,
        _ => false,
    }
}

/// Every locked version of `name` in a Cargo.lock.
pub fn locked_versions(lock: &str, name: &str) -> Vec<String> {
    let Ok(t) = lock.parse::<toml::Table>() else { return vec![] };
    t.get("package")
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
                .filter(|p| p.get("source").and_then(|s| s.as_str()).map(|s| s.starts_with("registry+")).unwrap_or(false))
                .filter_map(|p| p.get("version").and_then(|v| v.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `store` holds (crate, version of the patched copy, path). Decide what to change.
pub fn plan(lock: &str, store: &[(String, String, PathBuf)], libc_pin: bool) -> Vec<Action> {
    let mut out = vec![];
    for (name, _, why) in KNOWN {
        let locked = locked_versions(lock, name);
        let Some(newest) = locked.iter().max_by_key(|v| vt(v)).cloned() else { continue };
        if name == "libc" {
            if !classify::libc_needs_pin(&newest) {
                continue;
            }
            if libc_pin {
                out.push(Action::PinLibc { locked: newest });
                continue;
            }
        }
        let Some((_, copy, path)) = store.iter().find(|(n, _, _)| n == name) else {
            out.push(Action::Skip { name: name.into(), locked: newest, copy: "missing".into() });
            continue;
        };
        if locked.iter().any(|l| semver_compatible(l, copy)) {
            out.push(Action::Patch { name: name.into(), locked: newest, copy: copy.clone(), path: path.clone(), why: why.into() });
        } else {
            out.push(Action::Skip { name: name.into(), locked: newest, copy: copy.clone() });
        }
    }
    out
}

/// Add path patches under `[patch.crates-io]`, keeping the rest of the manifest as written.
/// Returns the new text and the lines that changed.
pub fn edit_manifest(text: &str, patches: &[(String, String)]) -> Result<(String, Vec<String>)> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing Cargo.toml")?;
    let patch = doc.entry("patch").or_insert(toml_edit::table()).as_table_mut().context("[patch] is not a table")?;
    patch.set_implicit(true);
    let cio = patch.entry("crates-io").or_insert(toml_edit::table()).as_table_mut().context("[patch.crates-io] is not a table")?;
    let mut changed = vec![];
    for (name, path) in patches {
        let mut t = toml_edit::InlineTable::new();
        t.insert("path", path.as_str().into());
        let new = toml_edit::value(t);
        if cio.get(name).map(|v| v.to_string().trim() == new.to_string().trim()).unwrap_or(false) {
            continue;
        }
        cio.insert(name, new);
        changed.push(format!("[patch.crates-io] {name} = {{ path = \"{path}\" }}"));
    }
    Ok((doc.to_string(), changed))
}

/// Cargo settings that QNX builds need, as (table, key, value, why).
pub fn config_settings(windows_host: bool) -> Vec<(&'static str, &'static str, toml_edit::Value, &'static str)> {
    let mut v = vec![("resolver", "incompatible-rust-versions", "fallback".into(), "pick dependency versions that support the QNX rustc 1.85")];
    if windows_host {
        v.push(("build", "incremental", false.into(), "fewer object files: qcc fails to spawn ld past the Windows command-line limit"));
        v.push(("profile.dev", "codegen-units", 16i64.into(), "same Windows command-line limit on large dependency graphs"));
    }
    v
}

/// Merge settings into .cargo/config.toml text, keeping anything the user set. Returns (text, changed lines).
pub fn edit_config(text: &str, settings: &[(&str, &str, toml_edit::Value, &str)]) -> Result<(String, Vec<String>)> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing .cargo/config.toml")?;
    let mut changed = vec![];
    for (table, key, val, why) in settings {
        let mut t = doc.as_table_mut();
        for part in table.split('.') {
            let e = t.entry(part).or_insert(toml_edit::table());
            if let Some(tt) = e.as_table_mut() {
                tt.set_implicit(true);
            }
            t = e.as_table_mut().context("config table is not a table")?;
        }
        if t.contains_key(key) {
            continue;
        }
        t.insert(key, toml_edit::Item::Value(val.clone()));
        changed.push(format!("[{table}] {key} = {} ({why})", val.to_string().trim()));
    }
    Ok((doc.to_string(), changed))
}

/// The patch store: --store, else $QNX_ATLAS_STORE, else `work/` of the atlas checkout that holds this binary.
pub fn find_store(given: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = given.map(Path::to_path_buf).or_else(|| std::env::var_os("QNX_ATLAS_STORE").map(PathBuf::from)) {
        return Ok(std::path::absolute(p)?);
    }
    let exe = std::env::current_exe()?;
    for a in exe.ancestors() {
        if a.join("patches").join("prepare.py").is_file() {
            return Ok(a.join("work"));
        }
    }
    bail!("no patch store found: run `python patches/prepare.py` in the atlas checkout, then pass --store <checkout>/work")
}

fn store_entries(store: &Path) -> Vec<(String, String, PathBuf)> {
    KNOWN
        .iter()
        .filter_map(|(name, dirs, _)| {
            // first candidate directory that exists wins
            let p = dirs.split('|').map(|d| store.join(d)).find(|p| p.join("Cargo.toml").is_file())?;
            let m: toml::Table = std::fs::read_to_string(p.join("Cargo.toml")).ok()?.parse().ok()?;
            let v = m.get("package")?.get("version")?.as_str()?.to_string();
            Some((name.to_string(), v, p))
        })
        .collect()
}

fn run(mut c: Command) -> Result<(bool, String, String)> {
    let o = c.stdin(Stdio::null()).output().context("running cargo")?;
    Ok((o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned()))
}

pub struct Opts<'a> {
    pub project: &'a Path,
    pub store: Option<&'a Path>,
    pub targets: &'a [String],
    pub libc_pin: bool,
    pub dry_run: bool,
    pub no_verify: bool,
    /// build once before changing anything, to show what the fixes change
    pub baseline: bool,
    pub jobs: usize,
}

/// Returns true when every target built.
pub fn fix(env: &QnxEnv, o: &Opts) -> Result<bool> {
    let project = std::path::absolute(o.project)?;
    let mut c = env.cargo();
    c.current_dir(&project).args(["locate-project", "--workspace", "--message-format", "plain"]);
    let (ok, out, err) = run(c)?;
    if !ok {
        bail!("{} is not a Cargo project: {}", project.display(), err.trim());
    }
    let manifest = PathBuf::from(out.trim());
    let root = manifest.parent().context("manifest has no parent")?.to_path_buf();
    println!("project      {}", root.display());
    if !root.join("Cargo.lock").is_file() {
        let mut c = env.cargo();
        c.current_dir(&root).arg("generate-lockfile");
        let (ok, _, e) = run(c)?;
        if !ok {
            bail!("cargo generate-lockfile failed: {}", e.trim());
        }
        println!("lockfile     generated");
    }
    let lock = std::fs::read_to_string(root.join("Cargo.lock"))?;
    let store = if o.libc_pin && locked_versions(&lock, "aws-lc-sys").is_empty() && locked_versions(&lock, "target-lexicon").is_empty() {
        vec![]
    } else {
        let s = find_store(o.store)?;
        println!("patch store  {}", s.display());
        store_entries(&s)
    };
    if o.baseline && !o.dry_run && !o.no_verify {
        verify(env, &root, o, "before")?;
    }
    let actions = plan(&lock, &store, o.libc_pin);
    let mut patches = vec![];
    if actions.is_empty() {
        println!("no known QNX fix applies: none of libc 0.2.189+, aws-lc-sys, target-lexicon is in the graph");
    }
    for a in &actions {
        match a {
            Action::Patch { name, locked, copy, path, why } => {
                println!("fix          {name} {locked} -> patched {copy}: {why}");
                patches.push((name.clone(), path.display().to_string().replace('\\', "/")));
            }
            Action::PinLibc { locked } => println!("fix          libc {locked} -> {LIBC_PIN} (cargo update --precise)"),
            Action::Skip { name, locked, copy } => println!("skip         {name} {locked}: patched copy is {copy}, not semver compatible; no known fix applied"),
        }
    }
    let cfg_path = root.join(".cargo").join("config.toml");
    let cfg_text = std::fs::read_to_string(&cfg_path).unwrap_or_default();
    let (cfg_new, cfg_changed) = edit_config(&cfg_text, &config_settings(cfg!(windows)))?;
    for l in &cfg_changed {
        println!("fix          .cargo/config.toml {l}");
    }
    if o.dry_run {
        println!("dry run, nothing written");
        return Ok(true);
    }
    if !cfg_changed.is_empty() {
        std::fs::create_dir_all(cfg_path.parent().unwrap())?;
        std::fs::write(&cfg_path, cfg_new)?;
        println!("changed      {}", cfg_path.display());
    }
    if !patches.is_empty() {
        let text = std::fs::read_to_string(&manifest)?;
        let (new, changed) = edit_manifest(&text, &patches)?;
        if !changed.is_empty() {
            let bak = manifest.with_file_name("Cargo.toml.qnx-atlas.bak");
            if !bak.exists() {
                std::fs::write(&bak, &text)?;
            }
            std::fs::write(&manifest, new)?;
            for l in &changed {
                println!("changed      {}: {l}", manifest.display());
            }
            println!("backup       {}", bak.display());
        }
        let mut c = env.cargo();
        c.current_dir(&root).arg("update");
        for (n, _) in &patches {
            c.args(["-p", n]);
        }
        let (ok, _, e) = run(c)?;
        if !ok {
            bail!("cargo update after patching failed: {}", e.trim());
        }
        println!("changed      Cargo.lock: {} re-pointed at the patched copies", patches.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", "));
    }
    if let Some(Action::PinLibc { .. }) = actions.iter().find(|a| matches!(a, Action::PinLibc { .. })) {
        let mut c = env.cargo();
        c.current_dir(&root).args(["update", "-p", "libc", "--precise", LIBC_PIN]);
        let (ok, _, e) = run(c)?;
        if !ok {
            bail!("pinning libc {LIBC_PIN} failed (another crate needs a newer libc; drop --libc-pin to use the patched copy): {}", e.trim());
        }
        println!("changed      Cargo.lock: libc pinned to {LIBC_PIN}");
    }
    if o.no_verify {
        return Ok(true);
    }
    let v = verify(env, &root, o, "verify")?;
    if v.iter().any(|c| *c == Some(classify::Class::RustcTooOld)) {
        // the lockfile predates the resolver setting; re-resolve for rustc 1.85, keeping the patches
        let mut c = env.cargo();
        c.current_dir(&root).arg("update");
        let (ok, _, e) = run(c)?;
        if !ok {
            bail!("cargo update for rustc 1.85 failed: {}", e.trim());
        }
        println!("changed      Cargo.lock: re-resolved for rustc 1.85 (a dependency needed a newer rustc)");
        return Ok(verify(env, &root, o, "verify")?.iter().all(Option::is_none));
    }
    Ok(v.iter().all(Option::is_none))
}

/// One entry per target: None when it built, else the failure class.
fn verify(env: &QnxEnv, root: &Path, o: &Opts, label: &str) -> Result<Vec<Option<classify::Class>>> {
    let mut all = vec![];
    for t in o.targets {
        let mut c = env.cargo();
        c.current_dir(&root)
            .args(["build", "--message-format=json", "--target", t])
            .env("CARGO_BUILD_JOBS", o.jobs.to_string())
            .env("CARGO_PROFILE_DEV_DEBUG", "0")
            .env("CARGO_INCREMENTAL", "0");
        let start = std::time::Instant::now();
        let (ok, out, err) = run(c)?;
        let v = classify::classify(ok, &out, &err);
        if v.ok {
            println!("{label:12} {t}: builds ({:.0}s)", start.elapsed().as_secs_f64());
            all.push(None);
        } else {
            all.push(Some(v.class.unwrap_or(classify::Class::Other)));
            let fe = v.first_error.map(|f| format!("{} {}: {}", f.package, f.version, f.message)).unwrap_or_default();
            println!("{label:12} {t}: FAILS [{}] {fe}", v.class.map(|c| c.slug()).unwrap_or("unknown"));
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"
version = 4
[[package]]
name = "app"
version = "0.1.0"
[[package]]
name = "libc"
version = "0.2.191"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "aws-lc-sys"
version = "0.47.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "target-lexicon"
version = "0.12.16"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    fn store() -> Vec<(String, String, PathBuf)> {
        vec![
            ("libc".into(), "0.2.191".into(), "/s/libc-qnx8".into()),
            ("aws-lc-sys".into(), "0.47.0".into(), "/s/aws-lc-sys-qnx".into()),
            ("target-lexicon".into(), "0.13.5".into(), "/s/target-lexicon-qnx".into()),
        ]
    }

    #[test]
    fn caret_rule() {
        assert!(semver_compatible("0.2.186", "0.2.191"));
        assert!(!semver_compatible("0.12.16", "0.13.5"));
        assert!(semver_compatible("1.2.0", "1.9.9"));
        assert!(!semver_compatible("0.0.1", "0.0.2"));
    }

    #[test]
    fn plans_patches_for_present_crates() {
        let a = plan(LOCK, &store(), false);
        assert!(matches!(&a[0], Action::Patch { name, .. } if name == "libc"));
        assert!(matches!(&a[1], Action::Patch { name, .. } if name == "aws-lc-sys"));
        assert!(matches!(&a[2], Action::Skip { name, copy, .. } if name == "target-lexicon" && copy == "0.13.5"));
    }

    #[test]
    fn old_libc_needs_nothing_and_pin_mode() {
        let old = LOCK.replace("0.2.191", "0.2.180");
        assert!(!plan(&old, &store(), false).iter().any(|a| matches!(a, Action::Patch { name, .. } if name == "libc")));
        assert_eq!(plan(LOCK, &store(), true)[0], Action::PinLibc { locked: "0.2.191".into() });
    }

    #[test]
    fn manifest_edit_keeps_text_and_is_idempotent() {
        let m = "[package]\nname = \"app\" # mine\nversion = \"0.1.0\"\n\n[dependencies]\ntokio = \"1\"\n";
        let p = vec![("libc".to_string(), "/s/libc-qnx8".to_string())];
        let (out, ch) = edit_manifest(m, &p).unwrap();
        assert!(out.starts_with(m));
        assert!(out.contains("[patch.crates-io]\nlibc = { path = \"/s/libc-qnx8\" }"));
        assert_eq!(ch.len(), 1);
        let (out2, ch2) = edit_manifest(&out, &p).unwrap();
        assert_eq!(out2, out);
        assert!(ch2.is_empty());
    }

    #[test]
    fn manifest_edit_extends_existing_patch_table() {
        let m = "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[patch.crates-io]\nserde = { path = \"../serde\" }\n";
        let (out, _) = edit_manifest(m, &[("libc".into(), "/s/l".into())]).unwrap();
        assert!(out.contains("serde = { path = \"../serde\" }\nlibc = { path = \"/s/l\" }"));
    }

    #[test]
    fn config_merge_keeps_user_values() {
        let (out, ch) = edit_config("[build]\nincremental = true\n", &config_settings(true)).unwrap();
        assert!(out.contains("incremental = true"));
        assert!(out.contains("incompatible-rust-versions = \"fallback\""));
        assert!(out.contains("[profile.dev]\ncodegen-units = 16"));
        assert_eq!(ch.len(), 2);
        let (_, again) = edit_config(&out, &config_settings(true)).unwrap();
        assert!(again.is_empty());
    }
}
