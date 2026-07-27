//! Milestone 3 verification (BUILD.md): frames, calls, closures, tail calls.
//!
//! The exit condition is "smoke.sh runs a recursive function; a tail loop runs
//! in constant space". The first half is smoke's; the second half is not
//! observable from outside the VM at all, so it is measured here — a tail loop
//! that quietly grew the frame stack would still print the right answer.
//!
//! Nothing here regenerates a golden file.

use apolisp::vm::{Outcome, Vm};
use apolisp::{compile, printer, reader, vm};

mod common;
use common::check_goldens_over;

/// What a finished program left behind: its printed value and everything it
/// emitted.
#[derive(Debug)]
struct Ran {
    value: String,
    output: String,
}

/// The high-water marks a run reached — frames deep, slots wide.
#[derive(PartialEq, Eq, Debug)]
struct Peak {
    frames: usize,
    slots: usize,
}

/// Run a program and return its printed value plus everything it emitted.
fn run(src: &str) -> Result<Ran, String> {
    Ok(run_traced(src)?.0)
}

/// The same, plus the high-water marks.
fn run_traced(src: &str) -> Result<(Ran, Peak), String> {
    let mut machine = Vm::new();
    let forms = reader::read_all(src, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source reads");
    let chunk = compile::compile(&forms, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source compiles");
    match vm::run_traced(&mut machine, &chunk) {
        Ok((Outcome::Returned(v), (frames, slots))) => Ok((
            Ran {
                value: printer::print(&v, &machine.interner),
                output: machine.take_output(),
            },
            Peak { frames, slots },
        )),
        Ok((Outcome::Threw(v), _)) => {
            Err(format!("threw {}", printer::print(&v, &machine.interner)))
        }
        Err(e) => Err(e.render("<test>", src)),
    }
}

fn value_of(src: &str) -> String {
    run(src).unwrap_or_else(|e| panic!("{src:?}: {e}")).value
}

// --- Golden transcripts -------------------------------------------------------

/// Rung 3 (BUILD.md). Milestone 3 owns `.out`.
///
/// Only the programs that *run* have one. The rest reference globals nothing
/// defines yet, so they fault — and a fault transcript is milestone 4's, which
/// cannot be written before Q23 says what a thrown value is. The list is
/// asserted rather than inferred, so adding a corpus program forces the choice
/// instead of silently skipping it.
#[test]
fn out_transcripts_match() {
    let runnable = ["hello.xs", "recursion.xs"];
    let with_out: Vec<String> = common::corpus_files()
        .iter()
        .filter(|p| p.with_extension("out").exists())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        with_out, runnable,
        "the set of runnable corpus programs changed; decide, do not regenerate"
    );
    let named: Vec<_> = common::corpus_files()
        .into_iter()
        .filter(|p| runnable.contains(&p.file_name().unwrap().to_string_lossy().as_ref()))
        .collect();
    assert_eq!(
        named.len(),
        runnable.len(),
        "a named corpus program is missing"
    );
    check_goldens_over("run", "out", named);
}

// --- ADR-028: tail calls ------------------------------------------------------

/// The exit condition, measured rather than assumed. A hundred thousand tail
/// calls must leave the frame stack exactly as deep as one call, and the slot
/// stack no wider.
///
/// Both halves matter. Frames alone would pass if the VM reused frames but
/// leaked slots on every iteration, which is the failure `../wallisp` reported:
/// TCO fixed its stack and not its heap, and each iteration still allocated a
/// frame that was never reclaimed (`PRIOR-ART.md`).
#[test]
fn a_tail_loop_runs_in_constant_space() {
    let program = |n: i64| {
        format!("(set-global! down (fn down [i] (if (< i {n}) (down (+ i 1)) i)))\n(down 0)")
    };
    let (short, small) = run_traced(&program(10)).unwrap();
    let (long, big) = run_traced(&program(100_000)).unwrap();

    assert_eq!(short.value, "10");
    assert_eq!(long.value, "100000");
    assert_eq!(
        small, big,
        "ten iterations peaked at {small:?} and a hundred thousand at {big:?} — \
         a tail call is reusing the frame but not the slots"
    );
}

/// The other direction, so the test above cannot pass by the VM never growing
/// at all: ordinary recursion *does* nest, and deeper input nests further.
#[test]
fn non_tail_recursion_grows_the_frame_stack() {
    let program = |n: i64| {
        format!("(set-global! sum (fn sum [n] (if (< n 1) 0 (+ n (sum (- n 1))))))\n(sum {n})")
    };
    let (ten, shallow) = run_traced(&program(10)).unwrap();
    let (fifty, deep) = run_traced(&program(50)).unwrap();

    assert_eq!(ten.value, "55");
    assert_eq!(fifty.value, "1275");
    assert!(
        deep.frames > shallow.frames,
        "50 levels peaked at {} frames and 10 at {} — recursion is not nesting",
        deep.frames,
        shallow.frames
    );
}

// --- ADR-002: closures --------------------------------------------------------

#[test]
fn a_closure_copies_its_captures_at_creation() {
    assert_eq!(value_of("(((fn [by] (fn [x] (+ x by))) 40) 2)"), "42");

    // Two closures from the same function capture independently.
    assert_eq!(
        value_of("(set-global! add (fn [by] (fn [x] (+ x by))))\n(+ ((add 1) 10) ((add 2) 10))"),
        "23"
    );

    // Chained through two levels, which is where the capture-of-a-capture
    // descriptor is exercised.
    assert_eq!(value_of("((((fn [a] (fn [] (fn [] a))) 7)))"), "7");
}

#[test]
fn self_recursion_resolves_through_identity() {
    // No global, no capture — the function reaches itself through the running
    // closure (ADR-002).
    assert_eq!(
        value_of("((fn f [n] (if (< n 1) 0 (+ 1 (f (- n 1))))) 5)"),
        "5"
    );
}

// --- ADR-033 / ADR-038: arity and variadics -----------------------------------

#[test]
fn arity_is_checked_in_the_callee_at_call_time() {
    for (src, want) in [
        ("((fn [a] a))", "takes 1 argument(s), given 0"),
        ("((fn [a] a) 1 2)", "takes 1 argument(s), given 2"),
        ("((fn [a & b] a))", "takes at least 1 argument(s), given 0"),
        ("(+ 1 nil)", "needs an integer"),
        ("(1 2)", "cannot call a int"),
    ] {
        let err = run(src).expect_err(&format!("{src:?} should fault"));
        assert!(
            err.contains(want),
            "{src:?}: expected {want:?}, got {err:?}"
        );
    }
}

/// E-11 is emphatic about this one: an empty list is truthy and `nil` is not, so
/// getting it wrong takes the opposite branch with no error anywhere.
#[test]
fn a_rest_parameter_is_an_empty_list_never_nil() {
    assert_eq!(value_of("((fn [a & rest] rest) 1)"), "()");
    assert_eq!(value_of("((fn [a & rest] rest) 1 2 3)"), "(2 3)");
    assert_eq!(value_of("((fn [& rest] rest))"), "()");
    // Truthy, which is the half that bites.
    assert_eq!(value_of("((fn [a & rest] (if rest 1 2)) 9)"), "1");
}

/// ADR-038: the frame is sized `max(slots, argc)`, so a call far wider than the
/// callee's own slot count still has somewhere to put its arguments.
#[test]
fn a_variadic_frame_fits_more_arguments_than_the_callee_has_slots() {
    let args: Vec<String> = (1..=60).map(|i| i.to_string()).collect();
    let src = format!("((fn [& xs] xs) {})", args.join(" "));
    assert_eq!(value_of(&src), format!("({})", args.join(" ")));
}

// --- ADR-037: overflow --------------------------------------------------------

/// Checked arithmetic, so debug and release are the same language. Without it
/// this expression panics in debug and wraps in release.
#[test]
fn integer_overflow_throws_rather_than_wrapping() {
    let err = run("(* 9223372036854775807 2)").expect_err("should fault");
    assert!(err.contains("overflowed"), "got {err:?}");

    let err = run("(+ 9223372036854775807 1)").expect_err("should fault");
    assert!(err.contains("overflowed"), "got {err:?}");

    // The boundary itself is fine.
    assert_eq!(value_of("(- 9223372036854775807 1)"), "9223372036854775806");
}

// --- TRAPS.md: truthiness -----------------------------------------------------

/// Only `nil` and `false` are falsy. Zero, the empty string, and empty
/// collections are all truthy, and every one of them is easy to get wrong in a
/// conditional opcode.
#[test]
fn only_nil_and_false_are_falsy() {
    for (src, want) in [
        ("(if nil 1 2)", "2"),
        ("(if false 1 2)", "2"),
        ("(if true 1 2)", "1"),
        ("(if 0 1 2)", "1"),
        ("(if \"\" 1 2)", "1"),
        ("(if (list) 1 2)", "1"),
        ("(if nil 1)", "nil"),
    ] {
        assert_eq!(value_of(src), want, "{src}");
    }
}

// --- ADR-027: globals and rebinding -------------------------------------------

#[test]
fn a_global_is_a_cell_that_rebinding_writes_through() {
    // A closure that read the global before the rebind sees the new value,
    // because the name's cell is created once and kept (ADR-027).
    assert_eq!(
        value_of("(set-global! x 1)\n(set-global! read-x (fn [] x))\n(set-global! x 2)\n(read-x)"),
        "2"
    );

    // And `set-global!` evaluates to the value it bound.
    assert_eq!(value_of("(set-global! y 5)"), "5");
}

#[test]
fn an_unbound_global_faults_where_it_was_used() {
    let err = run("(no-such-thing)").expect_err("should fault");
    assert!(err.contains("`no-such-thing` is not bound"), "got {err:?}");
    assert!(
        err.contains(":1:"),
        "the fault carries a position — {err:?}"
    );
}

// --- ADR-038: what is deliberately not built yet ------------------------------

/// `try` compiles (milestone 2) and does not run (milestone 4). The VM says so
/// rather than skipping the instruction, because a handler that silently does
/// nothing is the failure mode ADR-028 invariant 1 exists to prevent.
#[test]
fn try_compiles_but_does_not_run_yet() {
    let err = run("(try 1 (finally 2))").expect_err("should fault");
    assert!(err.contains("milestone 4"), "got {err:?}");
}

/// Q26: the numeric tower is undecided, so a float in arithmetic faults rather
/// than being coerced. Coercing would settle the question in a match arm.
#[test]
fn float_arithmetic_is_deferred_rather_than_guessed() {
    let err = run("(+ 1 2.5)").expect_err("should fault");
    assert!(err.contains("Q26"), "got {err:?}");
}

// --- The value a program produces ---------------------------------------------

#[test]
fn a_program_evaluates_to_its_last_form() {
    assert_eq!(value_of("1 2 3"), "3");
    assert_eq!(value_of(""), "nil");
    assert_eq!(value_of("; only a comment\n"), "nil");
}

#[test]
fn println_displays_strings_without_their_quotes() {
    let ran = run("(println \"a b\") (println 1 :k) (println [\"a\" \"b\"])").unwrap();
    assert_eq!(ran.value, "nil");
    // Inside a collection a string stays readable, or `["a b"]` and
    // `["a" "b"]` would print identically.
    assert_eq!(ran.output, "a b\n1 :k\n[\"a\" \"b\"]\n");
}
