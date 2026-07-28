//! Milestone 1 verification (BUILD.md).
//!
//! The two properties from ADR-026, the corpus `.forms` and `.spans` snapshots,
//! and the constraint-#1 checks that have no other home yet.
//!
//! Nothing here regenerates a golden file. If a snapshot disagrees, read the
//! diff and decide whether the behaviour change was intended — that decision is
//! the oracle, and automating it away removes the only thing keeping it honest.

use apolisp::error::SpanOrigin;
use apolisp::value::{Interner, LocatedForm, Value};
use apolisp::{printer, reader, value};
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::{bin, check_goldens, corpus_files, repo_root};

// Properties call the library directly (ADR-031), so `read(print(read(s)))` is
// compared on values rather than on printed strings. Snapshots still run the
// binary, because a golden file pins the artifact `just bless` regenerates and
// two producers of the same text would drift.

fn read(src: &str) -> Result<(Vec<LocatedForm>, Interner), String> {
    let mut interner = Interner::new();
    match reader::read_all(src, &mut interner) {
        Ok(forms) => Ok((forms, interner)),
        Err(e) => Err(e.render("<test>", src)),
    }
}

fn print_all(forms: &[LocatedForm], interner: &Interner) -> String {
    let mut out = String::new();
    for f in forms {
        out.push_str(&printer::print(&f.root, interner));
        out.push('\n');
    }
    out
}

// --- Value identity for the round-trip property -----------------------------

/// Structural identity, for asking "did this survive the trip" rather than
/// "does the language consider these equal".
///
/// Deliberately *not* `Value::PartialEq`. Language `=` is Q13's to settle, and
/// under IEEE rules `##NaN` is not equal to itself — a round-trip test built on
/// it would report a false failure for a value that arrived perfectly intact.
/// Floats are therefore compared by bit pattern, which also distinguishes `0.0`
/// from `-0.0` and so catches a printer that drops the sign.
///
/// Symbols and keywords compare by *name*, not by id: the two sides of a
/// round-trip are read into separate interners, where equal ids would mean
/// nothing.
fn same_value(a: &Value, ai: &Interner, b: &Value, bi: &Interner) -> bool {
    use Value::*;
    match (a, b) {
        (Nil, Nil) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x.to_bits() == y.to_bits(),
        (Str(x), Str(y)) => x.0 == y.0,
        (Bytes(x), Bytes(y)) => x.0 == y.0,
        (Sym(x), Sym(y)) => ai.name(x.0) == bi.name(y.0),
        (Keyword(x), Keyword(y)) => ai.name(x.0) == bi.name(y.0),
        (List(_), List(_)) | (Vec(_), Vec(_)) | (Map(_), Map(_)) => {
            let (xs, ys) = (value::children(a), value::children(b));
            xs.len() == ys.len() && xs.iter().zip(&ys).all(|(x, y)| same_value(x, ai, y, bi))
        }
        _ => false,
    }
}

// --- Property: reader round-trip -------------------------------------------

/// `read(print(read(s))) == read(s)`, compared on data and ignoring span
/// origins (ADR-026). Printing moves columns, so a span-sensitive equality here
/// could only ever fail.
fn assert_round_trips(src: &str, label: &str) {
    let (once, oi) = read(src).unwrap_or_else(|e| panic!("{label}: {e}"));
    let printed = print_all(&once, &oi);
    let (twice, ti) = read(&printed)
        .unwrap_or_else(|e| panic!("{label}: printed as {printed:?}, which failed to read: {e}"));

    assert_eq!(
        once.len(),
        twice.len(),
        "{label}: {} forms became {} after a round trip via {printed:?}",
        once.len(),
        twice.len()
    );
    for (i, (a, b)) in once.iter().zip(&twice).enumerate() {
        assert!(
            same_value(&a.root, &oi, &b.root, &ti),
            "{label}: form {i} changed across a round trip\n  printed: {printed:?}\n  \
             before: {} ({})\n  after:  {} ({})",
            printer::print(&a.root, &oi),
            value::kind_name(&a.root),
            printer::print(&b.root, &ti),
            value::kind_name(&b.root),
        );
    }
}

