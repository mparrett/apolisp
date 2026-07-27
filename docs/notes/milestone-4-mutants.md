# Milestone 4 — mutation check on the handler stack

**Code:** `aa7a20b`, fixed in `0590e46` · **Not normative.** A Q18 mutation pass
over milestone 4's load-bearing lines, predictions written before running.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — `unwind` does not drop the frames above the handler's owner | fails (the cross-frames test, the `errors.xs` transcript) | **failed** (3, including the flat-loop test) |
| M2 — `ENDFINALLY` pops the parked unwind and carries on instead of re-raising | fails (the suppression test's second case, `errors.xs`) | **failed** (3) |
| M3 — `POPHANDLER` is a no-op | fails everywhere, via the end-of-run assertion | **failed** (5) |
| M4 — a displaced parked unwind is not merged into the new one | fails, but only *indirectly* | **failed** (2), indirectly, exactly as predicted |
| M5 — unwinding drops the frames but keeps their slots | **survives** | **passed — the whole suite stayed green** |

M4 is worth the row it got. The prediction said no test names the suppression
rule for the *caught* case — the direct test only covers the uncaught path,
which is drained by a different branch — so it would be caught by the end-of-run
assertion tripping on a record left parked. That is what happened, in
`cleanup_runs_exactly_once_on_every_path` and the `errors.xs` golden. The
assertion is doing work no assertion about suppression is doing.

## The refutation

**M5 survived, as predicted, and the test written to catch it was dead too.**

The first response was a test: a tail loop that throws four frames deep and
catches, two hundred times, comparing the high-water marks against ten
iterations. It passed under the mutant. The reason is that **the leak is
bounded**: a call's window sits inside its caller's frame, so the next call
reuses the same slot range and never grows past the deepest point already
reached. No high-water mark can move. No value is ever wrong either, because the
callee prologue nil-fills everything above its arguments — a line that exists
for ADR-029's reason, and that turns out to be what makes this leak invisible.

So the mutant is real (dead values retained for the life of the run, and carried
into an `Image`) and unobservable from outside the VM. There is no cheap test to
add. Adding one anyway would have produced a second dead test, which is the
milestone-1 failure repeated on purpose.

The fix is structural, and it is milestone 2's finding in a new place: make it
one mechanism instead of two that have to agree. `drop_frame` is now the only
place a frame is released, called by both returning and unwinding, so "unwinding
forgets to give the slots back" is no longer expressible as a local change —
writing it means breaking returning too, and that is caught by eight tests.

**The contrast with milestone 3's M6 is the useful part.** That mutant —
*returning truncates the slot stack to the callee's base* — failed two tests,
because it discards the caller's own slots and produces wrong values. This one
truncates nothing. Same line, two directions: the direction that corrupts is
covered, the direction that merely retains is not covered by anything, and was
not covered for milestone 3 either. Nobody noticed because the mutant nobody
wrote is the one that survives.

## What the oracle still cannot see

Slot *release* has no observable outside the VM. Peaks are monotone, so they
measure the ceiling and never the floor; values are nil-filled, so retention
cannot corrupt one. The first place this becomes visible is milestone 8: an
`Image` carries the slot vector, so a snapshot taken after a caught throw would
be larger than one taken at the same point without it — and ADR-029's whole
claim is that machine state is a function of the computation. Worth checking
there rather than instrumenting for it now.

## Method

Commit first, then mutate, then `git checkout -- src/lib.rs`. Milestone 2's note
warns about making a *fix* while a mutant is applied; the same checkout ate an
uncommitted refactor here, one pass later, for the same reason. The scar is not
about fixes specifically — it is that `git checkout` does not know which of your
changes was the mutant.
