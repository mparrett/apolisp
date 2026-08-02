//! Milestone 8 (BUILD.md): fuel suspension, `Image`, and resume.
//!
//! **This file is the oracle for constraint #2.** Without it, "a running
//! program is a value you can pause, move, and inspect" is an aspiration rather
//! than a property (ADR-029). The shape BUILD.md asks for: run to fuel
//! exhaustion at an instruction boundary, take an `Image`, resume it in a
//! *fresh* VM of the same build, and compare the full transcript against the
//! uninterrupted run.
//!
//! It runs against the buffered in-memory host, so emitted effects are part of
//! the comparison rather than escaping it. `println` writes into `Vm::out`,
//! which is in the `Image` — a snapshot that dropped it would resume and
//! reprint, or resume and print nothing, and either shows up here as a
//! transcript diff.
//!
//! The strong form is what earns its place: cutting at *every* instruction
//! boundary rather than at one arbitrary point. A single cut proves the
//! mechanism works somewhere; cutting everywhere is what finds the field that
//! is only live between two particular instructions.

use apolisp::image::{self, SnapshotError};
use apolisp::vm::{Outcome, Vm};
use apolisp::{bytecode::Chunk, compile, expand, printer, reader, vm};

/// A finished run, as the transcript the driver would print.
#[derive(PartialEq, Eq, Debug)]
struct Transcript {
    output: String,
    ended: String,
}

/// Read, **expand**, compile. The expander is not optional here the way it is
/// in `tests/vm.rs`: `def`, `try`, and `with-open` are prelude macros, so a
/// pipeline that skipped expansion would be snapshotting a different program
/// from the one the driver runs.
fn build(src: &str) -> (Vm, Chunk) {
    let mut machine = Vm::new();
    // ADR-058, for every program rather than for the one that reads them. The
    // arguments are a global and globals are cells, so they are in every
    // `Image` this file takes whether the program looks at them or not —
    // setting them here is what makes that true of the harness too, and the
    // corpus entry below is what makes it observable.
    apolisp::host::set_args(&mut machine, &ARGS);
    let forms = reader::read_all(src, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source reads");
    let forms = expand::expand_all(forms, &mut machine)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source expands");
    let chunk = compile::compile(&forms, &mut machine.interner)
        .map_err(|e| e.render("<test>", src))
        .expect("the test source compiles");
    (machine, chunk)
}

/// Everything a `.out` transcript records, off a finished `Outcome`.
fn finish(vm: &mut Vm, outcome: Outcome) -> Transcript {
    let ended = match outcome {
        Outcome::Returned(v) => format!("value {}", printer::print(&v, &vm.interner)),
        Outcome::Threw(u) => format!("threw {}", printer::print(&u.value, &vm.interner)),
        Outcome::Suspended => unreachable!("asked to finish a run that suspended"),
    };
    Transcript {
        output: vm.take_output(),
        ended,
    }
}

fn uninterrupted(src: &str) -> Transcript {
    let (mut vm, chunk) = build(src);
    let (outcome, _, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), u64::MAX);
    finish(&mut vm, outcome)
}

/// How many instructions the program takes when nothing interrupts it. The
/// upper bound for "cut at every boundary".
fn steps(src: &str) -> u64 {
    let (mut vm, chunk) = build(src);
    let mut ex = vm::start(&chunk);
    let mut n = 0;
    loop {
        let (outcome, next, _) = vm::run_fueled(&mut vm, &chunk, ex, 1);
        ex = next;
        match outcome {
            Outcome::Suspended => n += 1,
            _ => return n,
        }
        assert!(n < 100_000, "runaway program in the harness");
    }
}

