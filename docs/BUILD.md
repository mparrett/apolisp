# Build order, budget, and the oracle

## Line budget

Every line counts — comments and blanks included, because constraint #1 is about
what has to be held at once, and a comment occupies the window like anything else
(ADR-030).

| Layer | Target |
|---|---:|
| Reader, printer, forms, metadata | 900 |
| Macro expansion | 800 |
| Core AST, lowering, bytecode compiler | 1,400 |
| VM: frames, calls, closures, errors | 1,400 |
| Values, collections, strings, bytes | 1,200 |
| Host handle table + blocking I/O | 700 |
| Fuel, `Image`, resume | 500 |
| REPL, disassembler, diagnostics | 600 |
| **Total core** | **~7,500** |

The serialization row was added by ADR-043, which is late: ADR-029 invented the
`Vm`/`Execution`/`Image` split after this table existed, and ADR-030 raised the
total without noticing the layer had no line. It was unbudgeted for four
milestones and nothing said so, because only the total is asserted.

**These are orders of magnitude.** ±1,000 on the total is noise; the rows are
guidance. Only the total is asserted, and the per-layer numbers print on every
run so the shape stays visible. The question a row answers is "did this subsystem
double," never "is this 40 lines over."

The budget is hard and amendable by ADR. Over budget is a decision to record in a
new entry, not a number to nudge or a test to silence.

## Prose budget

The documents get a cap of **20,000 lines** (ADR-056) — everything under `docs/`,
with the write-ups' duplicated `<style>` blocks excluded and printed. Unlike the
line budget it has no working target underneath it: the number *is* the
assertion, because it is a tripwire against getting carried away rather than a
size to build to. The test prints the current total and the per-file breakdown.

The asymmetry it closes is that constraint #1 is a context-window constraint and
had only ever been enforced against `src/`, while `ADR.md` grew to half the size
of the language it describes.

Tests, host adapters (HTTP, terminal, JSON), and tooling live **outside** this
budget. The boundary is the point: *substantial host capability is a Rust library
behind the handle table, not a language subsystem.*

## Build order

Each milestone is a runnable artifact.

| # | Milestone | Exit condition |
|---:|---|---|
| 1 | Reader + printer + forms with span origins | **Done** (`ffcefa7`) — round-trip + span-invariants properties pass |
| 2 | Core AST + slot compiler + disassembler | **Done** (`38d109c`) — `.disasm` goldens for six corpus programs |
| 3 | VM: frames, calls, closures, `if`/`let`/`fn`, tail calls | **Done** (`8ff2c9f`) — smoke runs `run`; 100k tail calls peak at the same frame *and* slot count as 10 |
| 4 | Errors, `try`/`throw`/`finally`, handler stack | **Done** (`aa7a20b`) — `control.xs` and `errors.xs` transcripts; cleanup counted on all four paths |
| 5 | Macro expansion + quasiquote + gensym | **Done** (`6c4785f`) — `defmacro` is a prelude macro; expansion is deterministic per unit |
| 6 | Collections, strings, bytes | **Done** (`de916cc`) — `tests/lang/` runs through the binary, and caught two bugs on its first run |
| 7 | Host handle table + blocking file/stdio | **Done** (`1e7555e`) — `with-open` closes on all four paths; a stale id reaches nothing |
| 8 | Fuel suspension + `Image` + resume | **Done** (`ab75b1b`) — the round-trip cuts at *every* instruction boundary; live handles are refused |
| 9 | REPL | **Done** (`5644b58`) — a session is one unit and one chunk; a function defined in one input is callable from the next |
| 10 | Host adapters: terminal, TCP, JSON | **Done** (`6017088`) — outside the budget, and the exclusion prints; ADR-042's three network kinds finally have raisers |

## The oracle

Constraint #1 already demands that every phase be printable. A printable phase is
a snapshot-testable phase — inspectability and verifiability are the same
feature, so building the printers first means the oracle mostly exists.

Climb in this order:

**Rung 1 — it typechecks.** `cargo check`. Always on, free.

**Rung 2 — it runs.** `smoke.sh`: read, expand, compile, and execute a hello
program end to end; exit nonzero on failure. Write this *before* the reader is
finished — a failing smoke test is a better queue than an empty one.

The stages run in **pipeline** order, which was not the order they were built
in: expand is milestone 5 while compile and run were 2 and 3. While the pipeline
had holes, the driver exited `3` for "not built yet" and smoke reported that
stage as pending and carried on — so a milestone was reachable the day it landed
rather than waiting on a later one, and smoke stayed nonzero while anything
remained. That was the queue.

