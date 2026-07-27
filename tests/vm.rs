//! Milestones 3 and 4 (BUILD.md): frames, calls, closures, tail calls, and
//! then errors, `try`/`throw`/`finally`, and the handler stack.
//!
//! Milestone 3's exit condition is "smoke.sh runs a recursive function; a tail
//! loop runs in constant space". The first half is smoke's; the second half is
//! not observable from outside the VM at all, so it is measured here — a tail
//! loop that quietly grew the frame stack would still print the right answer.
//!
//! Milestone 4's is "failure transcripts in the corpus; cleanup runs exactly
//! once". The transcripts are goldens; "exactly once" is counted here, because
//! a cleanup that runs twice produces the same *value* as one that runs once.
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
///
/// The `Err` side is a program that threw. Since ADR-039 a VM fault *is* a
/// throw, so this is where an arity error arrives too — rendered as the value
/// plus the position that travels beside it, which is what the transcript
/// prints and what these tests assert on.
fn run_traced(src: &str) -> Result<(Ran, Peak), String> {
    let mut machine = Vm::new();
    let forms = reader::read_all(src, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source reads");
    let chunk = compile::compile(&forms, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source compiles");
    match vm::run_traced(&mut machine, &chunk) {
        (Outcome::Returned(v), (frames, slots)) => Ok((
            Ran {
                value: printer::print(&v, &machine.interner),
                output: machine.take_output(),
            },
            Peak { frames, slots },
        )),
        (Outcome::Threw(u), _) => {
            let at = u.position("<test>", src).unwrap_or_else(|| "?".to_string());
            let mut msg = format!(
                "threw {} at {at}",
                printer::print(&u.value, &machine.interner)
            );
            for v in &u.suppressed {
                msg.push_str(&format!(
                    ", suppressing {}",
                    printer::print(v, &machine.interner)
                ));
            }
            Err(msg)
        }
    }
}

/// What a program printed, when it is the *order* of effects being pinned
/// rather than the value. A program that ends in a throw still has output.
fn output_of(src: &str) -> String {
    let mut machine = Vm::new();
    let forms = reader::read_all(src, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source reads");
    let chunk = compile::compile(&forms, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source compiles");
    vm::run(&mut machine, &chunk);
    machine.take_output()
}

fn value_of(src: &str) -> String {
    run(src).unwrap_or_else(|e| panic!("{src:?}: {e}")).value
}

// --- Golden transcripts -------------------------------------------------------

/// Rung 3 (BUILD.md). Milestone 3 owns `.out`; milestone 4 puts failures in it.
///
/// Only the programs that *run to a defined end* have one, and since ADR-039
/// that includes the ones that fail: `control.xs` faults on the first global
/// nothing defines, and `errors.xs` ends on an uncaught throw. The rest still
/// have none, and the list is asserted rather than inferred so adding a corpus
/// program forces the choice instead of silently skipping it.
///
/// `macros.xs` earns one for a reason the `.expanded` golden cannot cover: a
/// macro can expand to something that reads correctly and computes the wrong
/// answer, and only running it says which.
#[test]
fn out_transcripts_match() {
    let runnable = [
        "control.xs",
        "errors.xs",
        "hello.xs",
        "macros.xs",
        "recursion.xs",
    ];
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

// --- ADR-028 / ADR-039: the handler stack -------------------------------------

/// Milestone 4's exit condition, and the one a value cannot show: a cleanup
/// that runs twice produces the same answer as one that runs once. Every path
/// prints, so the count is in the output.
///
/// The four paths are the ones milestone 2's reviewer traced on the emitted
/// code: normal completion, a throw from the body, a throw from the handler,
/// and a throw from the cleanup itself.
#[test]
fn cleanup_runs_exactly_once_on_every_path() {
    for (src, want) in [
        // Nothing thrown.
        ("(try 1 (finally (println :c)))", ":c\n"),
        // Thrown from the body, caught.
        (
            "(try (throw :x) (catch e e) (finally (println :c)))",
            ":c\n",
        ),
        // Thrown from the handler: the catch region nests inside the finally
        // region, so the cleanup still runs (ADR-034).
        (
            "(try (try (throw :x) (catch e (throw :y)) (finally (println :c))) (catch e e))",
            ":c\n",
        ),
        // Thrown from the cleanup itself.
        (
            "(try (try (throw :x) (finally (println :c) (throw :y))) (catch e e))",
            ":c\n",
        ),
        // Nothing catches it at all: the cleanup runs on the way out.
        ("(try (throw :x) (finally (println :c)))", ":c\n"),
    ] {
        assert_eq!(output_of(src), want, "{src}");
    }
}

/// A cleanup on a path that never throws must not run the *unwinding* copy as
/// well — the two copies are the same source, so a double run is invisible in
/// the value and obvious in the output.
#[test]
fn the_two_copies_of_a_cleanup_are_never_both_taken() {
    assert_eq!(
        output_of("(try (println :body) (finally (println :c))) (println :after)"),
        ":body\n:c\n:after\n"
    );
}

#[test]
fn a_handler_binds_the_thrown_value() {
    assert_eq!(value_of("(try (throw 42) (catch e e))"), "42");
    assert_eq!(value_of("(try 1 (catch e e))"), "1");
    // ADR-039 clause 1: no shape is imposed, so a collection throws as itself.
    assert_eq!(
        value_of("(try (throw {:kind :boom}) (catch e e))"),
        "{:kind :boom}"
    );
}

/// ADR-039 clause 2, the decision this milestone turns on: a VM-raised fault
/// unwinds like a `throw`, so a handler catches one and a cleanup runs for it.
#[test]
fn a_vm_fault_is_a_throw() {
    assert_eq!(
        value_of("(try (no-such-global) (catch e e))"),
        "{:type :vm-error :kind :unbound :message \"`no-such-global` is not bound\"}"
    );
    // The kind is the contract; the message is prose (ADR-039 clause 3).
    for (src, kind) in [
        ("(try ((fn [a] a)) (catch e e))", ":kind :arity"),
        ("(try (1 2) (catch e e))", ":kind :not-callable"),
        ("(try (+ 1 nil) (catch e e))", ":kind :type"),
        (
            "(try (* 9223372036854775807 2) (catch e e))",
            ":kind :overflow",
        ),
        ("(try (+ 1 2.5) (catch e e))", ":kind :undecided"),
    ] {
        let got = value_of(src);
        assert!(got.contains(kind), "{src}: expected {kind:?} in {got:?}");
    }
    // And the cleanup runs for a fault exactly as it does for a throw — the
    // case `with-open` will depend on at milestone 7.
    assert_eq!(output_of("(try (nope) (finally (println :c)))"), ":c\n");
}

/// ADR-028 invariant 3. The cleanup's error wins; the original is retained on
/// it as suppressed, which is observable because both reach the transcript.
#[test]
fn a_cleanup_error_wins_and_keeps_the_original_as_suppressed() {
    let err =
        run("(try (throw :original) (finally (throw :from-cleanup)))").expect_err("should throw");
    assert!(err.starts_with("threw :from-cleanup"), "got {err:?}");
    assert!(err.contains("suppressing :original"), "got {err:?}");

    // A throw the cleanup catches *itself* displaces nothing: the parked error
    // resumes when the cleanup ends.
    let err = run("(try (throw :original) (finally (try (throw :inner) (catch e e))))")
        .expect_err("should throw");
    assert!(err.starts_with("threw :original"), "got {err:?}");
    assert!(!err.contains("suppressing"), "got {err:?}");
}

/// Unwinding is what drops the frames between the throw and the handler. The
/// run continues afterwards, which is the part that would break if the slot
/// stack were left where the throw abandoned it.
#[test]
fn unwinding_crosses_frames_and_leaves_the_machine_usable() {
    let deep = "(set-global! deep (fn deep [n] (if (< n 1) (throw :bottom) (+ 0 (deep (- n 1))))))";
    assert_eq!(
        value_of(&format!("{deep}\n(try (deep 5) (catch e e))")),
        ":bottom"
    );
    // Caught, then used: the slots the abandoned frames occupied are gone, so
    // the arithmetic below runs in a frame that is the right width.
    assert_eq!(
        value_of(&format!("{deep}\n(+ 1 (try (deep 5) (catch e 41)))")),
        "42"
    );
}

/// A loop that catches a throw every iteration stays flat.
///
/// The loop is a tail loop: the `try` region closes before the recursive call,
/// so ADR-028 rule 2 does not apply and the frame is reused. Unwinding has to
/// leave the machine exactly as deep as it found it, or the marks diverge.
///
/// **What this does not catch**, and the reason the doc comment says so: an
/// unwind that drops the abandoned frames but keeps their *slots*. That leak is
/// bounded — the next call reuses the same slot range and never grows past it —
/// so no high-water mark and no value can see it. The mutation pass found it,
/// no test could be written to attribute it, and the answer was structural:
/// `drop_frame` is the one place a frame is released
/// (`notes/milestone-4-mutants.md`).
#[test]
fn a_loop_that_catches_a_throw_every_iteration_stays_flat() {
    let program = |n: i64| {
        format!(
            "(set-global! boom (fn boom [n] (if (< n 1) (throw :x) (+ 0 (boom (- n 1))))))\n\
             (set-global! spin (fn spin [i] (if (< i {n}) (do (try (boom 3) (catch e e)) \
             (spin (+ i 1))) i)))\n(spin 0)"
        )
    };
    let (short, small) = run_traced(&program(10)).unwrap();
    let (long, big) = run_traced(&program(200)).unwrap();

    assert_eq!(short.value, "10");
    assert_eq!(long.value, "200");
    assert_eq!(
        small, big,
        "ten caught throws peaked at {small:?} and two hundred at {big:?} — \
         a caught throw is leaving something on the frame stack"
    );
}

/// ADR-028 rule 2 at run time. The compiler refuses to emit a tail call inside
/// a handler region; the observable consequence is that the frame stack grows,
/// which is the cost the rule accepts to keep the handler record meaningful.
#[test]
fn a_call_in_tail_position_inside_a_try_is_not_a_tail_call() {
    let program = |n: i64| {
        format!(
            "(set-global! down (fn down [i] (try (if (< i {n}) (down (+ i 1)) i) (finally nil))))\n(down 0)"
        )
    };
    let (short, small) = run_traced(&program(10)).unwrap();
    let (long, big) = run_traced(&program(50)).unwrap();

    assert_eq!(short.value, "10");
    assert_eq!(long.value, "50");
    assert!(
        big.frames > small.frames,
        "a loop inside a `try` stayed flat at {} frames — the handler region \
         did not stop the tail call",
        big.frames
    );
}

/// The handler stack is not the Rust stack (ADR-004/ADR-028): a handler pushed
/// in one frame survives the calls made under it, and nesting is unbounded by
/// the host.
#[test]
fn handlers_nest_and_survive_calls() {
    assert_eq!(
        value_of("(try (try (throw :inner) (catch e (throw :outer))) (catch e e))"),
        ":outer"
    );
    assert_eq!(
        value_of(
            "(set-global! boom (fn [] (throw :b)))\n\
             (try (try (boom) (finally nil)) (catch e e))"
        ),
        ":b"
    );
}

// --- Q26: what is deliberately not decided yet --------------------------------

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
