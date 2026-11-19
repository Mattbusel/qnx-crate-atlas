//! Reads what a crate says about where it runs: `categories`, `keywords`, `description`, the
//! `#![no_std]` attribute in its library root and the README. Every claim keeps the file and line it
//! came from, so a failing check can point at the exact sentence that is not true.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    /// "works without std": category no-std, keyword no_std, `#![no_std]`, README
    NoStd,
    /// "runs on microcontrollers": category embedded, keyword bare-metal, README
    Embedded,
    /// names QNX as supported
    Qnx,
    /// "cross-platform", "portable", "platform-independent"
    Portable,
}

impl ClaimKind {
    pub fn slug(self) -> &'static str {
        match self {
            ClaimKind::NoStd => "no-std",
            ClaimKind::Embedded => "embedded",
            ClaimKind::Qnx => "qnx",
            ClaimKind::Portable => "portable",
        }
    }

    /// Which build proves or disproves the claim.
    pub fn check(self) -> CheckKind {
        match self {
            ClaimKind::NoStd | ClaimKind::Embedded => CheckKind::BareMetal,
            ClaimKind::Qnx | ClaimKind::Portable => CheckKind::Qnx,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    /// a target with no operating system and no std (thumbv7em-none-eabihf by default)
    BareMetal,
    /// a POSIX RTOS the crate did not special-case: QNX
    Qnx,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Category,
    Keyword,
    Description,
    Attribute,
    Readme,
    LibDocs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claim {
    pub kind: ClaimKind,
    pub source: Source,
    /// file relative to the package root
    pub file: String,
    pub line: Option<u32>,
    /// the words that make the claim
    pub text: String,
}

/// What the package manifest and sources say, plus facts needed to build it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageClaims {
    pub name: String,
    pub version: String,
    pub claims: Vec<Claim>,
    /// a proc-macro crate runs on the host only, so target claims do not apply
    pub proc_macro: bool,
    pub has_lib: bool,
    pub has_default_features: bool,
    /// every feature the crate declares
    #[serde(default)]
    pub features: Vec<String>,
    /// the crate only exists for some OS family (`#![cfg(windows)]`, `cfg_attr(unix, no_std)`,
    /// `os::windows-apis`), so a bare-metal build does not test its claim
    #[serde(default)]
    pub platform_scope: Option<String>,
    /// features named in `#![cfg_attr(feature = "...", no_std)]`: no_std only when they are on
    #[serde(default)]
    pub no_std_features: Vec<String>,
}

impl PackageClaims {
    pub fn checks(&self) -> Vec<CheckKind> {
        let mut v: Vec<CheckKind> = self.claims.iter().map(|c| c.kind.check()).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn for_check(&self, check: CheckKind) -> Vec<&Claim> {
        self.claims.iter().filter(|c| c.kind.check() == check).collect()
    }
}

const NO_STD_WORDS: &[&str] = &["no_std", "no-std", "nostd"];
const EMBEDDED_WORDS: &[&str] = &["embedded", "bare-metal", "baremetal", "bare_metal", "microcontroller", "microcontrollers", "mcu", "firmware"];
const PORTABLE_WORDS: &[&str] = &["portable", "cross-platform", "crossplatform", "cross_platform", "platform-independent", "platform-agnostic", "multiplatform"];
/// keywords that say "embedded" means "in-process", not "on a microcontroller"
const IN_PROCESS: &[&str] =
    &["database", "db", "sql", "sqlite", "kv", "storage", "scripting", "script", "language", "assets", "resources", "include", "files", "static", "interpreter"];

fn line_of(text: &str, needle: &str) -> Option<u32> {
    text.lines().position(|l| l.contains(needle)).map(|i| i as u32 + 1)
}

fn negated(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    [
        " not ",
        "n't",
        " never ",
        "no longer",
        "little effort",
        "no effort",
        "minimal effort",
        "unsupported",
        " cannot ",
        "planned",
        "todo",
        "in the future",
        "would be nice",
        "requires std",
        "needs std",
    ]
    .iter()
    .any(|n| l.contains(n))
}

/// Phrases in prose that claim the crate runs without std. Matches whole words only.
fn prose_no_std(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    if !NO_STD_WORDS.iter().any(|w| contains_word(&lower, w)) {
        return false;
    }
    if lower.starts_with("#![no_std]") {
        // a line of a README code sample
        return true;
    }
    [
        "support",
        "compatible",
        "works",
        "usable",
        "can be used",
        "without std",
        "without the standard library",
        "no_std environment",
        "no_std context",
        "no_std target",
        "no-std environment",
        "is no_std",
        "is `no_std`",
        "`no_std`-compatible",
        "no_std-compatible",
        "no-std compatible",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

fn prose_embedded(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    // "bare-metal performance" is a speed claim, not a target claim
    if ["bare-metal performance", "bare metal performance", "bare-metal speed", "bare metal speed", "bare-metal fast", "close to bare metal", "close to the bare metal"]
        .iter()
        .any(|p| lower.contains(p))
    {
        return false;
    }
    ["embedded systems", "embedded devices", "embedded targets", "embedded platforms", "embedded environments", "bare metal", "bare-metal", "microcontroller"]
        .iter()
        .any(|p| lower.contains(p))
}

fn prose_portable(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    if [
        "cross-platform",
        "cross platform",
        "platform-independent",
        "platform independent",
        "platform-agnostic",
        "platform agnostic",
        "works on all platforms",
        "runs on all platforms",
        "any platform",
        "every platform",
        "all major platforms",
        "runs everywhere",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        return true;
    }
    // "portable" alone is ambiguous (Portable Executable, portable-simd, portable-atomic)
    if contains_word(&lower, "portable") {
        let noise = [
            "portable executable",
            "portable-simd",
            "portable_simd",
            "portable simd",
            "portable-atomic",
            "portable_atomic",
            "portable network graphics",
            "portable pixmap",
            "portable document",
            "portable anymap",
            "portable bitmap",
            "portable graymap",
            "portable float",
        ];
        if noise.iter().any(|n| lower.contains(n)) {
            return false;
        }
        return ["platform", "operating system", " os ", "unix", "windows", "linux", "target", "posix"].iter().any(|p| lower.contains(p));
    }
    false
}

/// QNX named as somewhere the crate runs: a support sentence or a row of a platform table, not any
/// mention of the word.
fn prose_qnx(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    if !contains_word(&lower, "qnx") && !lower.contains("nto-qnx") {
        return false;
    }
    lower.starts_with('|')
        || ["support", "works on", "runs on", "tested on", "compatible", "targets", "platforms", "available on", "builds for", "builds on"].iter().any(|p| lower.contains(p))
}

/// whole-word match where '_' and '-' count as word characters
fn contains_word(hay: &str, w: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(i) = hay[from..].find(w) {
        let s = from + i;
        let e = s + w.len();
        let before = hay[..s].chars().next_back().map(is_word).unwrap_or(false);
        let after = hay[e..].chars().next().map(is_word).unwrap_or(false);
        if !before && !after {
            return true;
        }
        from = s + 1;
        if from >= hay.len() {
            break;
        }
    }
    false
}

/// A list item that opens with a link (`- [name](url) ...`) describes another project, not this one.
fn links_elsewhere(s: &str) -> bool {
    let t = s.trim_start_matches(['-', '*', '+', ' ']);
    t.starts_with('[') && t.contains("](")
}

/// a claim kind, the sentence test for it, and the words that locate it in the sentence
type ProseCheck = (ClaimKind, fn(&str) -> bool, &'static [&'static str]);

/// Scan prose (README, crate docs, description) sentence by sentence, so a sentence wrapped over
/// two lines is read whole; one claim per kind per file, reported at the line of its key word.
fn scan_prose(text: &str, file: &str, source: Source, out: &mut Vec<Claim>) {
    let mut in_code = false;
    let mut seen: Vec<ClaimKind> = Vec::new();
    // units of (first line number, lines) that form one paragraph, list item or table row
    let mut units: Vec<Vec<(u32, String)>> = Vec::new();
    let mut cur: Vec<(u32, String)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i as u32 + 1;
        let l = raw.trim().trim_start_matches("//!").trim_start_matches("///").trim().to_string();
        if l.starts_with("```") {
            in_code = !in_code;
            units.push(std::mem::take(&mut cur));
            continue;
        }
        if in_code {
            // a #![no_std] inside a README code sample is still a no_std claim; other code is not prose
            if l.contains("#![no_std]") {
                units.push(vec![(n, l)]);
            }
            continue;
        }
        let starts_unit = l.is_empty()
            || l.starts_with('#')
            || l.starts_with('-')
            || l.starts_with('*')
            || l.starts_with('|')
            || l.split_once(". ").map(|(a, _)| a.chars().all(|c| c.is_ascii_digit()) && !a.is_empty()).unwrap_or(false);
        if starts_unit {
            units.push(std::mem::take(&mut cur));
        }
        let single = l.starts_with('#') || l.starts_with('|');
        if !l.is_empty() {
            cur.push((n, l));
        }
        if single {
            units.push(std::mem::take(&mut cur));
        }
    }
    units.push(cur);
    let checks: [ProseCheck; 4] = [
        (ClaimKind::NoStd, prose_no_std, &["no_std", "no-std", "nostd"]),
        (ClaimKind::Embedded, prose_embedded, &["embedded", "bare", "microcontroller"]),
        (ClaimKind::Qnx, prose_qnx, &["qnx"]),
        (ClaimKind::Portable, prose_portable, &["cross", "platform", "portable", "everywhere"]),
    ];
    for unit in units.into_iter().filter(|u| !u.is_empty()) {
        // joined text with the byte offset where each source line starts
        let mut joined = String::new();
        let mut starts = Vec::new();
        for (n, l) in &unit {
            starts.push((joined.len(), *n));
            joined.push_str(l);
            joined.push(' ');
        }
        let line_at = |off: usize| starts.iter().rev().find(|(s, _)| *s <= off).map(|(_, n)| *n).unwrap_or(unit[0].0);
        let mut from = 0;
        for sentence in joined.split_inclusive([';']).flat_map(|p| p.split_inclusive(". ")) {
            let off = from;
            from += sentence.len();
            let plain: String = sentence.chars().filter(|c| *c != '*' && *c != '_').collect();
            let s = sentence.trim();
            if s.is_empty() || negated(&plain) || links_elsewhere(s) {
                continue;
            }
            for (kind, f, anchors) in checks {
                if seen.contains(&kind) || !f(s) {
                    continue;
                }
                seen.push(kind);
                let lower = sentence.to_ascii_lowercase();
                let a = anchors.iter().filter_map(|a| lower.find(a)).min().unwrap_or(0);
                out.push(Claim { kind, source, file: file.into(), line: Some(line_at(off + a)), text: s.chars().take(200).collect() });
            }
        }
    }
}

/// Does the library root opt out of std? Returns (line, attribute text). A crate that writes
/// `#![no_std]` and then links std back in unconditionally (`extern crate std;` with no `#[cfg]`, a
/// common way to drop the std prelude) is not claiming anything.
pub fn no_std_attribute(lib_rs: &str) -> Option<(u32, String)> {
    let mut found = None;
    for (i, l) in lib_rs.lines().enumerate() {
        let t = l.trim();
        if t.starts_with("//") {
            continue;
        }
        let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.starts_with("#![no_std]") || (compact.starts_with("#![cfg_attr(") && compact.contains(",no_std")) {
            found = Some((i as u32 + 1, t.chars().take(200).collect()));
            break;
        }
        // inner attributes come before the first item; stop at the first item
        if t.starts_with("pub ") || t.starts_with("mod ") || t.starts_with("use ") || t.starts_with("fn ") || t.starts_with("extern crate") {
            break;
        }
    }
    found.as_ref()?;
    let lines: Vec<&str> = lib_rs.lines().map(str::trim).collect();
    for (i, l) in lines.iter().enumerate() {
        if *l == "extern crate std;" || l.starts_with("extern crate std as ") {
            // only a feature switch makes it optional for a user; `#[cfg(not(some_nightly_cfg))]` does not
            let prev = lines[..i].iter().rev().find(|p| !p.is_empty() && !p.starts_with("//"));
            if !prev.map(|p| p.starts_with("#[cfg") && p.contains("feature")).unwrap_or(false) {
                return None;
            }
        }
    }
    found
}

fn is_platform_cfg(cond: &str) -> bool {
    ["unix", "windows", "target_os", "target_family", "target_vendor", "target_env"].iter().any(|w| contains_word(cond, w))
}

/// `#![cfg(windows)]` on the crate root, or a `cfg_attr(<os condition>, no_std)`: the crate (or its
/// no_std mode) is scoped to an OS family.
pub fn crate_root_scope(lib_rs: &str) -> Option<String> {
    for l in lib_rs.lines().map(str::trim) {
        let compact: String = l.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.starts_with("#![cfg(") && is_platform_cfg(&compact) {
            return Some(l.to_string());
        }
        if compact.starts_with("#![cfg_attr(") && compact.contains(",no_std") && is_platform_cfg(&compact) {
            return Some(l.to_string());
        }
        if l.starts_with("pub ") || l.starts_with("mod ") || l.starts_with("use ") || l.starts_with("fn ") {
            break;
        }
    }
    None
}

/// Features that turn no_std on: `#![cfg_attr(feature = "x", no_std)]` (not `not(feature = ...)`).
pub fn no_std_switch_features(lib_rs: &str) -> Vec<String> {
    let mut v = Vec::new();
    for l in lib_rs.lines().map(str::trim) {
        let compact: String = l.chars().filter(|c| !c.is_whitespace()).collect();
        if !(compact.starts_with("#![cfg_attr(") && compact.contains(",no_std")) || compact.contains("not(") {
            continue;
        }
        let mut rest = compact.as_str();
        while let Some(i) = rest.find("feature=\"") {
            let after = &rest[i + 9..];
            if let Some(j) = after.find('"') {
                v.push(after[..j].to_string());
                rest = &after[j..];
            } else {
                break;
            }
        }
    }
    v
}

/// Read the claims of the package rooted at `dir` (a directory holding Cargo.toml).
pub fn read_package(dir: &Path) -> anyhow::Result<PackageClaims> {
    let manifest_text = std::fs::read_to_string(dir.join("Cargo.toml"))?;
    let manifest: toml::Table = manifest_text.parse()?;
    let pkg = manifest.get("package").and_then(|p| p.as_table()).cloned().unwrap_or_default();
    let get_str = |k: &str| pkg.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let list = |k: &str| -> Vec<String> { pkg.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default() };
    let mut out = PackageClaims {
        name: get_str("name").unwrap_or_default(),
        version: get_str("version").unwrap_or_default(),
        has_default_features: manifest
            .get("features")
            .and_then(|f| f.as_table())
            .map(|f| f.get("default").and_then(|d| d.as_array()).map(|d| !d.is_empty()).unwrap_or(false))
            .unwrap_or(false),
        ..Default::default()
    };
    if let Some(f) = manifest.get("features").and_then(|f| f.as_table()) {
        out.features = f.keys().filter(|k| k.as_str() != "default").cloned().collect();
    }
    // an optional dependency is a feature too, unless a feature refers to it as dep:name
    let uses_dep_syntax = manifest_text.contains("\"dep:");
    if let Some(d) = manifest.get("dependencies").and_then(|d| d.as_table()) {
        for (k, v) in d {
            let optional = v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false);
            if optional && !out.features.contains(k) && !(uses_dep_syntax && manifest_text.contains(&format!("\"dep:{k}\""))) {
                out.features.push(k.clone());
            }
        }
    }
    let lib = manifest.get("lib").and_then(|l| l.as_table());
    out.proc_macro = lib.and_then(|l| l.get("proc-macro").or_else(|| l.get("proc_macro"))).and_then(|v| v.as_bool()).unwrap_or(false);
    let lib_path = lib.and_then(|l| l.get("path")).and_then(|p| p.as_str()).map(str::to_string).unwrap_or_else(|| "src/lib.rs".into());
    let lib_rs = std::fs::read_to_string(dir.join(&lib_path)).ok();
    out.has_lib = lib_rs.is_some();

    let categories = list("categories");
    if let Some(c) = categories.iter().find(|c| ["os::windows-apis", "os::macos-apis", "os::ios-apis", "os::android-apis"].contains(&c.as_str())) {
        out.platform_scope = Some(format!("categories = [\"{c}\"]"));
    }
    if let Some(src) = &lib_rs {
        if let Some(scope) = crate_root_scope(src) {
            out.platform_scope = Some(scope);
        }
        out.no_std_features = no_std_switch_features(src);
    }
    let keywords: Vec<String> = list("keywords").into_iter().map(|k| k.to_ascii_lowercase()).collect();
    for c in &categories {
        let kind = match c.as_str() {
            "no-std" | "no-std::no-alloc" => Some(ClaimKind::NoStd),
            "embedded" => Some(ClaimKind::Embedded),
            _ => None,
        };
        if let Some(kind) = kind {
            out.claims.push(Claim {
                kind,
                source: Source::Category,
                file: "Cargo.toml".into(),
                line: line_of(&manifest_text, &format!("\"{c}\"")),
                text: format!("categories = [\"{c}\"]"),
            });
        }
    }
    let in_process = keywords.iter().any(|k| IN_PROCESS.contains(&k.as_str())) || categories.iter().any(|c| c.starts_with("database"));
    for k in &keywords {
        let kind = if NO_STD_WORDS.contains(&k.as_str()) {
            Some(ClaimKind::NoStd)
        } else if EMBEDDED_WORDS.contains(&k.as_str()) && !(k == "embedded" && in_process) {
            Some(ClaimKind::Embedded)
        } else if k == "qnx" || k == "nto" {
            Some(ClaimKind::Qnx)
        } else if PORTABLE_WORDS.contains(&k.as_str()) {
            Some(ClaimKind::Portable)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.claims.push(Claim {
                kind,
                source: Source::Keyword,
                file: "Cargo.toml".into(),
                line: line_of(&manifest_text, &format!("\"{k}\"")),
                text: format!("keywords = [\"{k}\"]"),
            });
        }
    }
    if let Some(d) = get_str("description") {
        let one: String = d.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut tmp = Vec::new();
        scan_prose(&one, "Cargo.toml", Source::Description, &mut tmp);
        for mut c in tmp {
            c.line = line_of(&manifest_text, "description");
            out.claims.push(c);
        }
    }
    if let Some(src) = &lib_rs {
        if let Some((line, text)) = no_std_attribute(src) {
            out.claims.push(Claim { kind: ClaimKind::NoStd, source: Source::Attribute, file: lib_path.clone(), line: Some(line), text });
        }
        let docs: String = src.lines().map(|l| if l.trim_start().starts_with("//!") { l } else { "" }).collect::<Vec<_>>().join("\n");
        scan_prose(&docs, &lib_path, Source::LibDocs, &mut out.claims);
    }
    let readme = match pkg.get("readme") {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(toml::Value::Boolean(false)) => None,
        _ => ["README.md", "README", "readme.md", "README.markdown"].iter().find(|f| dir.join(f).is_file()).map(|f| f.to_string()),
    };
    if let Some(r) = readme {
        if let Ok(text) = std::fs::read_to_string(dir.join(&r)) {
            scan_prose(&text, &r, Source::Readme, &mut out.claims);
        }
    }
    // keep the strongest source first for each kind: category, keyword, attribute, description, docs
    let rank = |s: Source| match s {
        Source::Category => 0,
        Source::Keyword => 1,
        Source::Attribute => 2,
        Source::Description => 3,
        Source::Readme => 4,
        Source::LibDocs => 5,
    };
    out.claims.sort_by_key(|c| (c.kind, rank(c.source)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(files: &[(&str, &str)]) -> PackageClaims {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("atlas-claims-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        for (f, t) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        let r = read_package(&d).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        r
    }

    #[test]
    fn category_keyword_and_attribute() {
        let p = pkg(&[
            ("Cargo.toml", "[package]\nname = \"x\"\nversion = \"1.0.0\"\nkeywords = [\"no_std\", \"parser\"]\ncategories = [\"no-std\", \"parsing\"]\n"),
            ("src/lib.rs", "//! A parser.\n#![cfg_attr(not(feature = \"std\"), no_std)]\npub fn f() {}\n"),
        ]);
        let kinds: Vec<_> = p.claims.iter().map(|c| (c.kind, c.source)).collect();
        assert!(kinds.contains(&(ClaimKind::NoStd, Source::Category)));
        assert!(kinds.contains(&(ClaimKind::NoStd, Source::Keyword)));
        assert!(kinds.contains(&(ClaimKind::NoStd, Source::Attribute)));
        assert_eq!(p.claims[0].source, Source::Category);
        assert_eq!(p.claims[0].line, Some(5));
        assert_eq!(p.checks(), vec![CheckKind::BareMetal]);
    }

    #[test]
    fn embedded_database_is_not_a_bare_metal_claim() {
        let p = pkg(&[
            ("Cargo.toml", "[package]\nname = \"db\"\nversion = \"1.0.0\"\nkeywords = [\"embedded\", \"database\"]\ndescription = \"An embedded database\"\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
        ]);
        assert!(p.claims.is_empty(), "{:?}", p.claims);
    }

    #[test]
    fn portable_needs_a_platform_word() {
        assert!(prose_portable("A small cross-platform library for retrieving random data"));
        assert!(prose_portable("Portable across Unix and Windows"));
        assert!(!prose_portable("Parse Portable Executable files"));
        assert!(!prose_portable("uses portable-atomic on targets without atomics"));
        assert!(!prose_portable("A portable hash function"));
    }

    #[test]
    fn negations_and_code_blocks_are_skipped() {
        let p = pkg(&[
            ("Cargo.toml", "[package]\nname = \"y\"\nversion = \"0.1.0\"\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("README.md", "# y\n\nThis crate does not support no_std.\n\n```rust\nlet cross_platform = 1; // cross-platform\n```\n\nRuns on QNX 8.0.\nDirect access to sockets, this means little effort to provide\ncross-platform utilities.\n"),
        ]);
        let kinds: Vec<_> = p.claims.iter().map(|c| (c.kind, c.line)).collect();
        assert_eq!(kinds, vec![(ClaimKind::Qnx, Some(9))]);
        assert_eq!(p.checks(), vec![CheckKind::Qnx]);
    }

    #[test]
    fn readme_no_std_support() {
        let p = pkg(&[
            ("Cargo.toml", "[package]\nname = \"z\"\nversion = \"0.1.0\"\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("README.md", "## Features\n\n- `no_std` support (disable default features)\n"),
        ]);
        assert_eq!(p.claims.len(), 1);
        assert_eq!((p.claims[0].kind, p.claims[0].source, p.claims[0].line), (ClaimKind::NoStd, Source::Readme, Some(3)));
    }

    #[test]
    fn no_std_then_extern_std_is_no_claim() {
        assert!(no_std_attribute(
            "#![no_std]
#![doc(x)]

extern crate alloc;
extern crate std;
"
        )
        .is_none());
        assert!(no_std_attribute(
            "#![cfg_attr(not(feature = \"std\"), no_std)]
#[cfg(feature = \"std\")]
extern crate std;
"
        )
        .is_some());
    }

    #[test]
    fn speed_is_not_bare_metal() {
        assert!(!prose_embedded("Tokio's zero-cost abstractions give you bare-metal performance."));
        assert!(prose_embedded("Runs on bare-metal targets such as Cortex-M."));
        assert!(!prose_no_std("- Add `no_std` and `set_no_std` to control the use of `#![no_std]` in probes."));
    }

    #[test]
    fn scope_and_switch_features() {
        assert_eq!(crate_root_scope("#![cfg(windows)]\npub mod x;"), Some("#![cfg(windows)]".into()));
        assert!(crate_root_scope("#![cfg_attr(unix, no_std)]").is_some());
        assert!(crate_root_scope("#![cfg_attr(not(feature = \"std\"), no_std)]").is_none());
        assert_eq!(no_std_switch_features("#![cfg_attr(feature = \"experimental-thread-local\", no_std)]"), vec!["experimental-thread-local"]);
        assert!(no_std_switch_features("#![cfg_attr(not(feature = \"std\"), no_std)]").is_empty());
        assert!(no_std_attribute("#![no_std]\n#[cfg(not(nightly_alloc_io))]\nextern crate std;\n").is_none());
    }

    #[test]
    fn emphasis_and_project_lists() {
        assert!(links_elsewhere("- [fast-paseto](https://x) High-performance, cross-platform tokens"));
        let mut out = Vec::new();
        scan_prose("The `Hash` trait [does **not** guarantee][x] portable results across platforms.\n", "README.md", Source::Readme, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn attribute_only_before_items() {
        assert!(no_std_attribute("#![no_std]\npub fn f(){}").is_some());
        assert!(no_std_attribute("#![cfg_attr(not(test), no_std)]").is_some());
        assert!(no_std_attribute("pub fn f() {}\n// #![no_std]\n").is_none());
        assert!(no_std_attribute("#![forbid(unsafe_code)]\n").is_none());
    }

    #[test]
    fn whole_words() {
        assert!(contains_word("runs on qnx.", "qnx"));
        assert!(!contains_word("qnxsdp", "qnx"));
        assert!(contains_word("is no_std compatible", "no_std"));
        assert!(!contains_word("no_stdlib", "no_std"));
    }
}
