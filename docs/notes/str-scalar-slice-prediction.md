# `str-scalar-slice`, predicted before it is written

Pre-registration for Q34, in the habit `BUILD.md` asks for. Written before the
primitive exists, so the scoring afterwards is not a story assembled around
whatever the numbers turned out to be. The previous round of this
(`the-editor-prediction.md`) was wrong about the mechanism, which is the reason
the habit is worth the five minutes.

## What is being built

`(str-scalar-slice s from to)` — the substring between two **scalar** indices,
raising when `from > to` or `to` exceeds the scalar length. One `char_indices`
pass, no index structure built or cached. **O(n) in the string, not O(1).**

It replaces this idiom, which is what the editor has today:

```
(defn scalars-take [n s] (scalars-str (take n (str-scalars s))))
(defn scalars-drop [n s] (scalars-str (drop n (str-scalars s))))
```

`take` and `drop` are prelude `conj` loops, so that idiom is quadratic in the
column (Q6/E-11). The claim under test is that a linear native scan is enough to
close the gap, and that O(1) was never the thing that mattered.

## Claims

**1. Per-keystroke cost becomes near-flat but not flat.** The byte-indexed
`str-slice` measured ~4 µs flat because it does no scanning. This one scans, so a
linear term exists. I expect VM call overhead to dominate it across the range the
editor cares about:

| line length | predicted µs/keystroke |
|---:|---:|
| 100 | ~4 |
| 400 | ~4–5 |
| 1,600 | ~5–8 |

So a **1.3–2× rise from 100 to 1,600 characters**, against the 46× rise the
current scalar path shows over the same span. Speedup at 1,600 characters:
**1,500–2,500×**, somewhat below the 2,905× the byte version showed, and the
shortfall is the scan.

**2. The linear term is real and will show if pushed far enough.** At 25,000
characters the scan should be unmistakable — I expect ≥20 µs, i.e. clearly above
the ~4 µs floor. **If it does not show, I have measured call overhead and should
not conclude the operation is flat.** This claim exists because the failure mode
of claim 1 is a flattering one.

**3. The editor's core gets smaller.** `scalars-take` and `scalars-drop` stop
existing as compositions and become direct calls, and `clip` with them. I expect
the core to lose 2–4 lines and the `.disasm` golden to lose two protos.

**4. It will not fix `RET`.** `split-line` rebuilds the line vector with
`concat`, which is O(buffer) and has nothing to do with strings. Enter at 1,000
lines should stay at ~13 ms. Stated because the temptation after a large speedup
is to assume the program is fast now.

**5. Core cost: 25–35 lines**, landing near 6,590 of 7,500.

## What would refute the entry rather than a number

If per-keystroke cost at 1,600 characters comes out **above ~50 µs**, the linear
scan is not cheap enough and `str-scalar-offset` composed with a cached offset —
or fixing Q6/E-11 outright — is the better shape after all. That is the outcome
that says the ADR chose wrong, as distinct from the predictions above being
miscalibrated.

## Scoring — 2026-07-29, after the fact

**The decision was right and three of five predictions were wrong.** Measured
release, length-preserving edit at the middle of the line:

| line | old idiom | `str-scalar-slice` | speedup |
|---:|---:|---:|---:|
| 100 | 93.6 µs | 0.92 µs | 102× |
| 400 | 955.5 µs | 1.74 µs | 550× |
| 1,600 | 9,683.8 µs | 4.54 µs | 2,134× |
| 6,400 | 159,639.8 µs | 10.85 µs | 14,707× |
| 25,000 | 2,213,853.5 µs | 43.58 µs | 50,802× |
| 100,000 | — | 165.68 µs | — |

**Claim 1 — near-flat, ~4 µs, a 1.3–2× rise from 100 to 1,600. Headline held,
shape refuted.** The predicted speedup band at 1,600 characters was 1,500–2,500×
and the answer is 2,134×, inside it. Everything about *why* was wrong. I predicted
a ~4 µs floor from the previous session's byte-slice measurement and expected it to
swamp the scan; the real floor is ~0.5 µs, so the linear term is visible almost
immediately and the rise from 100 to 1,600 is **4.9×**, not 1.3–2×. The absolute
numbers are four times better than predicted and the curve is four times worse.
Being right about the ratio while wrong about both terms is worth noticing.

**Claim 2 — the linear term shows by 25,000 characters, ≥20 µs. Held.** 43.58 µs,
and the 100,000-character point settles it: 4× the characters costs 3.80×, with
the two largest steps at 4.01× and 3.80×. Linear, measurably.

This claim existed because claim 1's failure mode was a flattering one, and that
is exactly how it paid: the small-string numbers on their own look flat, and had I
stopped at 1,600 I would have written "flat" into an ADR that says linear.

**Claim 3 — the editor core loses 2–4 lines and two protos. Half held.** Two
protos exactly. But the core lost **one** line, not 2–4: the two definitions went
away and a six-line comment explaining why took their place. Deleting code and
adding prose is not what "smaller" predicted.

**Claim 4 — `RET` unchanged at ~13 ms. Refuted.** It nearly halved, 13.228 → 7.244
ms at 1,000 lines, because `split-line` slices the line as well as rebuilding the
vector and I had only accounted for the rebuild. The shape held — still O(buffer),
4× the buffer still costing 10.2× — but the number moved.

The interesting consequence is one I did not predict at all: typing got 20× faster
and `RET` 1.8×, so **the gap between them widened from ~60× to ~660×**. Fixing the
cheap operation is what made the expensive one conspicuous. Filed as Q36.

**Claim 5 — 25–35 core lines. Refuted, by 20 lines.** 6,561 → 6,616 is **55**, and
the overrun is entirely comment. ADR-030 counts comments deliberately, so this is
a real cost. Worth stating as a rule for next time: on a change whose *point* is a
subtle invariant, the explanation is the bulk of the diff, and estimating the code
is not estimating the change.

**The refutation threshold was not met.** Above ~50 µs at 1,600 characters would
have said the ADR chose wrong; the answer is 4.54 µs. `str-scalar-slice` over
`str-scalar-offset` stands, and nothing measured argues for the offset.

## The part that was not a prediction at all

**The benchmark took three attempts, and both failures were the fixture rather
than the operation.** Attempt one built the line with `(join "" (repeat L "x"))`,
which is quadratic, so at 25,000 characters I was timing the setup and the "new"
path looked worse than quadratic. Attempt two fixed the fixture with doubling but
inserted 4,000 characters into a 100-character line, so the line was 4,100
characters by the end and the reported per-edit cost was an average over a length
that had grown 41×. Attempt three made the edit length-preserving — replace rather
than insert, same two slices, same concatenation — and the numbers became
monotonic and coherent.

`the-editor-prediction.md` records the same class of error from the previous
session ("`(range n)` is itself the quadratic thing I measured in step 1"). Twice
now, in a language where every collection operation is quadratic, **the fixture has
been the confound.** The rule that would have caught both on the first attempt: in
a language with no cheap sequence construction, measure a zero-iteration baseline
per size and require the per-operation cost to be monotonic in the size before
believing any of it.