/// Run `src`, cut after exactly `cut` instructions, round-trip through an
/// `Image` into a fresh VM, and finish there.
fn cut_and_resume(src: &str, cut: u64) -> Transcript {
    let (mut vm, chunk) = build(src);
    let (outcome, ex, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), cut);
    assert!(
        matches!(outcome, Outcome::Suspended),
        "cutting at {cut} did not suspend"
    );

    let img = image::capture(&vm, &ex, &chunk).expect("no handles are open");
    // Everything below runs against the restored pair. The originals are
    // dropped here on purpose: a resume that reached back into them would pass
    // this test while being useless, and there is no way to reach a value that
    // has been moved.
    drop((vm, ex));

    let (mut fresh, ex) = image::restore(&img, &chunk).expect("the chunk matches");
    let (outcome, _, _) = vm::run_fueled(&mut fresh, &chunk, ex, u64::MAX);
    finish(&mut fresh, outcome)
}

/// What `build` hands every program as its arguments (ADR-058). Two of them,
/// and distinguishable from each other, so a round trip that kept the vector
/// and lost its order fails as loudly as one that dropped it.
const ARGS: [&str; 2] = ["alpha", "beta"];

/// The programs the property runs over. Each is here for a distinct piece of
/// state that has to survive the round trip, named so a failure says which.
const PROGRAMS: &[(&str, &str)] = &[
    ("globals and cells", "(def a 1) (def b (+ a 2)) (println a b) b"),
    // ADR-058. The failure this catches is quiet rather than loud: the fresh VM
    // `restore` builds has already bound `*command-line-args*` to `[]`, because
    // that is what `host::install` does — so an encoder that dropped the global
    // would not fault on resume, it would answer the empty vector and print a
    // shorter line. BUILD.md's limit on this property is the reason the entry
    // exists at all: adding state means adding a program that creates it, and
    // the property will not ask.
    (
        "a program's arguments",
        "(println (nth *command-line-args* 1)) (str *command-line-args*)",
    ),
    (
        "closures and captures",
        "(def mk (fn mk [n] (fn [] n))) (def f (mk 7)) (println (f)) (f)",
    ),
    (
        "a frame stack mid-call",
        "(def deep (fn deep [n] (if (< n 1) 0 (+ 1 (deep (- n 1)))))) (println (deep 6)) (deep 4)",
    ),
    (
        "handlers and a caught throw",
        "(println (try (throw :boom) (catch e e) (finally (println :cleanup)))) :done",
    ),
    (
        "a parked unwind mid-cleanup",
        "(try (try (throw :original) (finally (println :running) (throw :from-cleanup))) (catch e e))",
    ),
    // The same shape with nothing catching, which is what makes the parked
    // record *observable*. Above, the catch binds the value alone and drops the
    // suppressed chain (ADR-039 clause 4), so an encoder that discarded
    // `pending` entirely round-tripped it perfectly — the parked error never
    // reached the transcript either way. Found by `just mutate` (ADR-055), and
    // it is milestone 4's hole in a second place: two nested `finally`s and no
    // `catch` is the smallest program where a parked unwind can be seen at all.
    (
        "a parked unwind that reaches the transcript",
        "(try (try (throw :original) (finally (throw :from-cleanup))) (finally (println :outer)))",
    ),
    (
        "collections, strings, and shared structure",
        "(def a [1 2 3]) (def b [a a]) (def c {:k b :j b}) (println (count c) (str \"x\" \"y\")) c",
    ),
    (
        "the numeric tower and float edges",
        "(println (/ 1.0 0.0) (/ -1.0 0.0) (+ 1 2.5)) (* 1.0e308 10.0)",
    ),
    (
        "a tail loop, which suspends in a hot frame",
        "(def go (fn go [n acc] (if (< n 1) acc (go (- n 1) (+ acc n))))) (println (go 12 0)) (go 5 0)",
    ),
    (
        "a program that ends by throwing",
        "(println :before) (throw {:type :app-error :kind :nope})",
    ),
    // The gensym counter is VM state a program can advance at run time, and
    // dropping it from the `Image` was invisible until this line existed: a
    // resume that restarted the counter reissues a name it already handed out,
    // which is the one thing a fresh symbol may never do.
    (
        "a run-time gensym counter",
        "(println (gensym \"x\") (gensym \"x\")) (gensym \"x\")",
    ),
    // Negative zero has bitten this project once already — milestone 6's first
    // in-language run found `-0.0` and `0.0` sharing a constant-pool entry. An
    // encoder that stored an `f64` rather than its bits, or normalised on the
    // way through, loses the sign and `(/ 1.0 z)` flips from `##-Inf` to
    // `##Inf`. Computed rather than written, so it does not depend on the
    // reader.
    (
        "a negative zero, whose sign only shows on division",
        "(def z (* -1.0 0.0)) (println z (/ 1.0 z)) z",
    ),
];

