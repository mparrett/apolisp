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
release-build divergence testing happen.

**Follow-up: a Dockerfile that runs `just verify` on Linux.** Everything to
date has been verified on macOS only, and milestone 10 is where that stopped
being harmless. `TRAPS.md` records a read deadline raising `:would-block` on
Unix and `:timeout` on Windows; the same class of divergence is available
between macOS and Linux for socket errors, terminal capability detection, and
path handling, and none of it is exercised. The point is not portability as a
goal — it is that a platform-specific assumption baked into a golden or a
`:kind` mapping is invisible on the machine that wrote it. Cheap, and it makes
the existing gate say something it currently only implies.

## On process

You write it, we argue about it, the corpus catches regressions. A diff-only
review by a fresh context — given the change and none of the reasoning behind it,
tasked only with enumerating why it is broken — is a tool to reach for when
something is subtle, not a mandatory stage. SPEC v0.1's loop methodology (paired
adversarial reviewers per diff, mechanical queues, a pilot phase, an operator
role) is million-line-port machinery and is heavier than the reader it was meant
to produce. It lives in commit `c494f2a` if it is ever needed.
