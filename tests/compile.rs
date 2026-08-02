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
use apolisp::value::{Interner, Value};
use apolisp::{bytecode, compile, error, expand, reader, vm};

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

/// Read, **expand**, then compile — what `apolisp compile` does, and what the
/// corpus-wide properties below have to use.
///
/// `compile_ok` skips expansion, which is right for the hand-written snippets
/// in this file: they are core forms, and reaching the compiler through the
/// expander would test the expander instead. It was wrong for the corpus, and
/// silently so. Every corpus program's macros happened to expand to something
/// the compiler also accepted *unexpanded*, so two properties claiming to hold
/// "over the whole corpus" were holding over a program the VM never runs.
/// `editor.xs` is the first entry with a macro that syntax-quotes a parameter
/// vector, and unexpanded that reads as `(fn ~name ~params ...)` — a list where
/// the compiler wants a vector. The properties did not weaken; they had never
/// been applied to post-expansion bytecode at all.
///
/// The prelude is deliberately left out. `compile_unit` would fold it in, and
/// then the span assertion below would be checking prelude origins against the
/// program's source length.
fn compile_expanded(src: &str) -> Chunk {
    let mut vm = vm::Vm::new();
    let forms = reader::read_all(src, &mut vm.interner)
        .unwrap_or_else(|e| panic!("{}", e.render("<test>", src)));
    let forms = expand::expand_all(forms, &mut vm)
        .unwrap_or_else(|e| panic!("{}", e.render("<test>", src)));
    compile::compile(&forms, &mut vm.interner)
        .unwrap_or_else(|e| panic!("{}", e.render("<test>", src)))
}

/// The error from reading *or* compiling. The depth bound (ADR-036) is a reader
/// error reached through `apolisp compile`, so a helper that insisted on reading
/// first could not see it.
fn compile_err(src: &str) -> String {
    let mut interner = Interner::new();
    let forms = match reader::read_all(src, &mut interner) {
        Ok(forms) => forms,
        Err(e) => return e.render("<test>", src),
    };
    match compile::compile(&forms, &mut interner) {
        Ok(_) => panic!("{src:?} should not compile"),
        Err(e) => e.render("<test>", src),
    }
}

/// The slot a constant's value lands in, for asking which binding an expression
/// actually read.
fn slot_holding(p: &Proto, want: &Value) -> Slot {
    let k = p
        .consts
        .iter()
        .position(|c| c == want)
        .unwrap_or_else(|| panic!("no constant equal to {want:?} in {:?}", p.consts))
        as u32;
    p.code
        .iter()
        .find_map(|i| match *i {
            Instr::Const { dst, k: got } if got == k => Some(dst),
            _ => None,
        })
        .expect("the constant is loaded somewhere")
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
        // A file with no forms has no source text to point at, and `Unknown`
        // says so rather than lying (ADR-026). That case is pinned separately;
        // asserting `Source` over it here would fail on a legal program.
        let mut interner = Interner::new();
        if reader::read_all(&src, &mut interner).unwrap().is_empty() {
            continue;
        }
        let chunk = compile_expanded(&src);
        let mut from_source = 0usize;
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
                // Widened when the corpus gained a program whose macros expand
                // to real code (`editor.xs`), which is the deliberate widening
                // the previous comment here asked for. `Generated` is legal
                // post-expansion; `Unknown` still is not, because the point of
                // the property is that every instruction points *somewhere*. A
                // `Generated` span is the macro call site, so it is inside this
                // file too and the bound applies to both.
                match o {
                    SpanOrigin::Source(s) | SpanOrigin::Generated(s) => {
                        if matches!(o, SpanOrigin::Source(_)) {
                            from_source += 1;
                        }
                        assert!(
                            (s.end as usize) <= src.len(),
                            "{}: proto {i} pc {pc} has span {}..{} outside a {}-byte file",
                            path.display(),
                            s.start,
                            s.end,
                            src.len()
                        );
                    }
                    other => panic!(
                        "{}: proto {i} pc {pc} has origin {other:?}, which points nowhere",
                        path.display()
                    ),
                }
            }
        }
        // Without this, a mutant that stamped every instruction `Generated`
        // would pass the loop above — legalizing `Generated` costs the property
        // its teeth unless something still insists source code reaches bytecode.
        assert!(
            from_source > 0,
            "{}: no instruction traces to source text",
            path.display()
        );
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
    let (chunk, interner) = compile_ok(src);
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
    // belong to line 1, each argument to its own line, and the return to the
    // expression whose value it returns.
    //
    // The interner is the one that read this program: `Interner::new()` here
    // would panic inside `disassemble` while rendering the failure, replacing
    // the report with an index-out-of-bounds.
    assert_eq!(
        lines,
        vec![1, 2, 3, 1, 1],
        "origins were {lines:?} for\n{src}\ndisassembly:\n{}",
        bytecode::disassemble(&chunk, &interner, src)
    );
}

