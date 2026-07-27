//! Milestone 2 verification (BUILD.md): the core AST, the slot compiler, and
//! the disassembler.
//!
//! The `.disasm` goldens pin what the compiler emits. The tests around them pin
//! the things a golden cannot see on its own — that the decisions in ADR-002,
//! ADR-028, ADR-033, ADR-034, and ADR-035 are the ones actually implemented, and
//! that `lines` is a live mechanism rather than a parallel array of the same
//! value repeated.
//!
//! Nothing here regenerates a golden file.

use apolisp::bytecode::{CaptureSrc, Chunk, Instr, Proto, Slot};
use apolisp::error::SpanOrigin;
use apolisp::value::Interner;
use apolisp::{bytecode, compile, error, reader};

mod common;
use common::check_goldens;

fn compile_ok(src: &str) -> (Chunk, Interner) {
    let mut interner = Interner::new();
    let forms = reader::read_all(src, &mut interner)
        .unwrap_or_else(|e| panic!("{src:?}: {}", e.render("<test>", src)));
    let chunk = compile::compile(&forms, &mut interner)
        .unwrap_or_else(|e| panic!("{src:?}: {}", e.render("<test>", src)));
    (chunk, interner)
}

fn compile_err(src: &str) -> String {
    let mut interner = Interner::new();
    let forms = reader::read_all(src, &mut interner).expect("the test source reads");
    match compile::compile(&forms, &mut interner) {
        Ok(_) => panic!("{src:?} should not compile"),
        Err(e) => e.render("<test>", src),
    }
}

fn count(p: &Proto, f: impl Fn(&Instr) -> bool) -> usize {
    p.code.iter().filter(|i| f(i)).count()
}

fn is_tail_call(i: &Instr) -> bool {
    matches!(i, Instr::TailCall { .. })
}

fn is_call(i: &Instr) -> bool {
    matches!(i, Instr::Call { .. })
}

// --- Golden snapshots -------------------------------------------------------

/// Rung 3 (BUILD.md). Milestone 2 owns `.disasm`.
#[test]
fn disasm_snapshots_match() {
    check_goldens("compile", "disasm");
}

// --- ADR-023 point 2: lines is parallel to code -----------------------------

/// Every instruction has an origin, and the array is exactly as long as the
/// code. Kept structural by there being one `emit`, and checked here because
/// "structural by construction" is a claim about code that can be edited.
#[test]
fn every_instruction_has_an_origin() {
    for path in common::corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let (chunk, _) = compile_ok(&src);
        for (i, p) in chunk.protos.iter().enumerate() {
            assert_eq!(
                p.code.len(),
                p.lines.len(),
                "{}: proto {i} has {} instructions and {} origins",
                path.display(),
                p.code.len(),
                p.lines.len()
            );
            for (pc, o) in p.lines.iter().enumerate() {
                // Nothing in the corpus is macro-generated yet, so every
                // instruction traces to real source text. When milestone 5
                // lands, `Generated` becomes legal here and this assertion is
                // the thing that has to be widened deliberately.
                match o {
                    SpanOrigin::Source(s) => assert!(
                        (s.end as usize) <= src.len(),
                        "{}: proto {i} pc {pc} has span {}..{} outside a {}-byte file",
                        path.display(),
                        s.start,
                        s.end,
                        src.len()
                    ),
                    other => panic!(
                        "{}: proto {i} pc {pc} has origin {other:?}, not a source span",
                        path.display()
                    ),
                }
            }
        }
    }
}

/// The `../reg-lisp` lesson, reproduced before anything depends on it: a mutant
/// that never restored the compiler's line counter passed that project's entire
/// suite, because every test program had its subexpressions on the same line as
/// the enclosing form (`PRIOR-ART.md`, Q18).
///
/// So this program puts every subexpression on its own line and asserts the
/// exact line each instruction came from. A compiler that pins every origin to
/// the enclosing form, or to byte 0, or that is off by one, fails here and
/// nowhere else.
#[test]
fn instruction_origins_track_subexpressions_not_the_enclosing_form() {
    let src = "(f\n  1\n  2)\n";
    let (chunk, _) = compile_ok(src);
    let top = &chunk.protos[0];
    let lines: Vec<usize> = top
        .lines
        .iter()
        .map(|o| match o.span() {
            Some(s) => error::line_col(src, s.start as usize).0,
            None => 0,
        })
        .collect();

    // GETGLOBAL f, CONST 1, CONST 2, TAILCALL, RETURN — the callee and the call
    // belong to line 1, and each argument to its own line.
    assert_eq!(
        lines,
        vec![1, 2, 3, 1, 1],
        "origins were {lines:?} for\n{src}\ndisassembly:\n{}",
        bytecode::disassemble(&chunk, &Interner::new(), src)
    );
}