#[test]
fn reader_round_trip() {
    let mut checked = 0;
    for path in corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        assert_round_trips(&src, &path.display().to_string());
        checked += 1;
    }
    assert!(checked >= 4, "corpus shrank to {checked} files");
}

#[test]
fn round_trip_covers_awkward_scalars() {
    // Cases where a naive printer reads back as a *different* value rather than
    // failing outright — the silent half of printer/reader drift.
    for case in [
        "1.0", // must not print as `1`, which reads back as an integer
        "-0.0",
        "1e10",
        "##Inf",
        "##-Inf",
        "##NaN",
        "\"\"",
        "\"a\\nb\"",
        "\"\\\\\"",
        "\"héllo 🙂\"",
        "(1.0 2 \"3\" :4 five)",
        "(δ :χ \"ω\")",
        "{}",
        "()",
        "'()",
        "{:a 1 :b [2 3]}",
    ] {
        assert_round_trips(case, case);
    }
}

/// The type-changing round trip that string comparison could not see.
///
/// `1e400` parsed to infinity, printed as `##Inf`, and read back as a *symbol*
/// named `##Inf`, which printed as `##Inf` again. Both passes produced identical
/// text while the data changed from float to symbol (ADR-032).
#[test]
fn non_finite_floats_do_not_change_type_across_a_round_trip() {
    for (src, want) in [("##Inf", f64::INFINITY), ("##-Inf", f64::NEG_INFINITY)] {
        let (forms, _) = read(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        match forms[0].root {
            Value::Float(f) => assert_eq!(f, want, "{src} read as the wrong float"),
            ref other => panic!("{src} read as {}, not a float", value::kind_name(other)),
        }
    }

    let (forms, _) = read("##NaN").unwrap();
    match forms[0].root {
        Value::Float(f) => assert!(f.is_nan(), "##NaN read as {f}"),
        ref other => panic!("##NaN read as {}", value::kind_name(other)),
    }

    // A finite-looking literal that overflows is an error, not a silent
    // infinity — the same rule the oversized integer already follows.
    let err = read("1e400").expect_err("1e400 should not read");
    assert!(err.contains("overflows to infinity"), "got {err:?}");
}

// --- The hand-written `PartialEq` -------------------------------------------

/// `Value::PartialEq` is hand-written on purpose (`TRAPS.md`) and had no test:
/// it was unreachable from a test suite that compared printed strings.
#[test]
fn value_equality_is_the_hand_written_one() {
    let mut i = Interner::new();
    let (a, b) = (i.sym("x"), i.sym("x"));
    assert_eq!(a, b, "interned symbols with the same name must be equal");
    let k = i.keyword("x");
    // Symbols and keywords share the interner, so `x` and `:x` hold the same
    // id. Equality must not follow the id alone — this is the `TRAPS.md` entry
    // that keeps them separate variants rather than a flag bit.
    assert_ne!(a, k, "symbol `x` must not equal keyword `:x`");

    // Q13: floats never compare equal to integers until it is settled.
    assert_ne!(Value::Int(1), Value::Float(1.0));
    assert_eq!(Value::Int(1), Value::Int(1));

    // Q13 again: IEEE rules, so NaN is not equal to itself. Pinned so that
    // settling Q13 has to change a test rather than pass silently.
    let (forms, _) = read("##NaN").unwrap();
    assert_ne!(forms[0].root, forms[0].root, "language `=` on NaN is IEEE");

    // Q20: no cross-type sequential equality. Widening later is safe.
    let (list, li) = read("(1 2)").unwrap();
    let (vector, _) = read("[1 2]").unwrap();
    assert_ne!(list[0].root, vector[0].root, "list must not equal vector");
    let (same, _) = read("(1 2)").unwrap();
    assert_eq!(list[0].root, same[0].root);
    // The structural comparison the round-trip property uses agrees on the
    // parts of this that are not Q13's or Q20's to decide.
    assert!(same_value(&list[0].root, &li, &same[0].root, &li));
}

// --- Property: span invariants ---------------------------------------------

/// Every `Source` span lies inside its file and on character boundaries, and
/// child-origin arity matches child count (ADR-026).
///
/// The invariant is checked over the whole corpus rather than a sample, because
/// the failure it guards against is a *category* of node having no origin.
fn assert_spans_hold(src: &str, label: &str) {
    let (forms, _) = read(src).unwrap_or_else(|e| panic!("{label}: {e}"));
    let mut problems = Vec::new();
    for f in &forms {
        value::check_origins(&f.root, &f.origins, src, &mut problems);
    }
    assert!(
        problems.is_empty(),
        "{label}: span invariants violated:\n  {}\n--- source\n{src}",
        problems.join("\n  ")
    );
}

#[test]
fn span_invariants_hold_over_corpus() {
    for path in corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        assert_spans_hold(&src, &path.display().to_string());
    }
}

