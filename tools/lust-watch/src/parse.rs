//! The `watch` command line. Only `check` is accepted. Every flag after
//! `check` is kept as the user typed it and handed to `lust check`.

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct CheckCmd {
    /// Absolute path of the file to watch and check.
    pub file: PathBuf,
    /// Absolute `--manifest-path`, when the user passed one.
    pub manifest: Option<PathBuf>,
    /// Absolute `-o` path, when the user passed one. That file is not a source.
    pub output: Option<PathBuf>,
    /// Arguments after the word `check`, unchanged, including the file as typed.
    pub args: Vec<String>,
}

const VALUE_FLAGS: &[&str] = &[
    "-o",
    "--emit-c",
    "--profile",
    "--max-errors",
    "--manifest-path",
    "--package",
    "-p",
    "--bin",
    "--features",
    "-C",
    "--target",
    "--test",
];

pub fn parse_watch(args: &[String]) -> Result<CheckCmd, String> {
    let Some(cmd) = args.first() else {
        return Err(usage());
    };
    if cmd != "check" {
        return Err(format!(
            "lust watch only runs check (no watch {cmd}).\n{}",
            usage()
        ));
    }
    let rest = &args[1..];
    let mut file: Option<String> = None;
    let mut manifest: Option<String> = None;
    let mut output: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if a == "--" {
            break;
        }
        if let Some(v) = a.strip_prefix("--manifest-path=") {
            manifest = Some(v.to_string());
            i += 1;
            continue;
        }
        if let Some(flag) = VALUE_FLAGS.iter().find(|f| *f == a) {
            let Some(v) = rest.get(i + 1) else {
                return Err(format!("lust watch: {flag} needs a value\n{}", usage()));
            };
            if *flag == "--manifest-path" {
                manifest = Some(v.clone());
            } else if *flag == "-o" {
                output = Some(v.clone());
            }
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        if file.is_none() {
            file = Some(a.clone());
        }
        i += 1;
    }
    let Some(file) = file else {
        return Err(format!("lust watch: check needs a source file\n{}", usage()));
    };
    let abs = |p: &str| std::path::absolute(Path::new(p)).unwrap_or_else(|_| PathBuf::from(p));
    Ok(CheckCmd {
        file: abs(&file),
        manifest: manifest.as_deref().map(abs),
        output: output.as_deref().map(abs),
        args: rest.to_vec(),
    })
}

pub fn usage() -> String {
    "\
usage: lust watch check <file.rs> [check flags]
       Re-runs `lust check` with those flags when a source, Cargo.toml, or
       Cargo.lock changes. The working directory, target, features, profile,
       and --sandbox are the same on every check.
       target/, .lust-cache, and other generated output are not watched.
       Ctrl-C stops the watcher and any daemon this session started."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn keeps_check_flags_as_typed() {
        let cmd = parse_watch(&s(&[
            "check",
            "src/main.rs",
            "--sandbox",
            "--features",
            "a,b",
            "--target",
            "x86_64-unknown-linux-gnu",
            "-C",
            "panic=abort",
            "--manifest-path",
            "Cargo.toml",
            "-o",
            "out.bin",
        ]))
        .unwrap();
        assert!(cmd.file.ends_with("src/main.rs"));
        assert!(cmd.manifest.unwrap().ends_with("Cargo.toml"));
        assert!(cmd.output.unwrap().ends_with("out.bin"));
        assert_eq!(
            cmd.args,
            s(&[
                "src/main.rs",
                "--sandbox",
                "--features",
                "a,b",
                "--target",
                "x86_64-unknown-linux-gnu",
                "-C",
                "panic=abort",
                "--manifest-path",
                "Cargo.toml",
                "-o",
                "out.bin",
            ])
        );
    }

    #[test]
    fn rejects_run_and_test() {
        assert!(parse_watch(&s(&["run", "a.rs"])).is_err());
        assert!(parse_watch(&s(&["test", "a.rs"])).is_err());
        assert!(parse_watch(&s(&["build", "a.rs"])).is_err());
        assert!(parse_watch(&s(&["check"])).is_err());
    }
}
