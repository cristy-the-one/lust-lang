//! End-to-end scan of a tiny fixture crate built in a temp directory.
//! Covers module walking (`mod`, `#[path]`), cfg scope labels, an
//! undecidable cfg, an unparseable file, the coverage curve, and the
//! dependency decision table.

use std::path::PathBuf;
use std::process::Command;

fn write(root: &PathBuf, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn fixture(name: &str) -> PathBuf {
    // One directory per test: cargo runs tests in parallel threads in
    // one process, so the pid alone is not unique.
    let root = std::env::temp_dir().join(format!("lust-port-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"fixture1\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nindexmap = \"2\"\n",
    );
    write(
        &root,
        "src/lib.rs",
        "pub mod sub;\n#[path = \"elsewhere.rs\"]\npub mod elsewhere;\n#[cfg(windows)]\npub mod win_only;\n#[cfg(feature = \"x\")]\npub mod feat;\npub mod broken;\n#[cfg(test)]\npub mod tests_only;\npub fn f() -> usize { #[used] static M: usize = 3; M }\npub fn h(cb: for<'a> fn(&'a i32) -> i32) -> i32 { cb(&1) }\n",
    );
    write(&root, "src/sub/mod.rs", "pub fn h() -> Box<dyn Fn(i32) -> i32> { Box::new(|x| x) }\n");
    write(&root, "src/elsewhere.rs", "pub const N: usize = 1;\n");
    write(&root, "src/win_only.rs", "pub fn w() { #[used] static M: usize = 3; }\n");
    write(&root, "src/feat.rs", "pub fn q() {}\n");
    write(&root, "src/tests_only.rs", "pub fn t() { #[used] static M: usize = 3; }\n");
    write(&root, "src/broken.rs", "pub fn broken( {\n");
    root
}

fn scan_json(root: &PathBuf) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_lust-port"))
        .args(["scan", "--manifest-path", root.join("Cargo.toml").to_str().unwrap(), "--target", "x86_64-unknown-linux-gnu", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "scan failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn scan_reports_gaps_and_survives_bad_files() {
    let root = fixture("gaps");
    let v = scan_json(&root);

    // All 7 files are found by walking mods, including #[path].
    let files: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    for want in ["src/lib.rs", "src/sub/mod.rs", "src/elsewhere.rs", "src/win_only.rs", "src/feat.rs", "src/tests_only.rs", "src/broken.rs"]
    {
        assert!(files.contains(&want), "missing {want} in {files:?}");
    }

    // The unparseable file is recorded and the scan still finished.
    assert_eq!(v["totals"]["parse_errors"], 1);
    assert_eq!(v["parse_errors"][0]["file"], "src/broken.rs");

    // feature="x" decides false: this package declares no features, so
    // its effective set is known and empty.
    let und: Vec<&str> = v["undecidable_cfg"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    assert!(!und.iter().any(|u| u.contains("feature")), "undecidable: {und:?}");
    assert_eq!(v["cargo_features"]["fixture1"], serde_json::json!([]));

    let findings = v["findings"].as_array().unwrap();
    let has = |feature: &str, file: &str, scope: &str| {
        findings.iter().any(|f| f["feature"] == feature && f["file"] == file && f["scope"] == scope)
    };
    assert!(has("attrs.unsupported", "src/lib.rs", "active"));
    assert!(has("attrs.unsupported", "src/win_only.rs", "target-specific"));
    assert!(has("attrs.unsupported", "src/tests_only.rs", "test-only"));
    // Box<dyn Fn> in sub/mod.rs is supported today (dyn.fn and
    // dyn.boxed are supported rows), so it makes no finding.
    assert!(!has("dyn.fn", "src/sub/mod.rs", "active"));
    assert!(!has("dyn.boxed", "src/sub/mod.rs", "active"));
    // for<'a> fn pointers are supported today (lifetimes.hrtb), so the
    // lib.rs `h` fn makes no finding.
    assert!(!has("lifetimes.hrtb", "src/lib.rs", "active"));

    // Every finding carries a byte range inside its file and a snippet.
    for f in findings {
        let text = std::fs::read_to_string(root.join(f["file"].as_str().unwrap())).unwrap();
        let (s, e) = (f["byte_start"].as_u64().unwrap() as usize, f["byte_end"].as_u64().unwrap() as usize);
        assert!(s <= e && e <= text.len(), "bad range {f}");
        assert!(f["line"].as_u64().unwrap() >= 1);
        assert!(!f["fix_class"].as_str().unwrap().is_empty());
    }

    // Findings are grouped by root cause.
    let causes: Vec<&str> = v["root_causes"].as_array().unwrap().iter().map(|g| g["root_cause"].as_str().unwrap()).collect();
    assert!(causes.contains(&"attrs-cfg"), "causes: {causes:?}");

    // elsewhere.rs (a const) and sub/mod.rs (Box<dyn Fn>) compile now;
    // feat.rs is cfg(feature="x")-off, so it is target-specific and out
    // of the active coverage universe. lib.rs needs milestone 4 for
    // its #[used] static (attrs.unsupported).
    let feat = v["files"].as_array().unwrap().iter().find(|f| f["file"] == "src/feat.rs").unwrap();
    assert!(feat["missing_features"].as_array().unwrap().is_empty(), "{feat}");
    assert_eq!(feat["scope"], "target-specific", "{feat}");
    assert_eq!(v["coverage"]["supported_today_files"], 2);
    let today = &v["coverage"]["by_plan_milestone"][0];
    let m6 = &v["coverage"]["by_plan_milestone"][3];
    assert_eq!(today["id"], "0");
    assert_eq!(m6["id"], "6");
    assert!(m6["files"].as_u64().unwrap() > today["files"].as_u64().unwrap());
    // The broken file blocks every milestone.
    let m8 = v["coverage"]["by_plan_milestone"].as_array().unwrap().last().unwrap();
    assert!(m8["files"].as_u64().unwrap() < v["coverage"]["total_files"].as_u64().unwrap());

    // The dependency report uses the decision table.
    let deps = v["dependencies"].as_array().unwrap();
    let indexmap = deps.iter().find(|d| d["name"] == "indexmap").expect("indexmap dep");
    assert_eq!(indexmap["decision"], "builtin-adapter");
    assert_eq!(indexmap["milestone"], 5);
}

#[test]
fn human_output_groups_findings() {
    let root = fixture("human");
    let out = Command::new(env!("CARGO_BIN_EXE_lust-port"))
        .args(["scan", "--manifest-path", root.join("Cargo.toml").to_str().unwrap(), "--target", "x86_64-unknown-linux-gnu"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Gaps by root cause"), "{text}");
    assert!(text.contains("attrs.unsupported"));
    // Grouped, not one line per finding: only 2 samples per feature.
    let attr_samples = text.matches("attrs.unsupported").count();
    assert!(attr_samples <= 3, "too many attr lines: {attr_samples}");
}

#[test]
fn bare_names_are_locals_and_serde_paths_are_named() {
    // One directory per test, as in `fixture`.
    let root = std::env::temp_dir().join(format!("lust-port-test-{}-serde", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write(&root, "Cargo.toml", "[package]\nname = \"fixture2\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    write(
        &root,
        "src/lib.rs",
        "pub mod bare;\nuse serde::Deserialize;\n#[derive(Debug, Deserialize)]\npub struct Cfg { pub name: String }\npub fn read(s: &str) -> String { serde_json::from_str::<String>(s).unwrap_or_default() }\n",
    );
    // `notify` and `same_file` are crate names in the seed list, but
    // here they are a local binding and a local function. A bare
    // one-segment path is never a crate root (w-tail).
    write(&root, "src/bare.rs", "fn same_file() -> bool { true }\npub fn g(notify: i32) -> i32 { if same_file() { notify } else { 0 } }\n");
    let v = scan_json(&root);
    let file = |name: &str| v["files"].as_array().unwrap().iter().find(|f| f["file"] == name).unwrap().clone();
    let missing = |f: &serde_json::Value| {
        f["missing_features"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    let lib = missing(&file("src/lib.rs"));
    assert!(lib.contains(&"ext.serde".to_string()), "lib missing: {lib:?}");
    assert!(lib.contains(&"ext.serde_json".to_string()), "lib missing: {lib:?}");
    assert!(!lib.contains(&"ext.unknown_crate".to_string()), "lib missing: {lib:?}");
    assert!(!lib.contains(&"derives.unknown".to_string()), "lib missing: {lib:?}");
    let bare = missing(&file("src/bare.rs"));
    assert!(bare.is_empty(), "bare locals blocked the file: {bare:?}");
    // The dependency report is separate from path use: no serde in
    // this manifest, so no dependency rows appear for it.
    let deps = v["dependencies"].as_array().unwrap();
    assert!(!deps.iter().any(|d| d["name"] == "serde"), "deps: {deps:?}");
}

/// The ts-rust jemalloc shape: `default = ["heavy", "fast"]` with
/// `[package.metadata.lust] dev-exclude-features = ["heavy"]`. Code
/// under `cfg(feature = "heavy")` (an item, a block, and a whole mod
/// file) must not block the active files, and `--cargo-features`
/// must bring it back for one scan.
#[test]
fn dev_config_excludes_a_default_feature() {
    let root = std::env::temp_dir().join(format!("lust-port-test-{}-devfeat", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"devfeat\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.lust]\ndev-exclude-features = [\"heavy\"]\n\n[features]\ndefault = [\"heavy\", \"fast\"]\nheavy = []\nfast = []\n",
    );
    write(
        &root,
        "src/lib.rs",
        "#[cfg(feature = \"heavy\")]\npub mod heavy;\npub fn f() -> usize {\n    #[cfg(feature = \"heavy\")]\n    { #[used] static M: usize = 3; }\n    #[cfg(not(feature = \"heavy\"))]\n    { }\n    1\n}\n#[cfg(feature = \"heavy\")]\npub fn g() { #[used] static M: usize = 3; }\n",
    );
    write(&root, "src/heavy.rs", "pub fn w() { #[used] static M: usize = 3; }\n");
    let manifest = root.join("Cargo.toml");
    let scan = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["scan", "--manifest-path", manifest.to_str().unwrap(), "--target", "x86_64-unknown-linux-gnu", "--json"];
        args.extend(extra);
        let out = Command::new(env!("CARGO_BIN_EXE_lust-port")).args(&args).output().unwrap();
        assert!(out.status.success(), "scan failed: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let file = |v: &serde_json::Value, name: &str| v["files"].as_array().unwrap().iter().find(|f| f["file"] == name).unwrap().clone();
    let v = scan(&[]);
    assert_eq!(v["cargo_features"]["devfeat"], serde_json::json!(["fast"]));
    let lib = file(&v, "src/lib.rs");
    assert!(lib["missing_features"].as_array().unwrap().is_empty(), "lib: {lib}");
    assert_eq!(file(&v, "src/heavy.rs")["scope"], "target-specific");
    // The override turns heavy back on: lib.rs is blocked again.
    let v = scan(&["--cargo-features", "heavy,fast"]);
    assert_eq!(v["cargo_features"]["devfeat"], serde_json::json!(["fast", "heavy"]));
    let lib = file(&v, "src/lib.rs");
    assert!(lib["missing_features"].as_array().unwrap().iter().any(|x| x == "attrs.unsupported"), "lib: {lib}");
    assert_eq!(file(&v, "src/heavy.rs")["scope"], "active");
}

#[test]
fn unknown_package_is_an_error() {
    let root = fixture("human");
    let out = Command::new(env!("CARGO_BIN_EXE_lust-port"))
        .args(["scan", "--manifest-path", root.join("Cargo.toml").to_str().unwrap(), "-p", "nope"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not found"));
}
