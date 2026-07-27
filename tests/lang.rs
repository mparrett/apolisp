//! Rung 4 of the oracle (BUILD.md): the test suite written in the language.
//!
//! This file is only a runner. Everything it asserts lives in `tests/lang/`,
//! written in apolisp, so it survives implementation churn and doubles as a
//! dogfooding pass — the rung's whole point is to be independent of the
//! internals it tests.
//!
//! Each test file is run through the real binary, so what is under test is the
//! artifact rather than a library call. A failing assertion throws, which ends
//! the run with exit 1 and puts the failing form in the transcript.

use std::path::PathBuf;
use std::process::Command;

mod common;
use common::{bin, repo_root};

/// Every `.xs` file in `tests/lang/` except the harness, sorted so failures
/// report in a stable order (BUILD.md, determinism).
fn suites() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/lang");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/lang missing")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xs"))
        .filter(|p| p.file_name().is_some_and(|n| n != "harness.xs"))
        .collect();
    files.sort();
    files
}

#[test]
fn the_in_language_suite_passes() {
    let harness = std::fs::read_to_string(repo_root().join("tests/lang/harness.xs"))
        .expect("the harness reads");
    let files = suites();
    assert!(!files.is_empty(), "tests/lang has no suites");

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = std::fs::read_to_string(&path).expect("a suite reads");

        // Pasted together rather than required, because there is no `require`
        // and one namespace (ADR-027, Q12). The harness goes first so its
        // macros are defined before a test uses one — expansion is sequential
        // (ADR-040).
        let unit = format!("{harness}\n{src}");
        let joined = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&name);
        std::fs::write(&joined, &unit).expect("the joined unit writes");

        let out = Command::new(bin())
            .arg("run")
            .arg(&joined)
            .output()
            .expect("failed to run apolisp");
        assert!(
            out.status.success(),
            "{name} failed (exit {:?})\n{}{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The runner has to be able to fail, which is not obvious from a suite that
/// passes: a harness that swallowed a throw, or a driver that exited 0 on one,
/// would make every assertion above decorative.
#[test]
fn a_failing_assertion_fails_the_run() {
    let harness = std::fs::read_to_string(repo_root().join("tests/lang/harness.xs"))
        .expect("the harness reads");
    for body in ["(is (= 1 2))", "(is= 1 2)", "(is (throws? 1))"] {
        let joined = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("failing.xs");
        std::fs::write(&joined, format!("{harness}\n{body}\n")).expect("writes");
        let out = Command::new(bin())
            .arg("run")
            .arg(&joined)
            .output()
            .expect("failed to run apolisp");
        assert!(!out.status.success(), "{body} should have failed the run");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(":assertion-failed"),
            "{body}: the transcript should name the failure, got {stdout}"
        );
    }
}
