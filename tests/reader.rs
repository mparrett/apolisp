//! Milestone 1 verification (BUILD.md).
//!
//! The two properties from ADR-026, the corpus `.forms` snapshots, and the
//! constraint-#1 checks that have no other home yet.
//!
//! Nothing here regenerates a golden file. If a snapshot disagrees, read the
//! diff and decide whether the behaviour change was intended — that decision is
//! the oracle, and automating it away removes the only thing keeping it honest.

use std::path::{Path, PathBuf};
use std::process::Command;

// The binary is the interface under test for snapshots; the properties want the
// library, which a `main.rs`-only crate does not expose. Until there is a
// `lib.rs`, drive both through the binary and keep the properties in terms of
// its observable output.

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin() -> PathBuf {
    let mut p = repo_root();
    p.push("target");
    p.push(if cfg!(debug_assertions) { "debug" } else { "release" });
    p.push("apolisp");
    p
}

fn read_cmd(path: &Path) -> Result<String, String> {
    let out = Command::new(bin())
        .arg("read")
        .arg(path)
        .output()
        .expect("failed to run apolisp; `cargo build` first");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

/// Read a temporary file's worth of source, without leaving one behind on
/// failure.
///
/// The name carries a per-call counter as well as the pid: cargo runs tests in
/// parallel threads of one process, so a pid-only name has every test writing
/// the same path and reading back someone else's source.
fn read_str(src: &str) -> Result<String, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("apolisp-prop-{}-{n}.xs", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let r = read_cmd(&path);
    let _ = std::fs::remove_file(&path);
    r
}

fn corpus_files() -> Vec<PathBuf> {
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

// --- Property: reader round-trip -------------------------------------------

/// `read(print(read(s))) == read(s)`, compared on data and ignoring span
/// origins (ADR-026). Printing moves columns, so a span-sensitive equality here
/// could only ever fail.
///
/// Comparing printed output is the available proxy for comparing values while
/// the crate has no library target: two forms print identically iff the printer
/// is a function of the data alone, which is the property being pinned.
#[test]
fn reader_round_trip() {
    let mut checked = 0;
    for path in corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let once = read_cmd(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let twice = read_str(&once)
            .unwrap_or_else(|e| panic!("{}: reprinting failed to read back: {e}", path.display()));
        assert_eq!(
            once,
            twice,
            "round-trip diverged for {}\n--- source\n{src}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 4, "corpus shrank to {checked} files");
}

#[test]
fn round_trip_covers_awkward_scalars() {
    // Cases where a naive printer reads back as a *different* value rather than
    // failing outright — the silent half of printer/reader drift.
    for case in [
        "1.0",         // must not print as `1`, which reads back as an integer
        "-0.0",
        "1e10",
        "\"\"",
        "\"a\\nb\"",
        "\"\\\\\"",
        "(1.0 2 \"3\" :4 five)",
        "{}",
        "()",
        "'()",
    ] {
        let once = read_str(case).unwrap_or_else(|e| panic!("{case:?}: {e}"));
        let twice = read_str(&once).unwrap_or_else(|e| panic!("{case:?} -> {once:?}: {e}"));
        assert_eq!(once, twice, "round-trip diverged for {case:?}");
    }
}

// --- Property: span invariants ---------------------------------------------

/// Every `Source` span lies inside its file, and child-origin arity matches
/// child count (ADR-026).
///
/// This runs through a debug rendering the binary exposes for the purpose; the
/// invariant is checked over the whole corpus rather than a sample, because the
/// failure it guards against is a *category* of node having no origin at all.
#[test]
fn span_invariants_hold_over_corpus() {
    for path in corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let out = Command::new(bin())
            .arg("spans")
            .arg(&path)
            .output()
            .expect("failed to run apolisp");
        assert!(
            out.status.success(),
            "{}: spans failed: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(
            report.contains("ok"),
            "{}: span invariants violated:\n{report}\n--- source\n{src}",
            path.display()
        );
    }
}

/// The arity half of the span invariants is checkable structurally; the *values*
/// are not, so they get pinned as goldens instead (ADR-026 point 3).
///
/// This exists because a mutation check proved the structural test alone was
/// dead: making every origin `Unknown`, or forcing every span to start at 0,
/// left the whole suite green. That is the `../reg-lisp` failure from
/// `PRIOR-ART.md`, reproduced here before anything depended on it.
#[test]
fn spans_snapshots_match() {
    let mut missing = Vec::new();
    let mut diffs = Vec::new();

    for path in corpus_files() {
        let out = Command::new(bin())
            .arg("spans")
            .arg(&path)
            .output()
            .expect("failed to run apolisp");
        assert!(out.status.success(), "{}: spans failed", path.display());
        let actual = String::from_utf8_lossy(&out.stdout).into_owned();
        let golden = path.with_extension("spans");
        match std::fs::read_to_string(&golden) {
            Err(_) => missing.push(golden),
            Ok(expected) if expected != actual => diffs.push(format!(
                "--- {}\nexpected:\n{expected}\nactual:\n{actual}",
                golden.display()
            )),
            Ok(_) => {}
        }
    }

    assert!(missing.is_empty(), "missing span goldens: {missing:?}");
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

// --- Golden snapshots -------------------------------------------------------

/// Rung 3 (BUILD.md). One phase per file; milestone 1 owns `.forms`.
#[test]
fn forms_snapshots_match() {
    let mut missing = Vec::new();
    let mut diffs = Vec::new();

    for path in corpus_files() {
        let actual = match read_cmd(&path) {
            Ok(s) => s,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        let golden = path.with_extension("forms");
        match std::fs::read_to_string(&golden) {
            Err(_) => missing.push((golden, actual)),
            Ok(expected) if expected != actual => {
                diffs.push(format!(
                    "--- {}\nexpected:\n{expected}\nactual:\n{actual}",
                    golden.display()
                ));
            }
            Ok(_) => {}
        }
    }

    // A missing golden file is a failure with instructions, never a silent
    // write. Creating it automatically would mean the first run of a broken
    // reader pins the broken behaviour.
    if !missing.is_empty() {
        let names: Vec<String> = missing
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        panic!(
            "missing golden files: {}\nreview the output and write them by hand \
             (`apolisp read <file>`), or run `just bless` once you have read the diff",
            names.join(", ")
        );
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

// --- Constraint #1 ----------------------------------------------------------

/// The line budget is the only governing constraint with no test (BUILD.md).
/// This is that test.
///
/// Every line counts, comments and blanks included: constraint #1 is about what
/// has to be held at once (ADR-030). Only the total is asserted — per-layer
/// numbers print, because a hard check at 300-line granularity would be false
/// precision. The question a layer answers is "did this double," not "is this
/// 40 over."
#[test]
fn core_stays_within_the_line_budget() {
    // An order of magnitude, not a threshold. Over budget is an ADR, not a
    // nudge to this constant.
    const BUDGET: usize = 7_000;

    let mut src = repo_root();
    src.push("src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();

    let mut total = 0;
    let mut report = String::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        total += text.lines().count();
        for (name, n) in layers(&text) {
            report.push_str(&format!("  {name:<10} {n:5}\n"));
        }
    }
    eprintln!("core: {total}/{BUDGET} lines (ADR-030)\n{report}");

    assert!(
        total <= BUDGET,
        "core is {total} lines against a budget of ~{BUDGET} (BUILD.md, ADR-030).\n\
         Raising this constant is an ADR, not an edit.\n{report}"
    );
}

/// Split a file into its inline `pub mod` sections. Reporting only — the
/// boundaries survive extraction into files, at which point this reads the
/// filenames instead (ADR-015).
fn layers(text: &str) -> Vec<(String, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut marks: Vec<(usize, String)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(rest) = l.strip_prefix("pub mod ") {
            if let Some(name) = rest.strip_suffix(" {") {
                marks.push((i, name.to_string()));
            }
        }
    }
    let mut out = Vec::new();
    if let Some((first, _)) = marks.first() {
        out.push(("(driver)".to_string(), *first));
    }
    for (k, (start, name)) in marks.iter().enumerate() {
        let end = marks.get(k + 1).map(|(i, _)| *i).unwrap_or(lines.len());
        out.push((name.clone(), end - start));
    }
    out
}

#[test]
fn value_size_is_asserted_not_assumed() {
    // ADR-025 keeps ADR-010's one surviving clause. Checked in the binary,
    // where the type lives; this test pins that the check is actually wired up.
    let out = Command::new(bin())
        .arg("sizes")
        .output()
        .expect("failed to run apolisp");
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "size assertion failed: {report}");
    assert!(report.contains("Value"), "unexpected report: {report}");
}

// --- Errors -----------------------------------------------------------------

/// Failure cases belong in the corpus from milestone 4 (BUILD.md), but reader
/// errors exist now and their positions are the first thing ADR-026 buys.
#[test]
fn reader_errors_carry_a_position() {
    for (src, want) in [
        ("(1 2", "unclosed `(`"),
        ("[1 2", "unclosed `[`"),
        ("{:a", "unclosed `{`"),
        ("{:a 1 :b}", "no value"),
        ("\"unterminated", "unclosed string"),
        (")", "unmatched `)`"),
        (":", "`:` with no name"),
    ] {
        let err = read_str(src).expect_err(&format!("{src:?} should not read"));
        assert!(
            err.contains(want),
            "{src:?}: expected {want:?}, got {err:?}"
        );
        assert!(
            err.contains(":1:"),
            "{src:?}: error has no line:col — {err:?}"
        );
    }
}

#[test]
fn unclosed_delimiter_points_at_the_opener() {
    // In a long file the opening delimiter is the only position that says
    // anything; pointing at EOF is technically true and useless.
    let src = "(a\nb\nc\nd\n";
    let err = read_str(src).expect_err("should not read");
    assert!(err.contains(":1:1:"), "expected the opener at 1:1, got {err:?}");
}

#[test]
fn a_number_that_does_not_fit_is_an_error_not_a_symbol() {
    // Falling through to a symbol would be a silent wrong answer.
    let err = read_str("99999999999999999999999").expect_err("should not read");
    assert!(err.contains("number"), "got {err:?}");
}
