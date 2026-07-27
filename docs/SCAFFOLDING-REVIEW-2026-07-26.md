# Project scaffolding review

**Date:** 2026-07-26  
**Scope:** repository structure, active design documents, milestone-1 implementation,
tests, corpus, and local build commands at `83f49ae`.

## Executive assessment

This is not enterprise software wearing a tiny-language costume. The production
shape is admirably direct: one crate, no dependencies, concrete data types, a
hand-written reader, no trait lattice, no dependency injection, no premature
workspace, and no generic compiler framework. Most unusual choices are explicit
responses to the charter rather than novelty for its own sake.

The project is nevertheless **over-scaffolded in its decision process and
under-conventional at its test boundary**. Thirty ADRs and nine errata exist
before milestone 2, while the one-file rule has already forced tests through a
subprocess and reduced a claimed value-level property to string comparison. The
result is the wrong trade: substantial machinery explains correctness, but the
test boundary cannot directly observe it.

The core architecture should continue. The immediate correction is not a larger
framework or more process. It is to introduce one conventional library boundary,
make the current verification ladder honest and green, and reduce the amount of
historical reasoning an implementer must load to discover the current design.

Two correctness defects found during this review reinforce that conclusion:

1. `1e400` reads as `Float(infinity)`, prints as `##Inf`, and then reads as a
   `Sym`. The round-trip test still passes because it compares the two printed
   strings rather than the two values.
2. An unknown escape containing a three-byte Unicode character, such as
   `"\€"`, panics while rendering the error because the computed byte offset is
   not a UTF-8 boundary.

## Calibration against the charter

| Dimension | Assessment |
|---|---|
| Essential implementation complexity | Good. The code is small, concrete, and mostly linear. |
| “Best-practice” over-adherence | Moderate in decision records and oracle terminology; low in the implementation. |
| Enterprise patterns | Largely absent from code; the append-only ADR/governance model is the main exception. |
| Idiomatic Rust | Good data modeling and ownership choices; weak crate/test boundary; formatting and one minor lint are outstanding. |
| Well-trodden architecture | Mostly yes: forms-as-values, explicit frames, a register-like VM, `Rc` in a single-threaded runtime, golden compiler phases. |
| Deliberate deviations | Usually justified: one file, no lazy sequences, snapshot-ready state, no compatibility contract. The one-file deviation has now reached its stopping condition. |
| Verification proportionality | The phase corpus is promising, but the present “property” and smoke ladder overstate what they verify. |

The repository currently contains roughly:

- 901 lines of core implementation;
- 359 lines of Rust tests;
- 2,074 lines in the README and active/consulted/evidence documents;
- 870 lines of notes and archived reviews;
- 4,419 tracked lines in total.

That ratio is not automatically bad: much of the design is for later milestones,
and serializable execution state really does reward early decisions. It does mean
that documentation is already the largest contributor to the context-window
constraint, even though only `src/*.rs` is budgeted.

## Findings

### 1. The one-file rule has started making correctness less observable

**Priority: high — change before milestone 2.**

[`tests/reader.rs`](../tests/reader.rs) explains that properties use the binary
because the crate has no `lib.rs` (lines 13–16). This creates several secondary
mechanisms:

- each test writes a temporary file and launches a process;
- value equality cannot be tested even though `Value::PartialEq` is a deliberate,
  load-bearing implementation;
- “round-trip equality” is approximated by equality of printer output;
- the binary path is reconstructed as `target/{debug,release}/apolisp` rather
  than supplied by Cargo;
- running with `CARGO_TARGET_DIR=/tmp/...` builds the tests in that directory but
  still executes the old binary in the repository's `target` directory. The
  suite can therefore pass against a stale artifact.

