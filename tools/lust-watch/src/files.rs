//! Which directories are watched, and which paths can start a check.
//!
//! Generated trees stay out of the watch set: `target/`, `.lust-cache/`,
//! `.git/`, `$CARGO_TARGET_DIR`, `$LUST_CACHE_DIR`, and the `-o` output file.

use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

const SKIP_DIRS: &[&str] = &["target", ".lust-cache", ".git"];

#[derive(Debug)]
pub struct Plan {
    /// Directories to watch, non-recursively. Subdirectories are added except
    /// the skipped ones, so a write under `target/` is not an event.
    pub roots: Vec<PathBuf>,
    /// Absolute directory prefixes that are generated output.
    pub exclude: Vec<PathBuf>,
    pub output: Option<PathBuf>,
}

pub fn watch_plan(file: &Path, manifest: Option<&Path>, cwd: &Path) -> Plan {
    let mut exclude = Vec::new();
    for var in ["CARGO_TARGET_DIR", "LUST_CACHE_DIR"] {
        if let Some(v) = std::env::var_os(var) {
            if !v.is_empty() {
                exclude.push(abs(Path::new(&v)));
            }
        }
    }
    let mut roots = Vec::new();
    roots.push(project_root(file, cwd));
    if let Some(parent) = file.parent() {
        roots.push(abs(parent));
    }
    if let Some(manifest) = manifest {
        if let Some(parent) = manifest.parent() {
            roots.push(abs(parent));
        }
    }
    let mut seen = HashSet::new();
    let mut extra = Vec::new();
    for root in &roots {
        path_dep_dirs(root, &mut extra, &mut seen, 0);
    }
    roots.extend(extra);
    roots.retain(|p| p.is_dir());
    roots.sort();
    roots.dedup();
    Plan { roots, exclude, output: None }
}

/// The highest Cargo project directory at or above `file`. When `file` lives
/// under `cwd`, the climb stops at `cwd`, so a manifest above the working
/// directory is not part of the watch set. A directory without a manifest
/// (Cargo's `crates/` folder) does not end the climb.
pub fn project_root(file: &Path, cwd: &Path) -> PathBuf {
    let file_dir = abs(file.parent().unwrap_or(file));
    let cwd = abs(cwd);
    let inside = file_dir.starts_with(&cwd);
    let mut dir = file_dir.clone();
    let mut best = file_dir;
    let mut saw = false;
    let mut misses = 0u32;
    for _ in 0..24 {
        if dir.join("Cargo.toml").is_file() {
            best = dir.clone();
            saw = true;
            misses = 0;
        } else if saw {
            misses += 1;
        }
        if inside && dir == cwd {
            break;
        }
        if saw && misses > 3 {
            break;
        }
        let Some(parent) = dir.parent() else { break };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    best
}

fn path_dep_dirs(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, depth: u32) {
    if depth > 8 || !seen.insert(dir.to_path_buf()) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else { return };
    for rel in toml_path_deps(&text) {
        let dep = abs(&dir.join(rel));
        if dep.is_dir() {
            out.push(dep.clone());
            path_dep_dirs(&dep, out, seen, depth + 1);
        }
    }
}

/// `path = "..."` values in a manifest. Enough for path dependencies; it is
/// not a TOML parser.
pub fn toml_path_deps(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("path") {
        let after = &rest[at + 4..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with('=') {
            rest = &rest[at + 4..];
            continue;
        }
        let value = trimmed[1..].trim_start();
        let Some(q) = value.chars().next() else { break };
        if q != '"' && q != '\'' {
            rest = &rest[at + 4..];
            continue;
        }
        let body = &value[1..];
        let Some(end) = body.find(q) else { break };
        out.push(body[..end].to_string());
        rest = &body[end + 1..];
    }
    out
}

pub fn is_skipped_dir(path: &Path, exclude: &[PathBuf]) -> bool {
    if exclude.iter().any(|ex| path.starts_with(ex)) {
        return true;
    }
    path.components().any(|c| SKIP_DIRS.iter().any(|name| c.as_os_str() == *name))
}

pub fn is_relevant(path: &Path, exclude: &[PathBuf], output: Option<&Path>) -> bool {
    if is_skipped_dir(path, exclude) {
        return false;
    }
    if output.is_some_and(|out| path == out) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with(".lust-daemon-") {
        return false;
    }
    if name == "Cargo.toml" || name == "Cargo.lock" || name == ".lust.toml" {
        return true;
    }
    if name == "config.toml" && path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == ".cargo") {
        return true;
    }
    path.extension().is_some_and(|e| e == "rs")
}

/// A fingerprint of the relevant files under `roots`. Skipped directories
/// are not read. Used to tell a check that raced a save from one that
/// matches the tree still on disk.
pub fn fingerprint(roots: &[PathBuf], exclude: &[PathBuf], output: Option<&Path>) -> u64 {
    let mut files = BTreeSet::new();
    for root in roots {
        collect(root, exclude, output, &mut files);
    }
    let mut h = 0xcbf29ce484222325u64;
    for path in files {
        h = mix(h, hash_bytes(path.as_os_str().as_encoded_bytes()));
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            h = mix(h, meta.len());
            let nanos = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
            h = mix(h, nanos as u64);
            h = mix(h, (nanos >> 64) as u64);
        }
    }
    h
}