/// Multi-byte characters, which the ASCII-only corpus cannot exercise. A span
/// arithmetic bug is invisible in ASCII, where every offset is a boundary.
#[test]
fn non_ascii_forms_read_with_well_formed_spans() {
    for src in [
        "\"héllo\"",
        "(δ 1)",
        ":χ",
        "{:α \"ω\"}",
        "[é 🙂]",
        "; ω\n(a)",
    ] {
        assert_spans_hold(src, src);
    }
}

/// Origins cover every syntactic child, including immediates (ADR-026). Checked
/// on shape rather than on positions, which the `.spans` goldens pin.
#[test]
fn every_syntactic_child_has_an_origin() {
    let (forms, _) = read("{:a [1 2] :b (c \"d\" 1.0)}").unwrap();
    fn walk(v: &Value, o: &value::Origins) {
        assert_eq!(
            o.children.len(),
            value::child_count(v),
            "{} has {} children but {} origins",
            value::kind_name(v),
            value::child_count(v),
            o.children.len()
        );
        assert!(
            matches!(o.origin, SpanOrigin::Source(_)),
            "{} came from source but its origin is {:?}",
            value::kind_name(v),
            o.origin
        );
        for (c, co) in value::children(v).iter().zip(&o.children) {
            walk(c, co);
        }
    }
    walk(&forms[0].root, &forms[0].origins);
}

// --- Golden snapshots -------------------------------------------------------

/// Rung 3 (BUILD.md). One phase per file; milestone 1 owns `.forms`.
#[test]
fn forms_snapshots_match() {
    check_goldens("read", "forms");
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
    check_goldens("spans", "spans");
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
    // An order of magnitude, not a threshold: ADR-030 puts ±1,000 inside the
    // noise band, so the tripwire sits at the top of that band rather than at
    // the working number. Failing at 7,001 would invite the 40-line debate the
    // ADR exists to prevent.
    // ADR-043 added the row this layer never had: `Image`, fuel, and resume
    // were invented by ADR-029 after the table in `BUILD.md` was written, and
    // ADR-030 raised the total without noticing the layer had no line.
    const BUDGET: usize = 7_500;
    const TRIPWIRE: usize = BUDGET + 1_000;

    let mut src = repo_root();
    src.push("src");
    // `prelude.xs` counts. It is not Rust, but it is core language — `def` and
    // `defmacro` are defined there — and ADR-030 measures what has to be held
    // at once, not what the compiler compiles.
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs" || e == "xs"))
        .collect();
    files.sort();

    // ADR-045: host adapters are outside the budget, and the exclusion prints.
    // `BUILD.md` has said adapters are outside since before any existed, which
    // cost nothing while it described an empty set. The moment it describes
    // real files it becomes a way to move lines out of a budget by moving them
    // into a directory, and the only defence is that the move is visible on
    // every run.
    let mut excluded: Vec<(String, usize)> = Vec::new();
    let adapters = src.join("adapters");
    if let Ok(entries) = std::fs::read_dir(&adapters) {
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            let n = std::fs::read_to_string(&p).unwrap().lines().count();
            excluded.push((
                format!("adapters/{}", p.file_name().unwrap().to_string_lossy()),
                n,
            ));
        }
    }

    let mut total = 0;
    let mut report = String::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        let n = text.lines().count();
        total += n;
        let name = path.file_name().unwrap().to_string_lossy();
        report.push_str(&format!("{name}: {n}\n"));
        for (layer, n) in layers(&text) {
            report.push_str(&format!("  {layer:<10} {n:5}\n"));
        }
    }
    let mut excluded_report = String::new();
    let excluded_total: usize = excluded.iter().map(|(_, n)| n).sum();
    for (name, n) in &excluded {
        excluded_report.push_str(&format!("  {name:<22} {n:5}\n"));
    }
    eprintln!(
        "core: {total}/{BUDGET} lines (ADR-030)\n{report}\n\
         outside the budget (ADR-045): {excluded_total} lines\n{excluded_report}"
    );

    assert!(
        total <= TRIPWIRE,
        "core is {total} lines against a budget of ~{BUDGET} (BUILD.md, ADR-030),\n\
         past the {TRIPWIRE}-line edge of the noise band.\n\
         Raising these constants is an ADR, not an edit.\n{report}"
    );
}