Milestone 5 filled the last hole and the mechanism went with it: an unreachable
branch is one nobody tests. Smoke now runs four stages, and any nonzero exit is
a failure.

**Rung 3 — behavior is pinned.** A corpus of `.xs` programs, each with a committed
snapshot per phase:

```
tests/corpus/<name>.xs
tests/corpus/<name>.forms      # reader output, printed
tests/corpus/<name>.spans      # the same forms with their origins (ADR-026)
tests/corpus/<name>.expanded   # post-macroexpansion forms
tests/corpus/<name>.disasm     # bytecode disassembly (milestone 2)
tests/corpus/<name>.out        # execution transcript (milestone 3, see below)
```

`.expanded` is milestone 5's, and on a program with no macros it is identical
to `.forms` — which is what makes the diff between the two readable as "what
expansion did". A macro's output origins are pinned by `.disasm` rather than
here: the disassembly prints a source position per instruction, and those come
from the origins the expander attached.

`.spans` is the fifth, added by ADR-026 point 3. It exists because origins live
outside the value graph, so the printed form cannot show them, and a mutation
check proved the structural invariant alone was dead without it — see
`notes/milestone-1-pilot.md`.

`.out` is a canonical transcript, not just stdout: exit status, the final value
*or* the thrown value, the position it was raised at, and any errors it
displaced. A failure with no defined record is a failure that cannot be pinned,
which is why milestone 4 could not start before ADR-039 fixed what a thrown
value is.

Only the programs that *run to a defined end* carry one, and which those are is
asserted as a list rather than inferred from which files happen to exist — so
adding a corpus program forces the choice instead of silently skipping it. Since
ADR-039 that list includes programs that fail: a fault is a value, so `control.xs`
pins one and `errors.xs` pins a cleanup running on every path. `just bless`
updates a `.out` where one exists and never creates one, because creating the
first one for a program is the decision.

The driver prints the path it was given, so the corpus harness runs it from the
repository root with a relative path. An absolute one would put the checkout
directory in a golden file.

A snapshot per phase localizes a regression to one phase before anyone reads
a diff. This is the highest-leverage thing in the project and should exist by the
end of week one. It is also what makes reckless optimization safe (ADR-021) and
what keeps the system discussable once the code is real.

**`examples/` — the half a golden cannot reach.** A program that needs a terminal
cannot carry a `.out`, and leaving it out of the repository entirely is how the
editor's shell spent four core changes going stale in a scratchpad. So the
corpus keeps the pure half and `examples/` keeps the impure one, and the split is
the same claim the pure/impure architecture makes rather than a filing decision.

A file is a compilation unit and there is no `load`, so the two halves are joined
by `just edit FILE`, which cuts the corpus program at its script marker, appends
the shell, and appends the `(edit "FILE")` call — which is also how a program
takes an argument in a language with no argv.

`the_editor_shell_still_fits_the_core` keeps them honest. It **evaluates** the
join rather than compiling it, minus the one call that needs a tty: globals here
resolve at call time, so a shell calling something the core deleted *compiles*
perfectly and faults only when the line runs. A compile check was written first
and passed with a call to a function ADR-052 had deleted still sitting in it.
Anything in `examples/` needs a check that runs, or it is a file nobody notices
has rotted.

**Rung 5 — the checks can fail.** `just mutate` (ADR-055). Rungs 1 to 4 answer
"is the code right"; this one answers "would we know if it were not". Each entry
breaks one load-bearing line and asserts the named test flips from pass to fail.

It is opt-in rather than part of `verify`, because every mutation is a rebuild
and a gate measured in minutes is a gate people learn to skip. Run it when
touching something a check is supposed to hold.

Three things are asserted separately, because every way a mutation rots is
silent: that the edit changed the file, that the mutant still builds, and that
the test failed — the last from the exit status rather than from grepping output
for a word. `../reg-lisp` had twenty of eighty-two checks go quiet without it
showing, for exactly that reason.

A mutation may declare that it *should* survive, with a reason. Two kinds of
survivor are legitimate: a claim no test can separate, and a guard against a
failure severe enough to enforce twice on purpose. A declared survivor that
starts dying is reported too — that means the reason stopped being true.

**Rung 4 — behavior is specified.** A test suite written **in the language
itself** wherever possible, so it survives implementation churn and doubles as a
dogfooding pass. Keep it independent of the internals it tests.

Landed at milestone 6: `tests/lang/*.xs`, with `tests/lang.rs` as a runner that
knows only how to start the binary. The harness is *concatenated* ahead of each
suite rather than imported, because there is no `require` and one namespace
(ADR-027, Q12) — the paste is the clearest statement of what a module system
would buy. A failing assertion throws, so the failure arrives as a transcript
naming the form.