// --- ADR-034: the encoding --------------------------------------------------

#[test]
fn instr_size_is_asserted_not_assumed() {
    let n = bytecode::instr_size();
    eprintln!("Instr: {n} bytes");
    assert!(
        n <= bytecode::INSTR_SIZE_LIMIT,
        "Instr is {n} bytes against a limit of {} (ADR-034)",
        bytecode::INSTR_SIZE_LIMIT
    );
}

/// Every slot an instruction names is inside the frame the proto declares. The
/// VM will index a frame with these, so an operand past `slots` is the bug that
/// presents as reading someone else's value.
#[test]
fn slot_operands_stay_inside_the_frame() {
    for path in common::corpus_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let (chunk, _) = compile_ok(&src);
        for (i, p) in chunk.protos.iter().enumerate() {
            for (pc, ins) in p.code.iter().enumerate() {
                for s in slots_of(ins) {
                    assert!(
                        s < p.slots,
                        "{}: proto {i} pc {pc} names r{s} in a frame of {} slots",
                        path.display(),
                        p.slots
                    );
                }
            }
        }
    }
}

/// Every slot an instruction reads or writes, including the whole argument
/// window a call occupies.
fn slots_of(i: &Instr) -> Vec<Slot> {
    match *i {
        Instr::Const { dst, .. }
        | Instr::GetCapture { dst, .. }
        | Instr::GetSelf { dst }
        | Instr::GetGlobal { dst, .. }
        | Instr::Closure { dst, .. } => vec![dst],
        Instr::Move { dst, src } | Instr::SetCell { cell: dst, src } => vec![dst, src],
        Instr::SetGlobal { src, .. } | Instr::Return { src } | Instr::Throw { src } => vec![src],
        Instr::Call { dst, base, argc } => {
            let mut v = vec![dst, base];
            v.extend(base..=base + argc);
            v
        }
        Instr::TailCall { base, argc } => (base..=base + argc).collect(),
        Instr::JumpUnless { cond, .. } => vec![cond],
        Instr::PushHandler { err, .. } => vec![err],
        Instr::Jump { .. } | Instr::PushFinally { .. } | Instr::PopHandler | Instr::EndFinally => {
            Vec::new()
        }
    }
}

// --- ADR-028 rule 2: tail calls and handler regions --------------------------

/// A call in tail position is a tail call — unless the frame is still needed,
/// which is what an open handler region means. ADR-028 rule 2 states it for
/// `finally`; the reason it gives ("the frame is still needed") covers `catch`
/// identically, because the handler record names this frame.
///
/// The other half matters as much: once the region is closed, tail calls come
/// back. A compiler that suppressed them for the rest of the function would
/// pass a one-sided version of this test and silently cost constant space.
#[test]
fn a_handler_region_suppresses_tail_calls_and_closing_it_restores_them() {
    let (plain, _) = compile_ok("(fn [] (g))");
    assert_eq!(count(&plain.protos[1], is_tail_call), 1, "plain tail call");

    let (fin, _) = compile_ok("(fn [] (try (g) (finally (h))))");
    assert_eq!(
        count(&fin.protos[1], is_tail_call),
        0,
        "a pending `finally` means the frame is still needed"
    );

    let (cat, _) = compile_ok("(fn [] (try (g) (catch e (h))))");
    assert_eq!(
        count(&cat.protos[1], is_call),
        1,
        "the protected body's call is an ordinary call"
    );
    assert_eq!(
        count(&cat.protos[1], is_tail_call),
        1,
        "the handler runs with the record already popped, so its tail call stands"
    );
}

