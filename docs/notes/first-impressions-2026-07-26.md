# First impressions — a fresh reader on the repo, 2026-07-26

**Not normative.** What the project looks like to a collaborator arriving after
milestone 1, before reading any history. Recorded because a first impression is
only available once, and the thing it measures — whether the docs orient someone
who wasn't there — stops being measurable as soon as the context is absorbed.

Read at `83f49ae`.

## The shape

A design corpus with one milestone of code hanging off it. Roughly 2,000 lines
of normative prose against 901 lines of Rust, and the prose came first.

**Docs.** `ETHOS.md` states four constraints and a priority order (line count →
serializable state → speed → ergonomics). `ADR.md` holds 30 entries plus errata,
scoped to the whole project rather than to what exists — values, refcounting,
bytecode shape, handles, snapshots, tail calls, hygiene are all settled on paper
and mostly unbuilt. `BUILD.md` carries the ~7,000-line core budget across seven
layers, ten milestones, a four-rung oracle, and three property tests.
`TRAPS.md`, `QUESTIONS.md`, `GLOSSARY.md`, and `PRIOR-ART.md` are consulted
rather than read through.

**Code.** One file, four inline `mod` blocks, which are the seams ADR-015 asked
for:

| mod | what is there |
|---|---|
| `error` | `Span` as byte offsets, `SpanOrigin` (`Source`/`Generated`/`Unknown`), `LispErr` rendering line and column on demand |
| `value` | the full v1 `Value` enum — 15 variants, size-asserted, `PartialEq` hand-written — plus interner, `Origins` tree, and the invariant checker |
| `reader` | character-driven, no tokenizer; lists, vectors, maps, strings, keywords, atoms, quote sugar, with a span on every node |
| `printer` | round-trip-safe, including float printing that reads back as a float |

**Driver.** `read`, `spans`, and `sizes` work. `expand`, `compile`, and `run`
exist and fail loudly. `smoke.sh` runs all four stages and stops at `expand`,
which is the queue rather than a defect.

**Oracle.** Ten tests green. Rung 1 and rung 3 are live; rung 2 fails by design.
The corpus is four programs — `hello`, `atoms`, `aggregates`, `quote` — with two
of the eventual four goldens each. `.expanded`, `.disasm`, and `.out` arrive
with the milestones that can produce them.

## What reads as unusual, and looks deliberate

- **The `Value` enum is complete before the VM exists.** `Cell`, `Handle`, and
  `Buffer` are unreachable variants carried so `size_of::<Value>()` is asserted
  against the real type rather than against a placeholder that will grow.
- **Unimplemented stages fail instead of no-opping.** The distinction between a
  stage that is absent and a stage that silently passes is the difference
  between having an oracle and believing you have one.
- **The open questions are visible from inside the code.** Five sites resolve a
  `Q6`/`Q13`/`Q20` question locally, in a comment, with the reasoning for
  choosing the option that is safe to widen later. Whether that survives the
  question being answered differently is the pilot note's own item 5.
- **Equality is narrow on purpose.** No cross-type sequential equality, no
  int-float comparison. Widening later is safe; narrowing is not.

## Two places the repo has drifted from itself

1. `README.md` line 7 still says "No code yet. What exists is the design."
2. `just bless` regenerates `.forms` only. A `.spans` golden has to be
   hand-edited, which puts the most friction on the file the pilot note already
   names as the one read least carefully (`milestone-1-pilot.md`, process
   observations). The review-gated rule survives either way, but the ergonomics
   point the wrong direction.

## The thing worth watching

`milestone-1-pilot.md` records that the span-invariants property was checking
arity only, and that two mutants — every origin `Unknown`, every span starting
at zero — passed the whole suite. That was found by an explicit attempt to break
the test, not by the test, the review, or the corpus.

From outside, the interesting part is not the dead test. It is that
`PRIOR-ART.md` had already written down this exact failure from `../reg-lisp`,
and it happened anyway, in the first milestone, to the person who wrote it down.
That is evidence about what documents can and cannot do, and it points at the
pilot note's question 2 — mutation checks as a rung rather than a question —
being the highest-value item currently open.

## Where the budget stands

901 of ~7,000 lines, against a 900-line target for the reader-printer-forms
layer. The layer is at its number and the total is 13% spent, which says nothing
yet; per the pilot note it will not say anything until roughly milestone 4.

Next is milestone 2: core AST, slot compiler, disassembler, with `.disasm`
goldens as the exit condition.