It earned its place immediately: two bugs on the first run, one of them a
miscompilation that had been live since milestone 2 (`-0.0` and `0.0` sharing a
constant-pool entry, so `(/ 1.0 0.0)` could produce `##-Inf`). The milestone-6
mutation pass then found that this rung is the *only* thing that catches any of
the semantics milestone 6 added — see `notes/milestone-6-mutants.md`.

Milestone 7 qualifies that, and the qualifier is what keeps the rungs from
collapsing into one. Rung 4 remains the only thing that catches a *semantic*
mutation, but an invariant no program can observe needs a Rust test: a handle
table that queued a freed slot twice passed the entire in-language suite,
because no program can ask the VM how many slots it holds. Where a claim is
about the machine rather than about the language, `tests/` is the only rung
that can hold it (`notes/milestone-7-mutants.md`).

## Property tests

Three, each pinning a stated design property rather than an implementation
detail:

- **Reader round-trip.** `read(print(read(s))) == read(s)`, comparing data and
  **ignoring span origins** — printing moves columns, so a span-sensitive
  equality here can only fail. Catches printer/reader drift. Span behavior is
  pinned separately by the span-invariants property and the `.spans` snapshots
  (ADR-026).

  Compared on **values, not on printed strings** (ADR-031). String comparison
  looks like the same test and is not: it cannot see a round trip that changes
  type while printing identically, which is exactly how `1e400` escaped
  (ADR-032). Floats compare by bit pattern, so `##NaN` and `-0.0` mean what
  they should here regardless of how Q13 settles language `=`.
- **Serialization round-trip.** Run to fuel exhaustion at an instruction
  boundary, take an `Image`, resume it in a fresh VM of the same build, and
  compare the full transcript against uninterrupted execution. Runs against a
  **buffered in-memory host**, so emitted effects are part of the comparison
  rather than escaping it. **This is the oracle for constraint #2** — without it,
  that constraint is an aspiration rather than a property (ADR-029).

  Landed at milestone 8 in `tests/snapshot.rs`, in two forms: cut at *every*
  instruction boundary and round-trip once, and cut at every boundary and
  round-trip *repeatedly* to the end. The second is a different claim — that
  loss does not accumulate, which a field restored to a plausible default
  survives once and fails over fifty.

  **Its limit is worth knowing before trusting it.** A round-trip property
  tests the encoding of whatever its corpus constructs and nothing about what
  it does not. Milestone 8's mutation pass dropped four separate fields from
  the `Image` — the gensym counter, the handle free list, the handle
  generations, and the sign of a negative zero — and this property, in both
  forms, over nine programs, noticed none of them. Adding a piece of state to
  the VM means adding a program that creates it; the property will not ask.
- **Differential testing.** For the subset that overlaps Clojure semantics, diff
  against Babashka. Scope before building — see Q16.

## Pre-registration

Before running a benchmark, write down what you expect and why. Afterwards,
record whether it was refuted. ADR-021 removes the gate on optimization work;
pre-registration is what turns that freedom into knowledge rather than folklore —
and the interesting results are the refutations. `../wallisp` runs this way, and
several of its headline findings are its own hypotheses being falsified
(`PRIOR-ART.md`).

Two lines in a commit message is enough. This is a habit, not a document.

**Check that the mutant applied.** A substitution whose pattern no longer
matches leaves the tree untouched, the suite green, and a run that is
indistinguishable from a surviving mutant — and the wrong reading is the
flattering one, because "it survived" is a finding and "it never happened" is a
wasted run recorded as a finding. Assert the old text was present before
writing the new. Milestone 9 lost two of twelve mutants this way and only
noticed because one of them had been *predicted* to survive, so checking the
prediction meant opening the file (`notes/milestone-9-mutants.md`).

**And check that it reached the binary.** There is a second way to get the same
wrong answer, and the rule above does not catch it: the mutant is in the source,
`grep` confirms it, the build recompiles, and the optimizer deletes it. A
`Box` allocated, never read and never freed is unobservable, so LLVM is entitled
to remove it — 150,000 injected leaks produced a run where allocations and frees
still matched (`notes/soak-leak-check.md`). Anything mutated under `--release` is
exposed, which makes the soak exactly where this lives. The vulnerable shape is a
mutant that adds work with no observable effect; one that changes an observable
result is safe. **Read the counters, not just the verdict** — allocation totals,
instruction counts, a transcript. A verdict alone cannot distinguish "nothing
leaked" from "nothing happened", and the flattering reading is still the wrong
one.