This is concrete evidence that ADR-015's “one file until it hurts” threshold has
been reached. A `src/lib.rs` plus the thin `src/main.rs` driver is not enterprise
layering; it is the standard Rust seam between behavior and process I/O. Keep the
implementation in one library file if the single-reading-view benefit still
matters. Tests can then call `reader::read_all` and `printer::print` directly.
CLI-specific tests should use Cargo's `CARGO_BIN_EXE_apolisp` path rather than
constructing a target path.

The benefit is immediate: test `read(print(read(s)))` on `Value`, test the
hand-written `PartialEq`, remove most temp-file/process scaffolding, and make
non-default target directories hermetic.

### 2. The round-trip “property” admits a type-changing false positive

**Priority: high — resolve with finding 1.**

[`src/main.rs`](../src/main.rs) accepts any `f64` parse result at lines 793–795,
including infinity. The printer emits `##Inf`, `##-Inf`, and `##NaN` at lines
887–893. The reader has no dispatch for those tokens, so it reads them as
symbols. For example:

```text
1e400       --read--> Float(infinity)
            --print--> ##Inf
            --read--> Sym("##Inf")
            --print--> ##Inf
```

The printed strings match, so both passes in `round_trip_covers_awkward_scalars`
would agree even though the data does not. Calling this a property obscures two
limitations: it is a fixed corpus/metamorphic test, not generated coverage, and
it compares a projection rather than the stated values.

Q13 deliberately owns NaN and signed-zero semantics. Until Q13 is settled, the
smallest coherent behavior is either to reject non-finite parsed literals or to
teach the reader the tokens the printer emits. In either case, add direct value
tests for positive/negative infinity, NaN policy, `-0.0`, and float-vs-integer
identity. A property-testing dependency is optional; a small deterministic form
generator would fit this project better than adopting a framework by reflex.

### 3. Reader-controlled input can panic in diagnostic rendering

**Priority: high — small local fix.**

The escape error at `src/main.rs:693–695` constructs its span with
`self.pos - 2`. That assumes the escaped item occupied one byte. For `"\€"`,
`bump` advances across the three-byte character, `self.pos - 2` lands inside it,
and `line_col` slices the source at that non-boundary (`src/main.rs:173`). The
binary panics rather than returning a `LispErr`.

Capture the escape's starting offset before advancing and build the span from
that saved boundary. Add non-ASCII symbols, keywords, strings, and malformed
escapes to reader tests. Do not generalize this into a source-location framework;
the current byte-span representation is otherwise appropriate.

### 4. The smoke test is a red roadmap, not a smoke test

**Priority: high — make the verification contract truthful.**

[`smoke.sh`](../smoke.sh) always invokes `read`, `expand`, `compile`, and `run`.
Only `read` exists, so it currently fails. More importantly, `expand` is milestone
5 but appears before milestone-2 `compile` and milestone-3 `run`; until milestone
5 lands, the script cannot exercise either newly completed stage. The failure is
therefore not a useful queue for build order.

This conflicts with several claims:

- BUILD calls smoke rung 2 and places “smoke passes” before the golden corpus;
- the verification ladder says it is green before merge;
- `just verify` says it runs everything that should be green before a commit,
  but omits smoke because smoke is intentionally red.

A smoke test should verify the coherent artifact that exists now and remain
green. Add stages when they land. If a no-op identity expansion is legitimately
part of the early pipeline, implement that explicitly; otherwise do not make a
future stage fail today's verification. `BUILD.md` is already the queue.

### 5. The ADR mechanism is becoming the enterprise subsystem the ethos rejects

**Priority: medium — simplify before adding many more ADRs.**

ADRs are valuable here when they protect an expensive day-one invariant:
explicit frames, snapshot boundaries, cell ownership, form/span representation,
and deterministic output are good examples. The milestone note documents real
payoff from several of them.

The append-only single log is less successful. A reader must traverse superseded
claims, correction headers, later amendments, and an errata section to learn the
current answer. At 1,044 lines and 30 decisions before the compiler exists, the
history is already competing with the “hold the core at once” goal. Git also
retains the historical reasoning, so append-only prose duplicates a property the
repository already has.