fn collect(dir: &Path, exclude: &[PathBuf], output: Option<&Path>, files: &mut BTreeSet<PathBuf>) {
    if is_skipped_dir(dir, exclude) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let path = ent.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect(&path, exclude, output, files);
        } else if is_relevant(&path, exclude, output) {
            files.insert(path);
        }
    }
}

fn mix(h: u64, v: u64) -> u64 {
    h ^ v.wrapping_mul(0x100000001b3)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Absolute path with `.` and `..` collapsed. `std::path::absolute` keeps
/// `..`, so a path dependency `../dep` would not match the dependency's
/// own directory. This does not touch the filesystem and does not add the
/// Windows `\\?\` prefix.
fn abs(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
            Component::RootDir | Component::Prefix(_) => out.push(c.as_os_str()),
        }
    }
    out
}

/// A real directory. Symlinks are not watched, so a link cannot pull the
/// watch set out of the project or into `target/`.
pub fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("lust-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn skips_generated_trees_and_keeps_sources() {
        let root = tmp("rel");
        let src = root.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join(".lust-cache")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let main = src.join("main.rs");
        let cargo = root.join("Cargo.toml");
        let cache = root.join(".lust-cache/x.rs");
        let obj = root.join("target/debug/x.rs");
        std::fs::write(&main, "fn main() {}\n").unwrap();
        std::fs::write(&cargo, "[package]\nname=\"w\"\n").unwrap();
        std::fs::write(&cache, "fn cache() {}\n").unwrap();
        std::fs::write(&obj, "fn obj() {}\n").unwrap();
        let out = root.join("app");
        assert!(is_relevant(&main, &[], None));
        assert!(is_relevant(&cargo, &[], None));
        assert!(!is_relevant(&cache, &[], None));
        assert!(!is_relevant(&obj, &[], None));
        assert!(!is_relevant(&root.join(".lust-daemon-abcd.sock"), &[], None));
        assert!(!is_relevant(&out, &[], Some(&out)));
        assert!(!is_relevant(&root.join("README.md"), &[], None));
        let h1 = fingerprint(&[root.clone()], &[], None);
        std::fs::write(&obj, "fn obj() { let _ = 1; }\n").unwrap();
        std::fs::write(&cache, "fn cache() { let _ = 1; }\n").unwrap();
        assert_eq!(h1, fingerprint(&[root.clone()], &[], None));
        std::fs::write(&main, "fn main() { let _ = 1; }\n").unwrap();
        assert_ne!(h1, fingerprint(&[root.clone()], &[], None));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_root_stops_at_the_working_directory() {
        let root = tmp("root");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let proj = root.join("proj");
        let nested = proj.join("crates/a/src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(proj.join("Cargo.toml"), "[workspace]\nmembers=[\"crates/a\"]\n").unwrap();
        std::fs::write(proj.join("crates/a/Cargo.toml"), "[package]\nname=\"a\"\n").unwrap();
        let file = nested.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let found = project_root(&file, &proj);
        assert_eq!(found, abs(&proj), "a manifest above the working directory stays out of the watch set");
        let outside = project_root(&file, &root.join("elsewhere"));
        assert_eq!(outside, abs(&root), "with no working-directory cap, the watch set is the top Cargo.toml");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_deps_are_roots() {
        let root = tmp("deps");
        let dep = root.join("dep");
        std::fs::create_dir_all(dep.join("src")).unwrap();
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::write(root.join("app/Cargo.toml"), "[dependencies]\ndep = { path = \"../dep\" }\n").unwrap();
        std::fs::write(root.join("app/src/main.rs"), "fn main() {}\n").unwrap();
        let plan = watch_plan(&root.join("app/src/main.rs"), None, &root.join("app"));
        assert!(plan.roots.iter().any(|p| p == &abs(&dep)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
