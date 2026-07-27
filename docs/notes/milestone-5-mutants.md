# Milestone 5 — mutation check on the expander

**Code:** `6c4785f`, fixed in `95c5dad` · **Not normative.** A Q18 mutation pass
over milestone 5's load-bearing lines, predictions written before running.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — the expander walks into `quote` | fails (the quote test, `macros.*`) | **failed** (4) |
| M2 — auto-gensym returns the written name, dropping the `#` | fails (the gensym test, `.expanded`) | **failed** (2), as predicted |
| M3 — a splice drops the items after it | fails (template test, `.expanded`, `.out`) | **failed (1)** — only the unit test |
| M4 — macro output is `Generated` everywhere, never reusing a passed-through argument's origins | fails, and **no golden can see it** | **failed** (2) — including a golden |
| M5 — `expand_all` does not reset the gensym counter | fails (the determinism test alone) | **failed** (1), as predicted |
| M6 — a macro receives *expanded* arguments | (added mid-pass) | **passed — all 75 tests stayed green** |

## Two refutations, in opposite directions

**M3 refuted the corpus, not the code.** `macros.xs` carried a comment saying it
had "a splice, and a tail the splice does not swallow". It did not. Every splice
in the file was the last item of its template, so an expander that dropped
everything *after* a splice expanded the entire corpus correctly and only the
unit test noticed. The comment was written first and never checked against a
wrong implementation — which is precisely the milestone-2 finding, one milestone
later: **a corpus program earns its place by distinguishing, and a comment
claiming it does is not the same as it doing so.**

**M4 refuted the prediction in the useful direction.** The prediction said no
golden could see macro-output origins, because origins are invisible in printed
forms — the milestone-1 shape. That was wrong: `.disasm` prints a source
position per instruction, and those positions come from the origins the expander
attached. So the disassembly golden *is* an origin oracle for expanded code,
which is a stronger position than milestone 1 was in and worth knowing before
someone decides `.expanded` needs a spans variant.

## The survivor

**M6 — a macro receives expanded arguments instead of the forms as written —
survived the entire suite.** It is the most basic rule macros have, and nothing
observed it.

The reason is that every macro in the suite *used* its arguments as code, and
for those, expanding early and expanding late produce the same answer. The
difference only appears when a macro keeps an argument as **data**: `as-data`
hands its argument back inside a `quote`, so what the argument was when it
arrived becomes part of the output. With that in the corpus and one unit test,
M6 dies on four tests.

This is R1's shape from milestone 2 — a rule everybody knows, enforced by code
nothing observes — and it is the third pass in a row where the mutation check
found something no property, corpus, or review pass did.

## Method

Commit first, then mutate, then `git checkout -- src/lib.rs`.

New this pass: **`cargo test` stops at the first failing test *binary***, so a
mutation pass that reads the failure list gets a truncated one. The first run of
M1 and M4 looked like they failed a single golden test each; both actually
failed tests in a later binary that never ran. Use `cargo test --no-fail-fast`
when counting what a mutant kills — otherwise the pass systematically
under-reports coverage, which biases exactly the judgement it exists to inform.