// --- The property --------------------------------------------------------------

#[test]
fn a_snapshot_at_every_instruction_boundary_reproduces_the_uninterrupted_run() {
    for (name, src) in PROGRAMS {
        let want = uninterrupted(src);
        let n = steps(src);
        assert!(n > 1, "{name}: nothing to cut");
        for cut in 1..n {
            let got = cut_and_resume(src, cut);
            assert_eq!(
                got, want,
                "{name}: cutting after {cut} of {n} instructions changed the run"
            );
        }
    }
}

/// The same programs, snapshotted *repeatedly* — every single instruction, all
/// the way to the end, each resume starting from a VM the previous one has
/// never touched.
///
/// The single-cut property above proves one round trip is lossless. This proves
/// loss does not accumulate, which is a different claim: a field restored to a
/// plausible-but-wrong default survives one trip and drifts over fifty.
#[test]
fn snapshotting_every_instruction_in_a_chain_reproduces_it_too() {
    for (name, src) in PROGRAMS {
        let want = uninterrupted(src);
        let (mut vm, chunk) = build(src);
        let mut ex = vm::start(&chunk);
        let outcome = loop {
            let (outcome, next, _) = vm::run_fueled(&mut vm, &chunk, ex, 1);
            match outcome {
                Outcome::Suspended => {
                    let img = image::capture(&vm, &next, &chunk).expect("no handles are open");
                    let (fresh, restored) =
                        image::restore(&img, &chunk).expect("the chunk matches");
                    vm = fresh;
                    ex = restored;
                }
                done => break done,
            }
        };
        let got = finish(&mut vm, outcome);
        assert_eq!(got, want, "{name}: chained snapshots drifted");
    }
}

// --- What an `Image` refuses ----------------------------------------------------

/// Needs a capability, not a language feature: without `fs` there is no
/// `io/open` to hold a handle open with, so ADR-013's subtracted build has
/// nothing to refuse (`tests/lang.rs` skips its io suite for the same reason).
#[test]
#[cfg(feature = "fs")]
fn a_live_handle_refuses_the_snapshot() {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("snapshot-live.txt");
    std::fs::write(&path, "x").expect("writes");
    let src = format!(
        "(def f (io/open \"{}\" :read)) (println :open) f",
        path.display()
    );

    let (mut vm, chunk) = build(&src);
    // Far enough in to have opened the file, short of the end.
    let n = {
        let (mut probe, probe_chunk) = build(&src);
        let mut ex = vm::start(&probe_chunk);
        let mut n = 0;
        loop {
            let (o, next, _) = vm::run_fueled(&mut probe, &probe_chunk, ex, 1);
            ex = next;
            match o {
                Outcome::Suspended => n += 1,
                _ => break n,
            }
        }
    };
    let (outcome, ex, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), n - 1);
    assert!(matches!(outcome, Outcome::Suspended));

    // ADR-029: adapter checkpointing is a later opt-in, so this is a refusal
    // and not a best effort. The count excludes the two standard streams —
    // without that exemption every snapshot would be refused, because ADR-042
    // made `io/stdin` and `io/stdout` permanent entries in the table.
    assert_eq!(
        image::capture(&vm, &ex, &chunk).unwrap_err(),
        SnapshotError::SnapshotHasLiveHandles(1)
    );
}