/// The cleanup is emitted twice — the normal path and the path the VM enters
/// while unwinding (ADR-034) — and exactly one `POPHANDLER` matches each
/// `PUSH`, which is ADR-028 invariant 1 read off the code.
#[test]
fn try_emits_one_pop_per_push_and_two_copies_of_finally() {
    let (chunk, _) = compile_ok("(fn [] (try (g) (catch e (h)) (finally (cleanup))))");
    let p = &chunk.protos[1];
    let pushes = count(p, |i| {
        matches!(i, Instr::PushHandler { .. } | Instr::PushFinally { .. })
    });
    let pops = count(p, |i| matches!(i, Instr::PopHandler));
    assert_eq!(pushes, 2, "a catch region inside a finally region");
    assert_eq!(pops, 2, "one POPHANDLER per region on the untroubled path");
    assert_eq!(
        count(p, |i| matches!(i, Instr::EndFinally)),
        1,
        "one unwinding exit"
    );

    // `cleanup` is fetched once per copy of the finally body.
    let fetches = count(p, |i| matches!(i, Instr::GetGlobal { .. }));
    assert_eq!(fetches, 4, "g, h, and cleanup twice; got {fetches}");
}

// --- ADR-002: self-recursion and flat captures -------------------------------

/// Self-recursion resolves through the running closure's identity, never
/// through a capture. If this ever regresses to a capture, the closure holds a
/// reference to itself and ADR-025's "no `Rc` cycle" claim goes with it.
#[test]
fn self_recursion_is_identity_not_capture() {
    let (chunk, _) = compile_ok("(fn f [n] (f n))");
    let p = &chunk.protos[1];
    assert!(
        p.captures.is_empty(),
        "a self-recursive fn captured {:?}",
        p.captures
    );
    assert_eq!(count(p, |i| matches!(i, Instr::GetSelf { .. })), 1);

    // Crossing a function boundary, the outer function's identity *is* captured
    // — there is no other way for the inner closure to reach it.
    let (chunk, _) = compile_ok("(fn f [] (fn [] (f)))");
    assert_eq!(chunk.protos[2].captures, vec![CaptureSrc::SelfFn]);
}

/// A capture chains outward one level at a time, and each level records where
/// it reads the value from in *its own* frame (ADR-002: copied at creation).
#[test]
fn captures_chain_through_every_level_they_cross() {
    let (chunk, _) = compile_ok("(fn [a] (fn [] (fn [] a)))");
    assert!(
        chunk.protos[1].captures.is_empty(),
        "`a` is a parameter here"
    );
    assert_eq!(
        chunk.protos[2].captures,
        vec![CaptureSrc::Local(0)],
        "the middle function captures the parameter's slot"
    );
    assert_eq!(
        chunk.protos[3].captures,
        vec![CaptureSrc::Capture(0)],
        "the inner function captures the middle function's capture"
    );

    // Capturing the same variable twice is one capture, not two.
    let (chunk, _) = compile_ok("(fn [a] (fn [] (list a a)))");
    assert_eq!(chunk.protos[2].captures.len(), 1);
}

// --- ADR-033: arity, order, variadics ----------------------------------------

#[test]
fn a_parameter_list_ends_in_at_most_one_rest_parameter() {
    let (chunk, _) = compile_ok("(fn [a b] a)");
    assert_eq!(
        (chunk.protos[1].params, chunk.protos[1].variadic),
        (2, false)
    );

    let (chunk, _) = compile_ok("(fn [a b & more] more)");
    assert_eq!(
        (chunk.protos[1].params, chunk.protos[1].variadic),
        (3, true)
    );

    let (chunk, _) = compile_ok("(fn [& more] more)");
    assert_eq!(
        (chunk.protos[1].params, chunk.protos[1].variadic),
        (1, true)
    );

    for bad in ["(fn [a &] a)", "(fn [& a b] a)", "(fn [& & a] a)"] {
        let err = compile_err(bad);
        assert!(err.contains("rest parameter"), "{bad}: {err}");
    }
}

