//! The bits of the harness both test files need: where the repository is, which
//! binary cargo built, and how a golden file is checked.
//!
//! Each file in `tests/` is its own crate, so anything only one of them uses
//! looks dead to the other. That is what the allow is for, and it is the price
//! of sharing a harness rather than copying it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The binary cargo built for *this* test run. Reconstructing
/// `target/debug/apolisp` by hand runs a stale artifact under a custom
/// `CARGO_TARGET_DIR`, which is a green suite testing yesterday's code.
pub fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_apolisp")
}

pub fn run_cmd(cmd: &str, path: &Path) -> Result<String, String> {
    // From the repository root, with a repository-relative path. A diagnostic
    // prints the path it was given (`--- at`, since ADR-039), so running this
    // with the absolute path the harness builds would put the checkout
    // directory in a golden file — and a golden that differs per machine is the
    // determinism failure BUILD.md is emphatic about. It also makes the harness
    // and `just bless` pass byte-identical arguments.
    let relative = path.strip_prefix(repo_root()).unwrap_or(path);
    let out = Command::new(bin())
        .current_dir(repo_root())
        .arg(cmd)
        .arg(relative)
        .output()
        .expect("failed to run apolisp");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // A program that failed is still a program with a transcript (BUILD.md:
    // `.out` records the exit status, so a failure has to be able to reach the
    // file). The driver exits 1 for "the program failed" and 2 or 3 for "the
    // driver could not run it" — only the second kind has nothing to compare.
    if out.status.success() || (out.status.code() == Some(1) && !stdout.is_empty()) {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

pub fn corpus_files() -> Vec<PathBuf> {
    let mut dir = repo_root();
    dir.push("tests/corpus");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/corpus missing")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xs"))
        .collect();
    // Directory order is not deterministic across machines, and a test that
    // reports failures in a different order each run is a test people stop
    // reading (BUILD.md, determinism).
    files.sort();
    files
}

/// Rung 3 (BUILD.md): one phase per file, compared against a committed
/// snapshot. Nothing here regenerates one — if a snapshot disagrees, read the
/// diff and decide whether the behaviour change was intended. That decision is
/// the oracle, and automating it away removes the only thing keeping it honest.
pub fn check_goldens(cmd: &str, ext: &str) {
    check_goldens_over(cmd, ext, corpus_files());
}

/// The same, over a chosen subset. Milestone 3 needs it: only some corpus
/// programs run, and which ones is a decision to state rather than a set to
/// infer from whichever golden files happen to exist.
pub fn check_goldens_over(cmd: &str, ext: &str, files: Vec<std::path::PathBuf>) {
    let mut missing = Vec::new();
    let mut diffs = Vec::new();

    for path in files {
        let actual = run_cmd(cmd, &path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let golden = path.with_extension(ext);
        match std::fs::read_to_string(&golden) {
            Err(_) => missing.push(golden),
            Ok(expected) if expected != actual => diffs.push(format!(
                "--- {}\nexpected:\n{expected}\nactual:\n{actual}",
                golden.display()
            )),
            Ok(_) => {}
        }
    }

    // A missing golden file is a failure with instructions, never a silent
    // write. Creating it automatically would mean the first run of a broken
    // phase pins the broken behaviour.
    if !missing.is_empty() {
        let names: Vec<String> = missing
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        panic!(
            "missing golden files: {}\nreview the output and write them by hand \
             (`apolisp {cmd} <file>`), or run `just bless` once you have read the diff",
            names.join(", ")
        );
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
