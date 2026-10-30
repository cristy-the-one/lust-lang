//! Reads `cargo metadata` JSON (no cargo_metadata crate, plain serde).
//! Uses `--no-deps` so a scan never needs the network, and `--locked`
//! when a Cargo.lock already exists so a read-only tree is not rewritten.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // target_directory is parsed for future cache use
pub struct Metadata {
    pub packages: Vec<Package>,
    #[serde(default)]
    pub workspace_members: Vec<String>,
    pub workspace_root: PathBuf,
    #[serde(default)]
    pub target_directory: PathBuf,
    /// `[workspace.metadata]` as raw JSON (the lust dev config reads it).
    #[serde(default)]
    pub workspace_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // version and edition are part of the cargo schema we may report later
pub struct Package {
    pub name: String,
    pub version: String,
    pub id: String,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub edition: String,
    /// The `[features]` table: feature name to its entries.
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    /// `[package.metadata]` as raw JSON (the lust dev config reads it).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // crate_types is part of the cargo schema
pub struct Target {
    pub name: String,
    #[serde(default)]
    pub kind: Vec<String>,
    #[serde(default)]
    pub crate_types: Vec<String>,
    pub src_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // uses_default_features is part of the cargo schema
pub struct Dependency {
    pub name: String,
    #[serde(default)]
    pub req: String,
    /// Null for normal dependencies, else "dev" or "build".
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub rename: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub uses_default_features: bool,
}

pub fn load(manifest_path: &Path) -> Result<Metadata, String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"]);
    cmd.arg(manifest_path);
    if find_lock(manifest_path).is_some() {
        cmd.arg("--locked");
    }
    let out = cmd.output().map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cannot parse cargo metadata output: {e}"))
}

fn find_lock(manifest_path: &Path) -> Option<PathBuf> {
    let mut dir = manifest_path.parent();
    for _ in 0..6 {
        let d = dir?;
        let lock = d.join("Cargo.lock");
        if lock.exists() {
            return Some(lock);
        }
        dir = d.parent();
    }
    None
}

/// Packages to scan: `-p` wins, then the package that owns the manifest,
/// then every workspace member (virtual manifests).
pub fn select<'m>(meta: &'m Metadata, manifest_path: &Path, pkg: Option<&str>) -> Result<Vec<&'m Package>, String> {
    if let Some(name) = pkg {
        let found: Vec<&Package> = meta.packages.iter().filter(|p| p.name == name).collect();
        if found.is_empty() {
            let names: Vec<&str> = meta.packages.iter().map(|p| p.name.as_str()).collect();
            return Err(format!("package `{name}` not found; packages here: {}", names.join(", ")));
        }
        return Ok(found);
    }
    let want = std::path::absolute(manifest_path).unwrap_or_else(|_| manifest_path.to_path_buf());
    let own: Vec<&Package> = meta
        .packages
        .iter()
        .filter(|p| std::path::absolute(&p.manifest_path).unwrap_or_else(|_| p.manifest_path.clone()) == want)
        .collect();
    if !own.is_empty() {
        return Ok(own);
    }
    let members: Vec<&Package> = meta.packages.iter().filter(|p| meta.workspace_members.contains(&p.id)).collect();
    if members.is_empty() {
        return Err("no package selected: pass -p <package>".to_string());
    }
    Ok(members)
}
