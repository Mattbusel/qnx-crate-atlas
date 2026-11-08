//! Pulls the most downloaded crates from the crates.io API (one request per second, named user agent,
//! as the crates.io data access policy asks).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const USER_AGENT: &str = "qnx-crate-atlas/0.1 (https://github.com/Mattbusel/qnx-crate-atlas; mattbusel@tensorust.com)";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateInfo {
    pub rank: usize,
    pub name: String,
    pub max_version: String,
    pub downloads: u64,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    crates: Vec<ApiCrate>,
}

#[derive(Deserialize)]
struct ApiCrate {
    name: String,
    #[serde(default)]
    max_stable_version: Option<String>,
    max_version: String,
    downloads: u64,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    repository: Option<String>,
}

pub fn top_crates(count: usize) -> Result<Vec<CrateInfo>> {
    let mut out: Vec<CrateInfo> = Vec::new();
    let per = 100usize;
    let pages = count.div_ceil(per);
    for page in 1..=pages {
        let url = format!("https://crates.io/api/v1/crates?page={page}&per_page={per}&sort=downloads");
        let body: Page = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("GET {url}"))?
            .body_mut()
            .read_json()
            .context("parsing crates.io response")?;
        for c in body.crates {
            if out.len() >= count {
                break;
            }
            out.push(CrateInfo {
                rank: out.len() + 1,
                name: c.name,
                max_version: c.max_stable_version.unwrap_or(c.max_version),
                downloads: c.downloads,
                description: c.description.map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ")),
                repository: c.repository,
            });
        }
        if page < pages {
            std::thread::sleep(Duration::from_millis(1100));
        }
    }
    Ok(out)
}
