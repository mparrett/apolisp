# Milestone 1 as a pilot — process notes

**Branch:** `milestone-1-reader` · **Code:** `8149c8e`, `26a019e`
**Not normative.** Working notes for a meta-assessment of the loop, not of the
reader. SPEC v0.1's Phase 3 framing applies: the pilot tests the *loop*, not the
items — are the docs sufficient, do the checks catch anything, does the commit
hygiene hold.

## What got built

901 lines of core (budget 5,300), one file, inline `mod` blocks. Reader,
printer, span origins, four debug/driver commands. Ten tests green in debug and
release. Four `.xs` corpus programs with `.forms` and `.spans` goldens.

Elapsed: roughly one working session for the code. The documentation phase that
preceded it ran across several days and produced 29 ADRs, 9 errata, and ~2,000
lines of prose. **That ratio is the first thing worth assessing.** It is not
obviously wrong — the docs are sized for the whole project rather than for
milestone 1 — but nothing yet proves the prose is load-bearing rather than
elaborate.

## Where the docs paid for themselves

- **ADR-025's exact `Value` enum.** Zero deliberation. Typed it in, moved on.
  This is the clearest case of an ADR doing its job: the decision was made once,
  in the right context, and cost nothing at the point of use.
- **ADR-015's "one file, inline mods."** No time spent on layout.
- **`TRAPS.md` on derived equality.** `PartialEq` on `Value` is hand-written
  with a comment pointing at the trap. Without the entry it would have been
  `#[derive(PartialEq)]` and wrong in a way nothing tests yet.
- **`BUILD.md`'s "write `smoke.sh` before the reader is finished."** Writing it
  first fixed the CLI surface before any of it existed, and its failure at
  `expand` is a real queue item rather than a stub.
- **Determinism as a stated prerequisite.** Two places sort or order explicitly
  (corpus file listing, map insertion order) with the reason in a comment. Both
  would otherwise have been nondeterministic and both reach golden output.

## Where the docs were wrong, and how it surfaced

Two errata came out of *building*, not reviewing — neither was findable by
reading:

- **E-8:** `Value` is 16 bytes, not the predicted 24. ADR-010 assumed `Rc<str>`
  and `Rc<[T]>` (fat pointers); `Rc<StrObj>` is thin.
- **E-9:** ADR-026's `LocatedForm { root, origin }` sketch is not implementable
  alongside its own "one origin per syntactic child." The carrier has to be a
  tree.

**Both were pseudo-code in an ADR.** That is a pattern worth a rule: the prose
in an ADR survived two reviews, and the code blocks did not survive first
contact. Either mark ADR code as illustrative, or stop putting it in.

## The finding that matters most

I ran the Q18 mutation check against my own span tests, with predictions
written first.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — every leaf origin becomes `Unknown` | passes (test is dead) | **passed** |
| M2 — every span starts at byte 0 | passes (test is dead) | **passed** |
| M3 — drop one child origin | fails (arity catches it) | failed |

The span-invariants property was checking **arity only**. Spans could be
systematically, totally wrong and the entire suite stayed green.

This is the `../reg-lisp` failure from `PRIOR-ART.md` — "a mutant that never
restored the compiler's line counter passed the whole suite" — reproduced, by
me, in the first milestone, *after* writing it into the prior-art document as a
thing to watch for. Knowing about a failure mode did not prevent it.

Worse: ADR-026 already specified the fix. Its verification list, point 3, says
`.forms` and `.expanded` snapshots should render origins in a debug mode. I
built the debug mode and did not snapshot it. The ADR was right and the
implementation was short of it, and nothing in the loop noticed except an
explicit attempt to break the test.

Fixed in this commit: `.spans` goldens per corpus file. M1 and M2 now both fail.

**The meta-question for the team:** an explicit mutation check found this in
about ten minutes. Nothing else would have — not the property test, not the
review, not the corpus. Should it be a rung rather than a Q?

## Known weaknesses a reviewer should push on

1. **Properties are tested through the binary, not the library.** There is no
   `lib.rs`, so `read(print(read(s))) == read(s)` is checked as *printed strings
   being equal*, which is a proxy for value equality, not value equality. The
   hand-written `PartialEq` on `Value` is therefore **untested**.
2. **Origins are produced and never consumed.** Nothing reads a span except the
   debug printer. Real pressure arrives at milestone 4 (errors). Until then the
   goldens pin whatever the reader currently does, correct or not — I checked
   them by eye, which is exactly the review-gated rule working as intended, and
   also exactly as strong as my eye.
3. **`Generated` and `Unknown` are unreachable.** No macro exists, so two thirds
   of `SpanOrigin` is dead code with no test. The interesting half of ADR-026 is
   unexercised.
4. **Quote-sugar span attribution is a judgment call not in any ADR.** `'x`
   synthesizes a `quote` symbol; I gave it the span of the `'` character.
   Defensible, undocumented, and it will interact with macro attribution later.
5. **Provisional decisions are comments, not tracked.** Five places resolve an
   open question locally with a `Q6`/`Q13`/`Q20` comment. Nothing fails when
   the question is later answered differently.
6. **The line-budget test is not yet informative.** 901 against 5,300 passes
   trivially; it will not say anything until roughly milestone 4.
7. **Integer literals too large for `i64` are a read error.** Clojure promotes.
   This is a Q20 call I made in code with a comment rather than in a decision.

## Process observations

- **Committing docs and code separately worked.** Errata landed in their own
  commit with the reasoning; the code commit is readable on its own.
- **The pre-registration habit (`BUILD.md`) cost nothing and paid twice.** The
  `Value`-size prediction was wrong and got recorded; the mutation predictions
  were right and made the result trustworthy rather than a fishing expedition.
- **One self-inflicted flake:** the test harness keyed temp files on pid alone,
  and cargo's parallel tests clobbered each other. Four tests failed for reasons
  unrelated to the reader. Diagnosed in one pass, but it is the kind of thing
  that would read as a reader bug to anyone who did not write the harness.
- **Golden blessing stayed manual.** `just bless` exists and is deliberately
  outside `just test`. I read all four `.forms` files before committing them.
  The `.spans` files are larger and I read them less carefully — that is an
  honest weak point in the same rule.

## Questions for the meta-assessment

1. Is the doc-to-code ratio right, or did we over-invest in prose that the
   implementation is now going to keep correcting?
2. Should mutation checks be promoted from Q18 to a standing oracle rung, given
   that one caught a dead test in the first milestone?
3. Should ADRs stop containing code sketches, given both errata came from them?
4. Is a `lib.rs` worth it now, so properties test values instead of strings?
5. Milestone 1's exit condition was "round-trip + span-invariants pass." Both
   passed while one of them was dead. Do exit conditions need to name the
   mutant that must fail, not just the test that must pass?