#[test]
#[cfg(feature = "fs")]
fn a_snapshot_with_no_open_files_is_allowed_after_they_close() {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("snapshot-closed.txt");
    std::fs::write(&path, "x").expect("writes");
    // Two `with-open`s, not one. The second reuses the slot the first freed,
    // so its handle id carries a bumped generation — and that id reaches the
    // transcript. An `Image` that dropped the free list would hand the second
    // open a fresh slot instead, printing `2:0` where the uninterrupted run
    // printed `2:1`; one that dropped the generations would index past the end
    // of a table it had just shortened. Neither was caught until this became
    // two opens (`notes/milestone-8-mutants.md`).
    let src = format!(
        "(with-open [f (io/open \"{p}\" :read)] (println :first))\n\
         (with-open [g (io/open \"{p}\" :read)] (println g))\n\
         :after",
        p = path.display()
    );
    let want = uninterrupted(&src);
    let n = steps(&src);
    // Only the tail, where the handle is closed again — the point is that
    // closing restores the ability to snapshot, not that every cut works.
    let mut allowed = 0;
    for cut in 1..n {
        let (mut vm, chunk) = build(&src);
        let (outcome, ex, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), cut);
        assert!(matches!(outcome, Outcome::Suspended));
        if image::capture(&vm, &ex, &chunk).is_ok() {
            allowed += 1;
            assert_eq!(cut_and_resume(&src, cut), want, "cut {cut} of {n} drifted");
        }
    }
    assert!(
        allowed > 0,
        "no cut was snapshottable, so this test proved nothing"
    );
}

#[test]
fn an_image_refuses_a_chunk_it_was_not_taken_from() {
    let (mut vm, chunk) = build("(def a 1) (+ a 1)");
    let (outcome, ex, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), 1);
    assert!(matches!(outcome, Outcome::Suspended));
    let img = image::capture(&vm, &ex, &chunk).expect("no handles are open");

    let (_, other) = build("(def a 2) (+ a 1)");
    assert_eq!(
        image::restore(&img, &other).err(),
        Some(SnapshotError::ChunkMismatch)
    );
    // And the one it *was* taken from still works, or the check above would
    // pass by refusing everything.
    assert!(image::restore(&img, &chunk).is_ok());
}

// --- Sharing ---------------------------------------------------------------------

/// ADR-043 part 2. Nothing in the language can observe the difference between
/// shared structure and copies — `=` is structural and there is no
/// `identical?` — so no in-language test can hold this, and without it the
/// encoder could expand `[a a]` into two vectors and pass every other test in
/// this file.
///
/// Measured as a *delta*, because an `Image` of the smallest possible program
/// already holds sixty-odd objects: one closure per primitive, plus the
/// prelude's macros. The absolute count says nothing; what these three `def`s
/// add says everything.
#[test]
fn shared_structure_encodes_once_rather_than_per_reference() {
    fn objects(src: &str) -> usize {
        let (mut vm, chunk) = build(src);
        let n = steps(src);
        let (outcome, ex, _) = vm::run_fueled(&mut vm, &chunk, vm::start(&chunk), n - 1);
        assert!(matches!(outcome, Outcome::Suspended));
        image::capture(&vm, &ex, &chunk)
            .expect("no handles are open")
            .object_count()
    }

    let base = objects("(def a [1 2 3]) a");
    // Four levels of doubling: sixteen references to one three-element vector.
    let shared = objects("(def a [1 2 3]) (def b [a a]) (def c [b b]) (def d [c c]) d");

    // One new object each for `b`, `c`, and `d`. An encoder that walked the
    // tree instead of interning by address would add fourteen — `d` alone
    // expands to fifteen vectors — and the gap widens by a factor of two for
    // every further level, which is the whole argument for ADR-043 part 2.
    assert_eq!(
        shared - base,
        3,
        "expected 3 new objects (b, c, d); a copying encoder adds 14"
    );
}
