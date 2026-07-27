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
| REPL, disassembler, diagnostics | 600 |
| **Total core** | **~7,000** |

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
| 4 | Errors, `try`/`throw`/`finally`, handler stack | Failure transcripts in the corpus; cleanup runs exactly once |
| 5 | Macro expansion + quasiquote + gensym | `defmacro` in-language; deterministic output |
| 6 | Collections, strings, bytes | In-language test suite begins |
| 7 | Host handle table + blocking file/stdio | `with-open` works; handles are generational |
| 8 | Fuel suspension + `Image` + resume | Round-trip property passes; live handles are refused |
| 9 | REPL | Becomes the primary development interface |
| 10 | Host adapters: terminal, TCP, JSON | Outside the line budget |

## The oracle

Constraint #1 already demands that every phase be printable. A printable phase is
a snapshot-testable phase — inspectability and verifiability are the same
feature, so building the printers first means the oracle mostly exists.

Climb in this order:

**Rung 1 — it typechecks.** `cargo check`. Always on, free.

**Rung 2 — it runs.** `smoke.sh`: read, expand, compile, and execute a hello
program end to end; exit nonzero on failure. Write this *before* the reader is
finished — a failing smoke test is a better queue than an empty one.

The stages run in **pipeline** order, which is not the order they are built in:
expand is milestone 5 while compile and run are 2 and 3. So a pending stage does
not stop the run. The driver exits `3` for "not built yet", and smoke reports
that stage as pending and carries on; any other nonzero exit is a real failure
and stops immediately. Smoke stays nonzero while anything is pending — that is
the queue — but a milestone that lands is reachable the day it lands rather than
waiting on a later one.

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

`.spans` is the fifth, added by ADR-026 point 3. It exists because origins live
outside the value graph, so the printed form cannot show them, and a mutation
check proved the structural invariant alone was dead without it — see
`notes/milestone-1-pilot.md`.

`.out` is a canonical transcript, not just stdout: exit status, the final value
*or* the thrown value, stdout, and any diagnostics. Milestone 4 puts failures in
the corpus, and a failure with no defined record is a failure that cannot be
pinned.

Only the programs that *run* carry one, and which those are is asserted as a
list rather than inferred from which files happen to exist — so adding a corpus
program forces the choice instead of silently skipping it. `just bless` updates
a `.out` where one exists and never creates one, because creating the first one
for a program is the decision. The rest of the corpus faults on globals nothing
defines yet, and a fault transcript cannot be pinned before Q23 says what a
thrown value is.

A snapshot per phase localizes a regression to one phase before anyone reads
a diff. This is the highest-leverage thing in the project and should exist by the
end of week one. It is also what makes reckless optimization safe (ADR-021) and
what keeps the system discussable once the code is real.

**Rung 4 — behavior is specified.** A test suite written **in the language
itself** wherever possible, so it survives implementation churn and doubles as a
dogfooding pass. Keep it independent of the internals it tests.

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

## On process

You write it, we argue about it, the corpus catches regressions. A diff-only
review by a fresh context — given the change and none of the reasoning behind it,
tasked only with enumerating why it is broken — is a tool to reach for when
something is subtle, not a mandatory stage. SPEC v0.1's loop methodology (paired
adversarial reviewers per diff, mechanical queues, a pilot phase, an operator
role) is million-line-port machinery and is heavier than the reader it was meant
to produce. It lives in commit `c494f2a` if it is ever needed.