This is not merely a high document-to-code ratio. It is a lookup-cost problem:
the current truth is deliberately scattered through time. The project says it
has no governance, but “supersede, never edit,” status rules, errata rules, and a
hard amendment procedure function as governance.

Prefer one of these smaller models:

1. Make `ADR.md` a mutable current-decision ledger, with Git as history; or
2. keep full historical ADRs but put a compact active-decision index at the top,
   and move superseded bodies out of the normative reading path.

Either model should let an implementer load the present design without reading
known-wrong pseudo-code. Pause new ADRs for decisions that are cheap to reverse
or owned by an already-scheduled open question. Comments such as “Q13 owns this”
are useful local fences; they do not all need another procedure around them.

### 6. The line budget is directionally useful but mechanically contradictory

**Priority: medium.**

ADR-030 says 7,000 lines is an order of magnitude, that ±1,000 is noise, and that
precision beyond the nearest thousand is false. The test fails at 7,001. That is
an exact threshold, and it will eventually incentivize the very 40-line debate
the ADR rejects.

The test also counts only core Rust. That is correct for controlling VM size, but
it does not measure the broader stated constraint—what an engineer or model must
hold—when current documentation already exceeds core code by more than 2:1.

Keep line reporting. Prefer a deliberately wide tripwire (for example, failure
only after the accepted noise band) plus trend visibility, and report total
active documentation and tests without assigning them hard quotas. Avoid adding
a metrics system; `wc` and the existing layer report are enough.

The layer reporter itself will need adjustment if modules move to files. Its
comment says it will read filenames after extraction, but the implementation
only recognizes inline lines beginning `pub mod `.

### 7. Golden testing is high-leverage, but “every phase for every program” will
become a test matrix

**Priority: medium — revisit as the corpus grows.**

Phase goldens are a well-trodden and appropriate compiler technique. They make
reader/expander/compiler regressions local, support deterministic optimization,
and fit this language better than mocks or internal trait seams.

The universal rule is the risky part. Current `.spans` files repeat the printed
form already stored in `.forms`, and the milestone note admits that the larger
span goldens received a less careful review. BUILD promises four snapshots for
every program, while ADR-026 introduced an additional span view; the “four
snapshot” description is already stale.

Separate two roles as the corpus grows:

- a small diagnostic corpus whose selected phase outputs are all reviewed; and
- a broad semantic/in-language suite that asserts outcomes without multiplying
  every case across every phase.

Snapshot only phases a case is designed to probe. This preserves localization
without creating an enterprise-style Cartesian test matrix that humans stop
reading.

### 8. Repository commands and current-state documentation need a button-up pass

**Priority: medium — small, concrete cleanup.**

- README still says “No code yet.”
- BUILD marks milestone 1 done using a commit on the side branch rather than the
  squashed mainline commit. The object exists, but it is not an ancestor of
  `main`.
- `just bless` says it regenerates golden files but only updates `.forms`, not
  `.spans`.
- The `bless` success message contains `` `git diff` `` inside double quotes.
  The shell executes that command substitution instead of printing the
  instruction.
- The blessing comment says to run the generator only after reading the diff;
  generation is what creates the diff. The intended rule is presumably:
  generate intentionally, then review every hunk before committing.
- `just verify` hard-codes `./target/debug/apolisp`, which has the same custom
  target-directory/stale-artifact problem as the tests.
- `cargo fmt --all -- --check` currently reports formatting changes. A strict
  `cargo clippy -D warnings` run reports only `Interner::len` lacking
  `is_empty`; since `len` is unused, deleting the dead method is more
  proportional than adding API to satisfy a lint.

Add formatting to the normal local verification path because legibility is a
governing constraint. Do not add CI, a pre-commit framework, or a blanket
warnings-as-errors policy solely to enforce it; one user and one collaborator do
not need that machinery yet.

