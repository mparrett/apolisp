//! Milestone 9 (BUILD.md): the REPL.
//!
//! The stated exit condition — "becomes the primary development interface" —
//! is not testable as written, so what is tested here is the four things
//! ADR-044 actually promises, each of which is false in an obvious REPL and
//! each of which someone would notice within a minute of using one:
//!
//! - a function defined in one input is callable from a later input (Q29),
//! - a macro defined in one input is in scope in the next,
//! - the gensym counter never restarts, so no two inputs mint the same name,
//! - a thrown value does not end the session.
//!
//! The semantics are exercised through `Session`, because that is where they
//! live (ADR-031). The prompt itself is driven through the binary, because
//! buffering a half-typed form is the driver's job and nothing else can see it.

use apolisp::printer;
use apolisp::session::{self, Ended, Session};
use std::io::Write;
use std::process::{Command, Stdio};

mod common;
use common::bin;

/// Evaluate one input and render it the way the prompt would.
fn eval(s: &mut Session, src: &str) -> String {
    match s.eval(src) {
        Ok(ended) => {
            let out = s.take_output();
            let tail = match ended {
                Ended::Value(v) => printer::print(&v, &s.vm.interner),
                Ended::Threw(u) => format!("threw {}", printer::print(&u.value, &s.vm.interner)),
            };
            format!("{out}{tail}")
        }
        Err(e) => format!("error {}", e.msg),
    }
}

// --- What a session keeps between inputs -----------------------------------------

/// Q29's REPL half. A `Closure` names its proto by a bare index (ADR-034), so
/// before ADR-044 this returned a proto from the wrong chunk or nothing at all.
/// The session has one chunk and appends to it, so the index stays valid.
#[test]
fn a_function_defined_in_one_input_is_callable_from_a_later_one() {
    let mut s = Session::new();
    assert_eq!(
        eval(&mut s, "(def double (fn double [x] (* x 2)))"),
        "#<fn>"
    );
    assert_eq!(
        eval(&mut s, "(def quad (fn quad [x] (double (double x))))"),
        "#<fn>"
    );
    // Three inputs later, and through a second function defined in a third.
    assert_eq!(eval(&mut s, "(quad 5)"), "20");
    assert_eq!(eval(&mut s, "(double 21)"), "42");
    // A closure that captured in an earlier input keeps its captures too.
    assert_eq!(
        eval(&mut s, "(def add-n (fn add-n [n] (fn [x] (+ x n))))"),
        "#<fn>"
    );
    assert_eq!(eval(&mut s, "(def add3 (add-n 3))"), "#<fn>");
    assert_eq!(eval(&mut s, "(add3 4)"), "7");
}

#[test]
fn a_macro_defined_in_one_input_is_in_scope_in_the_next() {
    let mut s = Session::new();
    eval(&mut s, "(defmacro twice [x] `(+ ~x ~x))");
    assert_eq!(eval(&mut s, "(twice 21)"), "42");
    // And a macro can use one defined earlier, which is the case that needs the
    // table to persist rather than merely to be re-seeded with the prelude.
    eval(&mut s, "(defmacro quadruple [x] `(twice (twice ~x)))");
    assert_eq!(eval(&mut s, "(quadruple 3)"), "12");
}

/// ADR-044 part 1's load-bearing clause. A counter that restarted per input
/// could hand input 2 a name input 1 already used, and a fresh symbol that is
/// not fresh is the one thing gensym may not produce.
#[test]
fn the_gensym_counter_never_restarts_within_a_session() {
    let mut s = Session::new();
    let mut seen = Vec::new();
    for _ in 0..5 {
        // One input each, so a per-input reset would show as a repeat.
        seen.push(eval(&mut s, "(gensym \"g\")"));
    }
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), seen.len(), "a name was reissued: {seen:?}");
}

/// ADR-039 gives the VM one failure path that leaves the machine usable, and
/// ADR-044 part 5 spends that here rather than re-earning it.
#[test]
fn a_thrown_value_does_not_end_the_session() {
    let mut s = Session::new();
    eval(&mut s, "(def x 1)");
    assert!(eval(&mut s, "(no-such-global)").starts_with("threw"));
    assert!(eval(&mut s, "(throw :deliberate)").starts_with("threw"));
    // The state from before the failures is intact, and new state still lands.
    assert_eq!(eval(&mut s, "x"), "1");
    assert_eq!(eval(&mut s, "(def y 2) (+ x y)"), "3");
    // Including a failure part-way through a multi-form input: `z` was set
    // before the throw, and it stays set.
    assert!(eval(&mut s, "(def z 9) (throw :after)").starts_with("threw"));
    assert_eq!(eval(&mut s, "z"), "9");
}

