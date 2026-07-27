# Milestone 3 — mutation check on the VM

**Code:** `8ff2c9f` · **Not normative.** A Q18 mutation pass over the VM's
load-bearing lines, predictions written before running.

Seven mutants, predictions first. All seven died; nothing survived.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — a tail call pushes a frame instead of reusing one | fails | failed (8 tests) |
| M2 — the callee prologue's arity check deleted | fails | failed |
| M3 — an empty rest parameter becomes `nil` instead of `()` | fails | failed (2) |
| M4 — `0` is falsy | fails | failed |
| M5 — addition wraps instead of checking | fails | failed |
| M6 — returning truncates the slot stack to the callee's base | fails | failed (2) |
| M7 — `GetSelf` yields `nil` rather than the running closure | fails | failed (4) |

A pass with no survivors is worth two lines rather than none: the interesting
result is the refutation, and "no refutation" is only meaningful if the
predictions were written down first.

M6 is the one with history. It is the bug that actually shipped into the working
tree during milestone 3 — returning restored the wrong slot-stack length — and it
was found by a program crashing, not by a test. It now fails two tests, so the
same slip cannot come back silently. That is the useful pattern: when a bug is
found by hand, turn it into a mutant and check that the fix is *observed*, not
merely present.

**Read M6 next to milestone 4's M5** (`milestone-4-mutants.md`), which mutates
the same line in the other direction. Truncating to the *wrong* length corrupts
a value and dies on two tests; not truncating at all merely retains dead slots,
and nothing in the suite can see it. A clean pass means the mutants that were
written all died — not that the line is covered.
