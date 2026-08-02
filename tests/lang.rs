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
        // The one suite that needs a capability rather than a language feature.
        // ADR-013 gates the filesystem and nothing else here, so this is the
        // only line the subtraction costs the oracle.
        .filter(|p| cfg!(feature = "fs") || p.file_name().is_some_and(|n| n != "io.xs"))
        .collect();
    files.sort();
    files
}

/// ADR-013's claim, as a test rather than a feeling: cutting a host capability
/// removes the *primitive* and leaves the language alone. Only runs in the
/// subtracted build, which is why `just verify` builds one.
#[test]
#[cfg(not(feature = "fs"))]
fn without_fs_the_filesystem_primitive_is_unbound_and_nothing_else_changes() {
    let joined = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("subtracted.xs");
    // `with-open` is a prelude macro over `try`/`finally`, so it still expands;
    // what is missing is the global it calls. A capability that took a language
    // feature with it would fail here at expansion instead.
    std::fs::write(
        &joined,
        "(println (try (io/open \"x\" :read) (catch e (get e :kind))))\n\
         (println (io/write io/stdout \"stdout survives\\n\"))\n",
    )
    .expect("writes");
    let out = Command::new(bin())
        .arg("run")
        .arg(&joined)
        .output()
        .expect("runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the subtracted build still runs\n{stdout}"
    );
    assert!(
        stdout.contains(":unbound"),
        "`io/open` should be an ordinary unbound global, got {stdout}"
    );
    assert!(
        stdout.contains("stdout survives"),
        "stdio is not gated by `fs`, got {stdout}"
    );
}

/// A writable directory the suite can name, emptied per run.
///
/// ADR-042 part 5: the runner injects it rather than the language asking for
/// it, because an `io/temp-dir` primitive would be a line in the 700-line host
/// row that exists only so a test can find a directory this function already
/// knows. Nothing machine-specific escapes — `tests/lang/` compares assertions,
/// not transcripts, so no golden ever sees this path.
fn tmp_dir_decl(name: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("lang-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the suite's temp directory is creatable");
    let text = dir.to_str().expect("a UTF-8 target directory");
    // Emitted into a string literal, so escaping would have to be right.
    // Refusing is better than getting it subtly wrong on a path nobody expects
    // to contain either character.
    assert!(
        !text.contains(['"', '\\']),
        "CARGO_TARGET_TMPDIR needs escaping before it can be injected: {text}"
    );
    format!("(def tmp-dir \"{text}\")\n")
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
        // (ADR-040), and `tmp-dir` goes ahead of both so it is defined before
        // anything reads it.
        let unit = format!("{}{harness}\n{src}", tmp_dir_decl(&name));
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

/// ADR-058. The suites above cannot hold this one: they are run with no
/// arguments, so what a program does with arguments it was *given* is only
/// observable from something that can pass them — which is the driver, and
/// which is this file's one job.
///
/// Three claims, and the third is the one that is quiet when it breaks. The
/// arguments arrive in order; the program's own path is not among them; and a
/// run with no arguments binds the empty vector rather than leaving the name
/// unbound — because an unbound global is a throw and a *nil* would be truthy
/// nowhere and falsy in `if`, so both failures look like program bugs at the
/// first `count`.
#[test]
fn a_program_receives_the_arguments_after_its_path() {
    let harness = std::fs::read_to_string(repo_root().join("tests/lang/harness.xs"))
        .expect("the harness reads");
    let joined = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("args.xs");
    std::fs::write(
        &joined,
        format!(
            "{harness}\n\
             (is= 2 (count *command-line-args*))\n\
             (is= \"alpha\" (nth *command-line-args* 0))\n\
             (is= \"beta\" (nth *command-line-args* 1))\n\
             ; A vector, not a list — the printed forms differ and `=` does not\n\
             ; (ADR-041), so this is the only assertion here that can see it.\n\
             (is= \"[\\\"alpha\\\" \\\"beta\\\"]\" (str *command-line-args*))\n"
        ),
    )
    .expect("writes");

    let out = Command::new(bin())
        .arg("run")
        .arg(&joined)
        .arg("alpha")
        .arg("beta")
        .output()
        .expect("runs");
    assert!(
        out.status.success(),
        "arguments should reach the program\n{}",
        String::from_utf8_lossy(&out.stdout)
    );

    // The same file with nothing after it. `count` is what makes the empty
    // vector distinguishable from a missing one: an unbound `*command-line-args*`
    // throws before `count` is reached, and the transcript says which.
    std::fs::write(
        &joined,
        format!("{harness}\n(is= 0 (count *command-line-args*))\n"),
    )
    .expect("writes");
    let out = Command::new(bin())
        .arg("run")
        .arg(&joined)
        .output()
        .expect("runs");
    assert!(
        out.status.success(),
        "no arguments should be the empty vector, not unbound\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
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
