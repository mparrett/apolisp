# Build order, budget, and the oracle

## Line budget

| Layer | Target |
|---|---:|
| Reader, forms, metadata | 600 |
| Macro expansion | 600 |
| Core AST, lowering, bytecode compiler | 1,100 |
| VM: frames, calls, closures, errors | 1,100 |
| Values, collections, strings, bytes | 900 |
| Host handle table + blocking I/O | 500 |
| REPL, disassembler, diagnostics | 500 |
| **Total core** | **~5,200** |

Tests, host adapters (HTTP, terminal, JSON), and tooling live **outside** this
budget. The boundary is the point: *substantial host capability is a Rust library
behind the handle table, not a language subsystem.*

Constraint #1 is the only governing constraint with no test. A per-module
line-count assertion is ~20 lines and turns this table into an oracle rung; add it
once there are two modules.

## Build order

Each milestone is a runnable artifact.

| # | Milestone | Exit condition |
|---:|---|---|
| 1 | Reader + printer + forms with metadata | Round-trip property test passes |
| 2 | Core AST + slot compiler + disassembler | Golden `.disasm` for hand-written forms |
| 3 | VM: frames, calls, closures, `if`/`let`/`fn` | `smoke.sh` runs a recursive function |
| 4 | Errors, `try`/`throw`, structured diagnostics | Failure cases in the golden corpus |
| 5 | Macro expansion + quasiquote + gensym | `defmacro` in-language; deterministic output |
| 6 | Collections, strings, bytes | In-language test suite begins |
| 7 | Host handle table + blocking file/stdio | `with-open` works; handles are generational |
| 8 | Serialization + suspend/resume | Round-trip property test passes |
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

**Rung 3 — behavior is pinned.** A corpus of `.xs` programs, each with four
committed snapshots:

```
tests/corpus/<name>.xs
tests/corpus/<name>.forms      # reader output, printed
tests/corpus/<name>.expanded   # post-macroexpansion forms
tests/corpus/<name>.disasm     # bytecode disassembly
tests/corpus/<name>.out        # stdout + final value
```

Four snapshots per program localizes a regression to a phase before anyone reads
a diff. This is the highest-leverage thing in the project and should exist by the
end of week one. It is also what makes reckless optimization safe (ADR-021) and
what keeps the system discussable once the code is real.

**Rung 4 — behavior is specified.** A test suite written **in the language
itself** wherever possible, so it survives implementation churn and doubles as a
dogfooding pass. Keep it independent of the internals it tests.

## Property tests

Three, each pinning a stated design property rather than an implementation
detail:

- **Reader round-trip.** `read(print(read(s))) == read(s)`. Catches metadata loss
  and printer/reader drift.
- **Serialization round-trip.** Suspend a running VM mid-computation, serialize,
  deserialize into a fresh VM, resume, and compare the final result and stdout
  against uninterrupted execution. **This is the oracle for constraint #2** —
  without it, that constraint is an aspiration rather than a property. See Q7 for
  what "suspend" means in a blocking-only v1.
- **Differential testing.** For the subset that overlaps Clojure semantics, diff
  against Babashka. Scope before building — see Q16.

## Two rules

**The oracle is append-only.** No golden file is regenerated to go green without a
human reading the diff and saying why. Regenerating a snapshot to make a test pass
is a failed task, not a fix. This is the one rule that would destroy the whole
approach if it slipped.

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
