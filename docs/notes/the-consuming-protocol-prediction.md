# The consuming protocol, pre-registered

*Written 2026-08-02, before any of ADR-061 is built. Scored in
`the-consuming-protocol.md`.*

ADR-021 removed the gate on optimisation work and BUILD.md's habit is what
replaces it: write the number down first, because a performance change with no
prior claim gets reported as a success whatever it does. This is the largest
performance change the project has attempted, so it gets the most specific
claims it can carry.

## The baseline, measured in release before any change

`examples/xgrep.xs` over a directory holding one file of *n* lines, one match,
best of three after a warm run, on the machine this was measured on.

| lines | time | ns per line² |
|---:|---:|---:|
| 2,000 | 0.02 s | 5.00 |
| 4,000 | 0.07 s | 4.38 |
| 8,000 | 0.29 s | 4.53 |
| 16,000 | 1.20 s | 4.69 |
| 32,000 | 4.84 s | 4.73 |
| 64,000 | 19.09 s | 4.66 |

Ratio 4.0× per doubling. Anyone re-running this should re-measure the baseline
rather than trusting the table: the absolute figures are one machine's.

## C1 — the ratio halves

**`split` goes from 4.0× per doubling to 2.0×**, measured over the same 2,000
to 64,000 range. That is the whole claim: a `conj` that mutates in place makes
`split` linear in the file, so doubling the input doubles the work.

**Refuted if** the ratio at the top of the range is above **2.5×**. Between 2.0×
and 2.5× the fix worked and something else is quadratic underneath it, which is
a finding rather than a failure and should be chased rather than rounded down.

## C2 — the absolute number, which is the one people will feel

**64,000 lines goes from 19.09 s to under 0.1 s.** That is the ~190× a
quadratic-to-linear change buys at this size, minus whatever the constant costs.

**Refuted if** it is above 0.5 s. Between 0.1 s and 0.5 s means linear with a
bad constant — check whether `MoveKill` is firing at all before concluding the
design is wrong.

## C3 — `map` moves too, and that is the point

`map` is the reason ADR-061 was preferred over making `split` native, so it has
to be scored separately or that argument was never tested. The editor measured
**3.87×** per doubling, 8,000 elements at 272 ms.

**`map` goes to 2.0× per doubling, and 8,000 elements under 20 ms.**

**Refuted if** `map` does not move while `split` does. That result would mean
the fix reaches primitives called on a temporary and not accumulators live
across a bytecode call — which is exactly the case a language closure creates,
and would make the native-`split` option the better one after all.

## C4 — the refcount reaches 1

The mechanism, checked directly rather than inferred from the timings, by the
same `Rc::strong_count` probe E-18 used.

**At `make_mut` inside `conj`, in the loop program, the count is 1.**

**Refuted if** it is 2 anywhere in the loop. This is the claim that says *why*
any speedup happened, and without it a good timing could be some other effect —
an allocator getting luckier, a branch predicting better. E-13 is the precedent:
the win it claimed was real in theory and had never once occurred.

## C5 — nothing else moves

**The `.out` transcripts do not change.** Every `.disasm` golden changes and no
`.out` does. A behavioural diff here is a bug in the liveness analysis, and the
most likely one is a slot cleared while a handler could still re-enter and read
it.

**Refuted if** any `.out` moves. That is not a refutation to work around; it is
the failure mode ADR-061 names as dangerous, and it produces wrong answers
rather than crashes.

## What I expect to go wrong

Recorded so that "we knew that" is checkable afterwards.

1. **The handler-region liveness**, in one direction or the other: either too
   conservative, so `MoveKill` almost never fires and C1 is refuted with the
   code working as designed; or too aggressive, so C5 fails. The first is the
   more likely and the harder to notice, because everything stays green.
2. **`MoveKill` firing far less often than expected.** The interesting number to
   print during the work is what fraction of `Move`s became `MoveKill`s. If it
   is small, C1 fails for a reason that has nothing to do with the protocol.
3. **The `.disasm` diff being too large to review honestly.** Every golden moves
   at once, and the failure is social rather than technical: a diff nobody can
   read gets approved. Mitigation is landing it as one commit that does nothing
   else, so the diff is one substitution repeated — and *checking that claim* by
   grepping the diff for any hunk that is not a `Move` → `MoveKill`.

## What would make me abandon it

If C1 lands but C3 does not — `split` linear, `map` still quadratic — then
ADR-061 bought the case that native `split` and `join` would have bought for a
fraction of the cost and a fraction of the blast radius. At that point the
honest move is to supersede ADR-061 rather than to keep it for the part that
worked.
