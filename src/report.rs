//! Folds results.jsonl (one line per crate x target attempt) into the published matrix, atlas.json.

use crate::classify::Class;
use crate::cratesio::CrateInfo;
use crate::probe::Outcome;
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub fn load_results(path: &Path) -> Result<Vec<Outcome>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    Ok(text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// results-compat.jsonl: newest compat attempt per (crate, target)
pub fn load_compat(path: &Path) -> HashMap<(String, String), Value> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut m = HashMap::new();
    for l in text.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            let k = (v["crate_name"].as_str().unwrap_or("").to_string(), v["target"].as_str().unwrap_or("").to_string());
            m.insert(k, v);
        }
    }
    m
}

/// newest attempt per (crate, target)
pub fn latest(results: Vec<Outcome>) -> HashMap<(String, String), Outcome> {
    let mut m = HashMap::new();
    for r in results {
        m.insert((r.crate_name.clone(), r.target.clone()), r);
    }
    m
}

#[derive(Serialize, Default, Clone)]
struct Tally {
    attempted: usize,
    pass: usize,
    pass_with_libc_pin: usize,
    fail: usize,
    skip: usize,
    by_class: BTreeMap<String, usize>,
    /// pass 1 failures by class, before the libc pin
    pass1_by_class: BTreeMap<String, usize>,
    /// builds with the libc QNX 8 compatibility patch (pass 1, pin, or compat pass)
    pass_with_libc_compat: usize,
    /// failures that remain after the compat patch, by class
    compat_by_class: BTreeMap<String, usize>,
}

pub fn build(list: &[CrateInfo], results: Vec<Outcome>, compat: &HashMap<(String, String), Value>, targets: &[String], meta: Value, ontarget: Option<Value>) -> Value {
    let latest = latest(results);
    let mut tallies: BTreeMap<String, Tally> = targets.iter().map(|t| (t.clone(), Tally::default())).collect();
    // blocker -> (crates blocked, class counts, targets)
    let mut blockers: HashMap<String, (std::collections::BTreeSet<String>, BTreeMap<String, usize>, std::collections::BTreeSet<String>)> = HashMap::new();
    let mut rows = Vec::new();

    for c in list {
        let mut res = serde_json::Map::new();
        for t in targets {
            let Some(o) = latest.get(&(c.name.clone(), t.clone())) else { continue };
            let tl = tallies.get_mut(t).unwrap();
            tl.attempted += 1;
            match o.status.as_str() {
                "pass" => tl.pass += 1,
                "pass-with-libc-pin" => tl.pass_with_libc_pin += 1,
                "skip" => tl.skip += 1,
                _ => tl.fail += 1,
            }
            if let Some(cl) = o.class {
                if o.status == "fail" {
                    *tl.by_class.entry(cl.slug().into()).or_default() += 1;
                }
            }
            let p1 = o.pass1_class.or(if o.status == "fail" { o.class } else { None });
            if let Some(cl) = p1 {
                if o.status != "skip" {
                    *tl.pass1_by_class.entry(cl.slug().into()).or_default() += 1;
                }
            }
            if o.status == "fail" {
                if let (Some(fe), Some(cl)) = (&o.first_error, o.class) {
                    let key = if fe.package.is_empty() { c.name.clone() } else { fe.package.clone() };
                    let e = blockers.entry(key).or_default();
                    e.0.insert(c.name.clone());
                    *e.1.entry(cl.slug().into()).or_default() += 1;
                    e.2.insert(t.clone());
                }
            }
            let cp = compat.get(&(c.name.clone(), t.clone()));
            let compat_ok = o.status.starts_with("pass") || cp.map(|v| v["status"] == "pass").unwrap_or(false);
            if o.status != "skip" {
                if compat_ok {
                    tl.pass_with_libc_compat += 1;
                } else {
                    let cl = cp.and_then(|v| v["class"].as_str().map(str::to_string)).or(o.class.map(|c| c.slug().to_string())).unwrap_or_else(|| "other".into());
                    *tl.compat_by_class.entry(cl).or_default() += 1;
                }
            }
            res.insert(
                t.clone(),
                json!({
                    "status": o.status,
                    "class": o.class.map(|c| c.slug()),
                    "pass1_class": o.pass1_class.map(|c| c.slug()),
                    "version": o.version,
                    "libc": o.libc,
                    "packages": o.packages,
                    "seconds": (o.seconds * 10.0).round() / 10.0,
                    "error": o.first_error,
                    "compat": cp.map(|v| json!({ "status": v["status"], "class": v["class"], "error": v["first_error"] })),
                }),
            );
        }
        if res.is_empty() {
            continue;
        }
        rows.push(json!({
            "rank": c.rank,
            "name": c.name,
            "downloads": c.downloads,
            "latest": c.max_version,
            "description": c.description,
            "repository": c.repository,
            "results": res,
        }));
    }

    let mut bl: Vec<Value> = blockers
        .into_iter()
        .map(|(pkg, (crates, classes, ts))| {
            let top = classes.iter().max_by_key(|(_, n)| **n).map(|(k, _)| k.clone());
            json!({ "package": pkg, "crates_blocked": crates.len(), "class": top, "targets": ts, "examples": crates.iter().take(8).collect::<Vec<_>>() })
        })
        .collect();
    bl.sort_by(|a, b| b["crates_blocked"].as_u64().cmp(&a["crates_blocked"].as_u64()).then(a["package"].as_str().cmp(&b["package"].as_str())));

    let classes: Vec<Value> = Class::all().iter().map(|c| json!({ "slug": c.slug(), "description": c.describe() })).collect();
    json!({
        "meta": meta,
        "targets": targets,
        "classes": classes,
        "summary": tallies,
        "blockers": bl,
        "crates": rows,
        "ontarget": ontarget,
    })
}