/// ADR-036. Deep nesting is a diagnostic with a position, not a killed process.
/// Before the bound existed, `compile` aborted with SIGABRT at around 1,400
/// levels on input the reader accepted at 3,000 — and this test runs on a cargo
/// test thread, whose stack is a quarter of the main thread's, so passing here
/// is the measurement that matters.
#[test]
fn nesting_past_the_bound_is_a_diagnostic_not_a_crash() {
    let nest = |n: usize| format!("{}1{}", "(f ".repeat(n), ")".repeat(n));

    // Just inside the bound still compiles, all the way through lowering.
    let (chunk, _) = compile_ok(&nest(reader::MAX_NESTING - 2));
    assert!(!chunk.protos[0].code.is_empty());

    let err = compile_err(&nest(reader::MAX_NESTING + 8));
    assert!(err.contains("nested more than"), "got {err:?}");
    assert!(
        err.contains(":1:"),
        "the bound reports a position — {err:?}"
    );
}

/// A file with no forms is legal and has nowhere to point.
#[test]
fn a_file_with_no_forms_compiles_to_nil_with_no_position() {
    let (chunk, _) = compile_ok("; nothing but a comment\n");
    let p = &chunk.protos[0];
    assert_eq!(p.code.len(), 2, "CONST nil, RETURN");
    assert!(
        p.lines.iter().all(|o| matches!(o, SpanOrigin::Unknown)),
        "there is no source text here, and `Unknown` says so instead of \
         inventing a position (ADR-026): {:?}",
        p.lines
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
        let chunk = compile_expanded(&src);
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
        Instr::Move { dst, src }
        | Instr::MoveKill { dst, src }
        | Instr::SetCell { cell: dst, src } => vec![dst, src],
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

/// Which positions *are* tail positions, and which are not.
///
/// The suppression test above only ever exercised a call in plain operator
/// position, so setting any individual tail site to `false` — `if`'s branches,
/// `let`'s body, `do`'s body, a clause-less `try`'s body — left the whole suite
/// green. So did the opposite slip: passing `tail` through to the *operator*,
/// which turns `((f) 1)` into "return `(f)`'s value and never call it".
#[test]
fn tail_position_is_the_last_expression_and_never_the_operator() {
    for src in [
        "(fn [] (g))",
        "(fn [] (do (a) (g)))",
        "(fn [] (let [x 1] (g)))",
        // No clauses means no handler region, so the body keeps tail position.
        "(fn [] (try (g)))",
    ] {
        let (chunk, _) = compile_ok(src);
        assert_eq!(
            count(&chunk.protos[1], is_tail_call),
            1,
            "{src}: the last expression is in tail position"
        );
    }

    // Both branches of a tail `if` are tail positions, not just the first.
    let (chunk, _) = compile_ok("(fn [] (if a (g) (h)))");
    assert_eq!(count(&chunk.protos[1], is_tail_call), 2);

    // The operator is evaluated, then called. It is never in tail position,
    // however tail the call around it is.
    let (chunk, _) = compile_ok("(fn [] ((f) 1))");
    let p = &chunk.protos[1];
    assert_eq!(
        count(p, is_call),
        1,
        "the computed callee is an ordinary call"
    );
    assert_eq!(count(p, is_tail_call), 1, "the outer call is the tail one");
    let call_at = p.code.iter().position(is_call).unwrap();
    let tail_at = p.code.iter().position(is_tail_call).unwrap();
    assert!(
        call_at < tail_at,
        "the callee is computed before it is called"
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

// --- ADR-033: `let` is sequential, and shadowing ------------------------------

/// `let` binds left to right and a name is not in scope while its own
/// initializer is compiled (ADR-033). Hoisting the declaration by two lines
/// turns `let` into `letrec` and no test noticed — so this one reads the
/// program where the two differ: under the sequential rule the initializer's
/// `x` is the outer one, which here means a global.
#[test]
fn let_bindings_are_sequential_not_recursive() {
    let (chunk, i) = compile_ok("(let [x x] x)");
    let p = &chunk.protos[0];
    assert!(
        p.code.iter().any(|ins| matches!(
            ins, Instr::GetGlobal { name, .. } if i.name(name.0) == "x"
        )),
        "the initializer's `x` is the outer binding, not the one being bound"
    );

    // And a later binding does see an earlier one.
    let (chunk, _) = compile_ok("(let [a 1 b a] b)");
    let p = &chunk.protos[0];
    assert_eq!(
        count(p, |ins| matches!(ins, Instr::Move { .. })),
        2,
        "`b` copies `a`, and the body copies `b`"
    );
}

/// An inner binding wins over an outer one, and a later binding in the same
/// vector wins over an earlier one. Both directions of the scope walk were
/// reversible without any test failing.
#[test]
fn the_innermost_and_latest_binding_wins() {
    for src in ["(let [x 1] (let [x 2] x))", "(let [x 1 x 2] x)"] {
        let (chunk, _) = compile_ok(src);
        let p = &chunk.protos[0];
        let shadowing = slot_holding(p, &Value::Int(2));
        let read = p
            .code
            .iter()
            .rev()
            .find_map(|ins| match *ins {
                Instr::Move { src, .. } => Some(src),
                _ => None,
            })
            .expect("the body copies the binding it read");
        assert_eq!(read, shadowing, "{src}: read the wrong `x`");
    }
}

/// A parameter shadows the function's own name. Checking the self-name first
/// would make `(fn f [f] f)` return the function instead of its argument, and
/// nothing failed when that order was swapped.
#[test]
fn a_parameter_shadows_the_functions_own_name() {
    let (chunk, _) = compile_ok("(fn f [f] f)");
    let p = &chunk.protos[1];
    assert_eq!(
        count(p, |i| matches!(i, Instr::GetSelf { .. })),
        0,
        "the parameter wins; `f` here is the argument"
    );

    // Without the parameter in the way, the same name is the function itself.
    let (chunk, _) = compile_ok("(fn f [n] f)");
    assert_eq!(
        count(&chunk.protos[1], |i| matches!(i, Instr::GetSelf { .. })),
        1
    );
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

    // `(fn [& &] a)` is the only one of these that reaches the clause forbidding
    // the rest parameter from being named `&`; the others fail on the length
    // check first, which left that clause unobserved.
    for bad in [
        "(fn [a &] a)",
        "(fn [& a b] a)",
        "(fn [& & a] a)",
        "(fn [& &] a)",
    ] {
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

// --- `loop`/`recur` (ADR-047) ----------------------------------------------

/// The refusals are the reason `loop`/`recur` is a core form rather than the
/// eight-line prelude macro that has the same semantics
/// (`docs/notes/loop-recur-attempt.md`). They are compile errors, so the
/// in-language suite cannot hold them — a file that does not compile has no
/// assertions in it.
#[test]
fn recur_is_refused_everywhere_it_would_not_be_a_tail_call() {
    // Not in a loop at all.
    let e = compile_err("(recur 1)");
    assert!(e.contains("only valid inside `loop`"), "got {e:?}");

    // Across a `fn`. The inner function's frame is not the loop's frame, so
    // this is the same refusal and not a special case.
    let e = compile_err("(loop [i 0] ((fn [] (recur 1))))");
    assert!(e.contains("only valid inside `loop`"), "got {e:?}");

    // Arity is the loop's, checked against the loop rather than at the call.
    let e = compile_err("(loop [i 0] (recur 1 2))");
    assert!(e.contains("rebinds the 1 name(s)"), "got {e:?}");

    // Not in tail position. This is the diagnostic a macro cannot produce,
    // because a macro does not know its own position.
    let e = compile_err("(loop [i 0] (+ 1 (recur (+ i 1))))");
    assert!(e.contains("must be in tail position"), "got {e:?}");

    // Across a `try`, which ADR-028 rule 2 already forbids for tail calls.
    let e = compile_err("(loop [i 0] (try (recur (+ i 1)) (finally nil)))");
    assert!(e.contains("cannot cross a `try`"), "got {e:?}");

    // And in a `finally`, which is not tail position to begin with.
    let e = compile_err("(loop [i 0] (try 1 (finally (recur 2))))");
    assert!(e.contains("must be in tail position"), "got {e:?}");
}

/// A `recur` in tail position is a `TailCall` and nothing else — the frame is
/// reused, which is what makes a loop a loop rather than a stack leak.
#[test]
fn recur_lowers_to_a_tail_call_even_where_the_loop_is_not_in_tail_position() {
    // The loop's *value* is an argument to `+`, so the loop is not in the
    // function's tail position. The `recur` inside it still must be a tail
    // call: it re-enters the loop rather than returning to `+`.
    let (chunk, _) = compile_ok("(fn [] (+ 1 (loop [i 0] (if (= i 5) i (recur (+ i 1))))))");
    let loop_proto = chunk
        .protos
        .iter()
        .find(|p| p.code.iter().filter(|i| is_tail_call(i)).count() == 1)
        .expect("the loop's own proto emits exactly one tail call");
    assert_eq!(
        count(loop_proto, is_tail_call),
        1,
        "the recur is the tail call"
    );
}

/// A refused compile must leave the chunk exactly as it was. In a REPL session
/// the chunk is shared across inputs (ADR-044), so a partially appended proto
/// would renumber every closure compiled after it.
#[test]
fn a_refused_recur_leaves_the_chunk_untouched() {
    let mut interner = Interner::new();
    let mut chunk = Chunk {
        protos: Vec::new(),
        prelude: None,
    };

    let good = reader::read_all("(fn [] 1)", &mut interner).unwrap();
    compile::compile_into(&mut chunk, &good, &mut interner).unwrap();
    let before = chunk.protos.len();

    let bad = reader::read_all("(loop [i 0] (+ 1 (recur (+ i 1))))", &mut interner).unwrap();
    compile::compile_into(&mut chunk, &bad, &mut interner)
        .expect_err("a non-tail recur should not compile");

    assert_eq!(
        chunk.protos.len(),
        before,
        "a refused compile appended protos to the chunk"
    );
}

// --- The prelude's own golden (ADR-048) -------------------------------------

/// The prelude's functions are compiled into every unit and left out of every
/// unit's disassembly, so this is the only thing pinning them. Without it they
/// would be the one piece of code in the language with no golden at all.
///
/// Compiled standalone, so proto 0 is the prelude's top level. That is *not*
/// the numbering it has inside a unit's chunk, where it is appended after the
/// unit — and appending it there is exactly what keeps every other `.disasm`
/// golden still while the prelude grows.
#[test]
fn the_prelude_disassembly_matches_its_golden() {
    let out = std::process::Command::new(common::bin())
        .current_dir(common::repo_root())
        .arg("prelude")
        .output()
        .expect("failed to run apolisp");
    assert!(out.status.success(), "`apolisp prelude` failed");
    let got = String::from_utf8_lossy(&out.stdout);

    let mut path = common::repo_root();
    path.push("tests/prelude.disasm");
    let want = std::fs::read_to_string(&path).expect("tests/prelude.disasm is missing");

    assert_eq!(
        got, want,
        "the prelude's disassembly changed. Read the diff and say why \
         before running `just bless` (BUILD.md: the oracle is review-gated)."
    );
}

/// ADR-048's promise, stated as a test rather than as a comment: a unit's
/// protos come first and the prelude's after, so a program's proto indices
/// depend on the program alone.
///
/// If this inverts, every `.disasm` golden in the corpus moves the next time
/// the prelude gains a function — which is the cost Q29 spent four milestones
/// declining to pay.
#[test]
fn the_prelude_is_appended_after_the_unit_and_left_out_of_the_disassembly() {
    let mut interner = Interner::new();
    let unit = reader::read_all("(fn [] 1)", &mut interner).unwrap();
    let prelude = reader::read_all("(set-global! k (fn [] 2))", &mut interner).unwrap();

    let alone = compile::compile(&unit, &mut interner).expect("compiles");
    let together = compile::compile_unit(&unit, &prelude, &mut interner).expect("compiles");

    let span = together.prelude.expect("compile_unit records the span");
    assert_eq!(
        span.top as usize,
        alone.protos.len(),
        "the prelude must start where the unit ends"
    );
    assert!(span.len > 0, "the prelude contributed no protos");

    // The unit's protos are byte-identical to compiling it with no prelude at
    // all, which is the property the goldens rest on.
    for (i, p) in alone.protos.iter().enumerate() {
        assert_eq!(
            format!("{:?}", together.protos[i]),
            format!("{p:?}"),
            "proto {i} changed when a prelude was added"
        );
    }

    // And the disassembly shows the unit only.
    let text = bytecode::disassemble(&together, &interner, "");
    assert!(!text.contains(&format!("proto {}", span.top)), "{text}");
}