/// Operator first, then arguments, left to right (ADR-033 rule 1). Observable
/// because `throw` is a core form and cells are mutable, so this is semantics
/// rather than a compiler liberty.
#[test]
fn arguments_evaluate_left_to_right_after_the_operator() {
    let (chunk, i) = compile_ok("(f (a) (b))");
    let names: Vec<&str> = chunk.protos[0]
        .code
        .iter()
        .filter_map(|ins| match ins {
            Instr::GetGlobal { name, .. } => Some(i.name(name.0)),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["f", "a", "b"]);
}

/// ADR-034: a call's callee sits at `base` and its arguments immediately after,
/// which is what lets the callee's frame receive them as its own first slots.
#[test]
fn a_call_window_is_contiguous_with_the_callee_first() {
    let (chunk, _) = compile_ok("(f 1 2 3)");
    let call = chunk.protos[0]
        .code
        .iter()
        .find(|i| is_call(i) || is_tail_call(i))
        .expect("a call");
    let (base, argc) = match *call {
        Instr::Call { base, argc, .. } | Instr::TailCall { base, argc } => (base, argc),
        _ => unreachable!(),
    };
    assert_eq!(argc, 3);
    let writes: Vec<Slot> = chunk.protos[0]
        .code
        .iter()
        .filter_map(|i| match *i {
            Instr::GetGlobal { dst, .. } | Instr::Const { dst, .. } => Some(dst),
            _ => None,
        })
        .collect();
    assert_eq!(writes, vec![base, base + 1, base + 2, base + 3]);
}

// --- ADR-035: collection literals --------------------------------------------

#[test]
fn a_collection_literal_in_code_position_is_a_call() {
    let (chunk, i) = compile_ok("[1 (f)]");
    let names: Vec<&str> = chunk.protos[0]
        .code
        .iter()
        .filter_map(|ins| match ins {
            Instr::GetGlobal { name, .. } => Some(i.name(name.0)),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["vector", "f"]);

    let (chunk, i) = compile_ok("{:a 1}");
    assert!(chunk.protos[0].code.iter().any(|ins| matches!(
        ins,
        Instr::GetGlobal { name, .. } if i.name(name.0) == "hash-map"
    )));

    // The constructor bypasses lexical scope: `[x]` is a vector literal even
    // where a local named `vector` is in scope (ADR-035).
    let (chunk, i) = compile_ok("(let [vector 1] [2])");
    assert!(chunk.protos[0].code.iter().any(|ins| matches!(
        ins,
        Instr::GetGlobal { name, .. } if i.name(name.0) == "vector"
    )));

    // `()` evaluates to itself, so it is a constant rather than a call.
    let (chunk, _) = compile_ok("()");
    assert_eq!(count(&chunk.protos[0], is_call), 0);
    assert_eq!(count(&chunk.protos[0], is_tail_call), 0);
}

// --- Shape errors -------------------------------------------------------------

/// The core is closed (ADR-007), so a malformed core form is a compile error
/// with a position — not a call to a global that happens to be named `if`.
#[test]
fn malformed_core_forms_are_errors_with_a_position() {
    for (src, want) in [
        ("(if)", "`if` takes a test"),
        ("(if a b c d)", "`if` takes a test"),
        ("(let 1 2)", "binding vector"),
        ("(let [a] a)", "name with no value"),
        ("(let [1 2] 3)", "binds symbols"),
        ("(fn)", "parameter vector"),
        ("(fn 1 2)", "parameter vector"),
        ("(fn [1] 2)", "parameters are symbols"),
        ("(quote a b)", "`quote` takes exactly one form"),
        ("(set-cell! a)", "`set-cell!` takes a cell and a value"),
        ("(set-global! 1 2)", "binds a symbol"),
        ("(set-global! a)", "takes a name and a value"),
        ("(throw)", "`throw` takes exactly one value"),
        ("(catch e 1)", "only valid inside `try`"),
        ("(finally 1)", "only valid inside `try`"),
        ("(try a (catch e 1) (catch f 2))", "at most one `catch`"),
        ("(try a (finally 1) (catch e 2))", "must be the last clause"),
        ("(try a (catch) 1)", "binds the thrown value"),
        ("(try a (finally 1) 2)", "must be one too"),
    ] {
        let err = compile_err(src);
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

/// ADR-023's cost paragraph: the compiler's input is not a closed type, because
/// a macro can put a closure or a handle into code position. Nothing can build
/// one yet, so this pins the path rather than the message.
#[test]
fn a_core_form_cannot_be_shadowed_by_a_local() {
    // `if` in head position is the core form regardless of what is in scope —
    // a language whose `if` depends on the environment has no closed core.
    let (chunk, _) = compile_ok("(let [if 1] (if true 2 3))");
    assert_eq!(
        count(&chunk.protos[0], |i| matches!(i, Instr::JumpUnless { .. })),
        1
    );
}