When a pass outgrows a commit message it goes in `notes/`, one file per
milestone, named `milestone-N-<topic>.md`. A pass belongs to the milestone whose
code it mutated rather than to the session that ran it — milestone 3's was filed
under milestone 2 for a while, and the cross-reference in a later note is what
found it.

## Two rules

**The oracle is review-gated.** No golden file is regenerated to go green without
a human reading the diff and saying why. Regenerating a snapshot to make a test
pass is a failed task, not a fix. This is the one rule that would destroy the
whole approach if it slipped.

Golden files *must* change when this deliberately unstable language changes — the
rule is a reviewed behavioral diff, not immutability. "Append-only" is reserved
for `ADR.md`, where it is literal.

**Determinism is a prerequisite, not a nice-to-have.** Gensym counters must be
deterministic per compilation unit. Map iteration order must be deterministic
wherever it can reach output. Nondeterminism makes golden files flap, flapping
golden files get disabled, and then there is no oracle.

## Verification ladder

```
implement → cargo check → smoke passes → golden corpus green →
in-language suite green → serialization round-trip green → merge → soak → tag
```

**Merge ≠ release.** The soak is where leak checks, reader fuzzing, and
release-build divergence testing happen. It is `just soak` — two legs anywhere,
and `just soak-linux` for all three, because the leak check needs valgrind. It
was a word in this document until it was written; the leg that needed a tool
was the leg that kept it hypothetical.

- **Release-build divergence.** `cargo test --release`. The goldens run the
  *binary*, so under `--release` they pin the release artifact rather than
  re-running library code. Nothing has diverged: 108 tests, both profiles, as of
  2026-08-02.
- **Reader fuzzing.** `the_reader_survives_arbitrary_input` in `tests/reader.rs`,
  which is in the ordinary suite at a small round count and cranked by
  `APOLISP_FUZZ_ROUNDS` for the soak. Two generators — token soup and corpus
  mutation — and three claims: the reader never panics, whatever it accepts
  round-trips with well-formed origins, and whatever it rejects renders. That
  last one is free, because rendering an error with a span outside the source or
  off a character boundary panics rather than returning a wrong answer.

  Corpus mutation is the productive half by a wide margin: 68% of its inputs
  parse against soup's 13%, so it is what actually reaches the accept-side
  oracles. Verified non-vacuous by counting, and verified to have teeth by
  reintroducing the multi-byte escape-span bug the reader's own comment warns
  about — caught in 246 inputs, with the input named.
- **Leak checks.** Valgrind over `tests/soak/*.xs`, definite losses only, since
  Rust's runtime leaves reachable allocations at exit by design. What validates
  the check is a recorded mutation pass (`notes/soak-leak-check.md`), not a
  fixture: a leak this language cannot express cannot be provoked by a program
  written in it.

**The gate also runs on Linux.** `just verify-linux` builds the `Dockerfile` and
runs exactly `just verify` inside it — exactly, because a container that ran a
rung the host gate does not would make a red container ambiguous between "Linux
differs" and "this rung was never in the gate". Not a dependency of `verify`: it
needs a running daemon, and a gate with an external prerequisite is one people
learn to skip.

The point was never portability as a goal. It is that a platform-specific
assumption baked into a golden or a `:kind` mapping is invisible on the machine
that wrote it.

*It found nothing, and the nothing is worth writing down.* The expectation was
that socket error classification would be where macOS and Linux parted, since
that is the newest code and the one place `TRAPS.md` already records a platform
split. Both give `:would-block` for a read deadline, both classify a write to a
closed peer as `:connection-reset`, and both take exactly two writes to get
there. 108 tests, three feature-lattice points, no diff, as of 2026-08-02. So
the `:would-block`
**or** `:timeout` tolerance in `tests/adapters.rs` is carrying Windows alone —
it is not a macOS-versus-Linux hedge, and no run has ever taken the branch that
needs it.

What this does **not** cover: the daemon on an arm64 host runs arm64 Linux, so
this is an OS axis and not an architecture one. `--platform linux/amd64` gets
the other, emulated and slow, and nothing has yet suggested an arch-sensitive
assumption is in here to find.

## On process

You write it, we argue about it, the corpus catches regressions. A diff-only
review by a fresh context — given the change and none of the reasoning behind it,
tasked only with enumerating why it is broken — is a tool to reach for when
something is subtle, not a mandatory stage. SPEC v0.1's loop methodology (paired
adversarial reviewers per diff, mechanical queues, a pilot phase, an operator
role) is million-line-port machinery and is heavier than the reader it was meant
to produce. It lives in commit `c494f2a` if it is ever needed.
