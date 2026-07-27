# Milestone 2 — mutation check on the compiler

**Code:** `ee027a4` · **Not normative.** The record of a Q18 mutation pass over
the new load-bearing lines, with predictions written before running.

Milestone 1's pilot found the span property was dead only because someone tried
to break it. The habit carried forward: four mutants, predictions first.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — `emit` records `Unknown` instead of the node's origin | fails (3 tests) | **failed** (3) |
| M2 — a call keeps its last argument's origin instead of its own | fails, but only via the subexpression test and the goldens | **failed** (2 — those two) |
| M3 — the `Call` arm drops `self.regions == 0`, so a tail call can escape a handler region | fails (the ADR-028 rule 2 test) | **passed — the whole suite stayed green** |
| M4 — `lookup` stops adding captures at the levels it crosses | fails (the capture-chain test) | **failed** (3) |

M2 is `../reg-lisp`'s surviving mutant aimed at our own code — the one that
"never restored the compiler's line counter" and passed that project's entire
suite because every test program had its subexpressions on the same line as the
enclosing form. It died here, and it died on exactly the two tests predicted:
`instruction_origins_track_subexpressions_not_the_enclosing_form`, whose program
puts every subexpression on its own line, and the `.disasm` goldens. The
`every_instruction_has_an_origin` test passed under M2, as predicted — an origin
that is present and wrong is not what it checks.

## The refutation

**M3 survived, and the reason is the interesting part.** ADR-028 rule 2 was
enforced twice: `try_form` cleared the `tail` flag before descending into a
protected body, *and* the `Call` arm checked `self.regions == 0` before emitting
a `TailCall`. Two mechanisms for one rule. Because the flag was already false by
the time a call was reached, the counter could never fire — it was dead code
that read as a safety net, and the test could not distinguish the two.

The test was not weak in the obvious way. It asserted both directions — that a
pending `finally` suppresses the tail call and that closing the region restores
it — and both assertions were true under the mutant, because the *other*
mechanism was carrying them.

Fixed by deleting the redundancy rather than by adding a test: `try_form` now
passes `tail` through untouched and the counter is the single enforcement point,
at the site where the decision is actually made. M3 now fails. No golden changed,
which is the confirmation that the removed clause was genuinely redundant — the
emitted code is byte-identical.

**The generalizable finding:** a mutation check does not only find dead tests. It
finds *duplicated enforcement*, where two mechanisms implement one rule and the
suite cannot tell which one is doing the work. Deleting one is the fix; adding a
test that pins both would have preserved the ambiguity.

It is also the second milestone in a row where the mutation check found something
no property, corpus, or review pass did. That is the evidence Q18 was waiting
for.

## Second pass — after a fresh-context review

A diff-only review by a fresh context (`BUILD.md`, "On process") ran its own
mutations and found **seven more survivors**, all in correct code that nothing
observed. The pattern is worth naming: every one of them was a *decision that
had a home in an ADR and no test*, so the ADR read as enforced when only the
code was carrying it.

| Mutant | Before | After |
|---|---|---|
| R1 — `tail` passed through to the operator position, so `((f) 1)` returns `(f)` instead of calling it | passed | fails |
| R2 — `let` declares before resolving its own initializer, i.e. becomes `letrec` | passed | fails |
| R3 — the block scope walk is not reversed, so an outer binding beats an inner one | passed | fails |
| R4 — a `fn`'s own name is checked before its parameters, so `(fn f [f] f)` returns the function | passed | fails |
| R5 — the rest parameter is allowed to be named `&` | passed | fails |
| R6 — `if`'s then-branch is not a tail position | passed | fails |
| R7 — a `do`/`let`/`fn` body's last expression is not a tail position | passed | fails |

R1 is the one to remember. It is a single token — `false` becoming `tail` on one
argument — and it produces a `TAILCALL` on a computed callee, which returns the
operator's value and never performs the call. Every test passed and every golden
was byte-identical, because no program in the corpus had a call whose operator
was itself a call.

**What made these invisible was corpus shape, not test design.** The tail-call
test existed and asserted both directions; it only ever exercised a call in
plain operator position. The lesson generalizes past mutation: a corpus assembled
to look representative is not the same as one assembled to distinguish. A program
belongs in the corpus because some plausible wrong implementation compiles it
differently.

The review also found two dead things of the M3 kind — the `src != dst` guard
before `MOVE`, which cannot fire and left the goldens byte-identical when
deleted, and a `RETURN` attributed to the first form in the file rather than the
expression it returns — plus one test that panicked out of bounds while building
its own failure message, so the only test written to be readable on failure was
the one that could not be.

## Method, so it can be repeated

Commit first, then mutate, then `git checkout -- src/lib.rs`. The one mistake
made here was making a *fix* while a mutant was still applied — the checkout that
reverted the mutant reverted the fix with it. Mutate against a clean tree only.