/// Split a file into its inline `pub mod` sections. Reporting only — after
/// extraction into files the per-file totals above carry the same information.
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
    // ADR-025 keeps ADR-010's one surviving clause. E-8 measured 16 bytes
    // against a predicted 24; the assertion is the limit, and the exact size
    // prints so a regression is visible before it crosses the line.
    let n = value::value_size();
    eprintln!("Value: {n} bytes, Origins: {} bytes", value::origins_size());
    assert!(
        n <= value::VALUE_SIZE_LIMIT,
        "Value is {n} bytes against a limit of {} (ADR-025)",
        value::VALUE_SIZE_LIMIT
    );
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
        let err = read(src).expect_err(&format!("{src:?} should not read"));
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
    let err = read(src).expect_err("should not read");
    assert!(
        err.contains(":1:1:"),
        "expected the opener at 1:1, got {err:?}"
    );
}

/// Non-ASCII source must reach a `LispErr`, not a panic.
///
/// The escape span was built by subtracting a byte count from the position
/// after the escaped character, which lands inside any multi-byte one and
/// panics `line_col` while rendering. A reader bug presenting as a crash on the
/// very input it rejects is the worst available failure mode, so the class is
/// pinned here and by the character-boundary span invariant.
#[test]
fn a_multibyte_escape_is_an_error_not_a_panic() {
    for src in ["\"\\€\"", "\"\\é\"", "\"a\\🙂b\""] {
        let err = read(src).expect_err(&format!("{src:?} should not read"));
        assert!(
            err.contains("unknown escape"),
            "{src:?}: expected an unknown-escape error, got {err:?}"
        );
        assert!(
            err.contains(":1:"),
            "{src:?}: error has no line:col — {err:?}"
        );
    }
    // The message names the character the source wrote, not the first byte of
    // its encoding.
    let err = read("\"\\€\"").unwrap_err();
    assert!(err.contains("`\\€`"), "escape misreported: {err:?}");
}

#[test]
fn a_number_that_does_not_fit_is_an_error_not_a_symbol() {
    // Falling through to a symbol would be a silent wrong answer.
    let err = read("99999999999999999999999").expect_err("should not read");
    assert!(err.contains("number"), "got {err:?}");
}

/// The CLI is the artifact the goldens pin, so its contract gets one test:
/// every stage in the pipeline exists, and a stage that cannot run its input
/// fails rather than no-opping (BUILD.md rung 2).
///
/// This replaces the pending-exit-code test. That code distinguished "not built
/// yet" from "ran and failed" while the pipeline had holes; milestone 5 filled
/// the last one, so what is worth pinning now is that there are no holes.
#[test]
fn every_pipeline_stage_runs() {
    let mut path = repo_root();
    path.push("tests/corpus/hello.xs");
    for cmd in ["read", "spans", "expand", "compile", "run"] {
        let out = Command::new(bin())
            .arg(cmd)
            .arg(&path)
            .output()
            .expect("failed to run apolisp");
        assert!(
            out.status.success(),
            "`{cmd}` on hello.xs exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // A stage that exists and cannot run its input fails, and says so.
    let bad = repo_root().join("tests/corpus/does-not-exist.xs");
    let out = Command::new(bin())
        .arg("expand")
        .arg(&bad)
        .output()
        .expect("failed to run apolisp");
    assert!(!out.status.success(), "a missing file must fail");
}