/// ADR-028 invariant 3 at the prompt: a cleanup that throws while unwinding
/// wins, and the error it displaced is retained on the winner's suppressed
/// chain. The session has to carry that chain out, and the prompt has to print
/// it — a `.out` transcript has a `--- suppressed` section for the same reason,
/// and dropping it at the REPL loses the original failure entirely.
///
/// Nothing else here constructs a suppressed chain, so removing the printing
/// survived the whole suite (`notes/milestone-9-mutants.md`).
#[test]
fn a_suppressed_error_survives_the_session_and_reaches_the_prompt() {
    let mut s = Session::new();
    match s.eval("(try (throw :original) (finally (throw :from-cleanup)))") {
        Ok(Ended::Threw(u)) => {
            assert_eq!(printer::print(&u.value, &s.vm.interner), ":from-cleanup");
            assert_eq!(
                u.suppressed
                    .iter()
                    .map(|v| printer::print(v, &s.vm.interner))
                    .collect::<Vec<_>>(),
                vec![":original"],
                "the displaced error was dropped"
            );
        }
        _ => panic!("the cleanup's throw should have escaped"),
    }

    let mut child = Command::new(bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start apolisp");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"(try (throw :original) (finally (throw :from-cleanup)))\n")
        .expect("writes");
    let out = child.wait_with_output().expect("runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(":from-cleanup"), "{stdout}");
    assert!(
        stdout.contains("--- suppressed") && stdout.contains(":original"),
        "the prompt dropped the suppressed chain: {stdout}"
    );
}

/// A malformed input is a report, not a state change. `emit`ted output from
/// before a compile failure is the interesting part: there is none, because the
/// input never ran.
#[test]
fn an_input_that_does_not_build_leaves_the_session_alone() {
    let mut s = Session::new();
    eval(&mut s, "(def a 1)");
    assert!(eval(&mut s, ")").starts_with("error"));
    assert!(eval(&mut s, "(let [x] x)").starts_with("error"));
    assert_eq!(eval(&mut s, "a"), "1");

    // Those two fail in the reader and in resolution, both *before* any proto
    // is appended. ADR-047 added a refusal that happens during lowering, after
    // protos are already in the chunk — so this is the same invariant one stage
    // further down, and the first version of it that could actually renumber
    // anything.
    //
    // Checked with a closure rather than a global: a global is a name in a
    // table, but a closure names its proto by *index* into the session's shared
    // chunk (ADR-044), so it is the thing a half-appended compile would break.
    eval(&mut s, "(def double (fn [x] (* x 2)))");
    assert!(eval(&mut s, "(loop [i 0] (+ 1 (recur (+ i 1))))").starts_with("error"));
    assert_eq!(eval(&mut s, "(double 21)"), "42");
    assert_eq!(
        eval(
            &mut s,
            "(loop [i 0] (if (= i 3) (double i) (recur (+ i 1))))"
        ),
        "6"
    );
}

// --- What the prompt has to decide ------------------------------------------------

/// `wants_more` is the whole of the driver's line-buffering policy, and it is a
/// flag on the reader's error rather than a bracket count — which gets strings
/// and comments wrong — or a test on the message text, which ADR-039's argument
/// forbids one layer down.
#[test]
fn incomplete_input_asks_for_more_and_wrong_input_does_not() {
    for open in [
        "(+ 1",
        "(+ 1 (* 2",
        "[1 2",
        "{:a 1",
        "{:a",
        "\"unterminated",
        "`(a ~",
        "(f ; a comment does not close it\n",
    ] {
        assert!(session::wants_more(open), "{open:?} should want more");
    }
    for done in [
        "(+ 1 2)",
        "[1 2]",
        "{:a 1}",
        "\"a string with ( in it\"",
        "; just a comment\n",
        // Wrong, not unfinished. Waiting for more here hangs the prompt on a
        // typo, which is the failure this distinction exists to prevent.
        ")",
        "(+ 1 2))",
        "{:a 1 :b}",
    ] {
        assert!(!session::wants_more(done), "{done:?} should not want more");
    }
}

/// The driver end to end, because buffering across lines is the one thing
/// `Session` cannot see.
#[test]
fn the_prompt_buffers_a_form_across_lines_and_exits_cleanly() {
    let mut child = Command::new(bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start apolisp");
    child
        .stdin
        .take()
        .expect("stdin")
        // The last call uses a value nothing else in the session produces, so
        // "the session survived the syntax error" cannot be satisfied by a
        // digit that was already on screen.
        .write_all(b"(def f (fn f [x]\n  (* x\n     2)))\n(f 21)\n\n(println :out)\n)\n(f 617)\n")
        .expect("writes");
    let out = child.wait_with_output().expect("runs");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "the session should exit 0: {stdout}");
    assert!(
        stdout.contains("42"),
        "the multi-line form did not run: {stdout}"
    );
    assert!(
        stdout.contains(":out"),
        "emitted output is missing: {stdout}"
    );
    // The stray `)` is reported against `<repl>` and the session carries on —
    // `(f 1)` runs after it.
    assert!(
        stdout.contains("<repl>"),
        "the syntax error was not reported: {stdout}"
    );
    assert!(
        stdout.contains("1234"),
        "the session stopped at the syntax error: {stdout}"
    );
}

/// A blank line is not an input. Checked on its own, because in a longer
/// session an extra `nil` hides among the real ones.
#[test]
fn a_blank_line_evaluates_to_nothing_at_all() {
    let mut child = Command::new(bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start apolisp");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"\n   \n\t\n")
        .expect("writes");
    let out = child.wait_with_output().expect("runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("nil"),
        "a blank line produced a value: {stdout:?}"
    );
}