## What is appropriately unconventional

Several choices might look non-idiomatic in isolation but fit the charter and
should not be “normalized” away:

- **Concrete enums and exhaustive matches.** This is idiomatic Rust and makes
  the runtime state space visible. The placeholder future `Value` variants are a
  small, justified exception to YAGNI because enum layout and serialization are
  explicit project properties.
- **A character-driven Lisp reader without a lexer framework.** This is the
  conventional small-Lisp path and preserves future reader dispatch better than
  a generated token layer.
- **Forms as values.** Well-trodden Lisp design, and it avoids a conversion
  boundary that would be especially awkward for macros and snapshots.
- **An explicit frame stack and register-like bytecode.** Neither is exotic;
  both follow established interpreter architecture and directly serve
  suspension, stack safety, and performance.
- **No lazy sequences.** This is a semantic deviation from Clojure, but it is a
  coherent simplification, clearly disclosed, with an alternative composition
  model planned.
- **No dependency for CLI parsing, errors, or snapshots yet.** The present CLI
  does not justify `clap`, and reader errors do not justify `thiserror`/`miette`.
  Add standardized machinery only at the milestone where it replaces real work.
- **Insertion-ordered vector-backed maps for the reader.** As a provisional
  syntax representation pending Q6, this is simpler and more deterministic than
  selecting a persistent map prematurely.
- **Release overflow checks while semantics are open.** This is a small and
  effective way to keep debug/release behavior aligned until Q10 is resolved.

The snapshot-ready `Vm`/`Execution`/`Image` design is the project's meaningful
deviation from an ordinary toy interpreter. ADR-029 has already narrowed it to a
credible first promise: same-build, fresh-VM, fuel-boundary resume, no live
handles. Keep that narrowness. Do not let “async and migration reuse the shape”
turn into a scheduler, resource-reacquisition protocol, or stable file format
before a workload asks for one.

## What should not be added

The review found no need for:

- a Cargo workspace or multi-crate architecture;
- service/repository layers, dependency injection, or trait abstractions around
  single implementations;
- a plugin system, stable public API, configuration framework, or feature matrix
  for language semantics;
- a parser generator or generic AST framework;
- a general mutation-testing platform;
- mandatory coverage targets, benchmark gates, or a large CI matrix;
- cross-build snapshot compatibility, live-handle migration, or an async
  runtime in v1.

These would be actual over-adherence to generalized best practices. The project
has avoided them so far and should keep doing so.

## Recommended sequence

1. Split the library behavior from the CLI driver, then make reader/printer
   properties compare actual values.
2. Decide/reject non-finite numeric literals for now and fix the Unicode escape
   panic; add direct regression cases.
3. Redefine smoke as the green end-to-end capability of the current milestone;
   make `verify` and BUILD agree with it.
4. Fix README/BUILD/`just bless`, target-path handling, and formatting.
5. Simplify the current-decision reading path before recording many more future
   decisions.
6. As corpus size grows, snapshot selected diagnostic cases per phase rather
   than every phase for every program.

After those changes, milestone 2 should proceed on the current architectural
course. The project does not need a redesign. It needs a slightly more ordinary
Rust test seam and slightly less constitutional machinery around an otherwise
well-proportioned small runtime.

## Verification performed for this review

- `cargo test`: passed, 10 integration tests.
- `cargo test --release`: passed, 10 integration tests.
- `CARGO_TARGET_DIR=/tmp/apolisp-scaffolding-review-target cargo test`: passed,
  while exposing that the tests still execute the repository-local binary.
- `cargo fmt --all -- --check`: failed with formatting diffs.
- `cargo clippy --all-targets --all-features -- -D warnings`: failed on
  `len_without_is_empty` for the unused `Interner::len` method.
- `./smoke.sh`: failed at the intentionally unimplemented `expand` command.
- Manual reader probes confirmed the infinity type change and Unicode escape
  panic described above.
