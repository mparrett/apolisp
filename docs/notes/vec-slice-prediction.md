# `vec-slice`, predicted before it is written

Pre-registration for Q36. The previous round of this
(`str-scalar-slice-prediction.md`) got the decision right and three of five
numbers wrong, and the one claim that paid for the whole exercise was the one
written specifically to catch a flattering result. Same structure here.

## What is being built

`(vec-slice v from to)` — the elements between two indices, half-open, as a
vector. Accepts a list or a vector because `take`/`drop` do today and `concat`
returns a list. Raises when `from > to` or `to` exceeds the count. One copy,
O(n).

Then `take` and `drop` stop being `conj` loops and become clamping wrappers over
it, and the editor's `split-line`/`join-prev` stop paying Q6/E-11 for what is
structurally one copy.

The choice under test is the one taken over `splice`: a general slice composed at
the call site, against a single-allocation primitive shaped for exactly two
callers. `split-line` will allocate four times where `splice` would allocate once.
**The bet is that four linear copies beat one linear copy by a constant that does
not matter, and that fixing `take`/`drop` for every caller is worth more.**

## Claims

**1. `RET` at 1,000 lines drops from 7.244 ms to ~0.10 ms.** About 70×. The
remaining cost is four copies of a 1,000-element vector plus the string work,
none of it quadratic.

**2. `RET` becomes linear in the buffer.** Today 4× the buffer costs 10.2×.
Predicted **3.5–4.5×**. This is the claim that matters, because claim 1 could be
satisfied by a large constant-factor win that leaves the exponent intact, and that
is the flattering reading.

**3. `take`/`drop` become linear, and it shows on a big vector.** Building a
4,000-element vector and taking half of it is quadratic today. Predicted **≥50×**
faster at 4,000 elements, and 4× the input costing ~4× rather than ~16×.

**4. The prelude gets shorter and the core gets longer.** `take` and `drop` lose
their loops; `vec-slice` costs 20–30 lines of native. Net **+15 to +25**, landing
at **6,631–6,641** of 7,500.

**5. The editor's `.out` golden does not move.** Same as ADR-052: this is an
optimisation, and if the transcript changes I have altered behaviour by accident.

## What would refute the entry rather than a number

**If `RET` at 1,000 lines lands above 1 ms**, the four allocations are the cost
and `splice` was the better shape after all — the choice made in
`ADR-053` would be wrong, not merely miscalibrated. Recording the threshold now so
that outcome cannot be reread as a success.

Secondary: if `take`/`drop` do *not* get faster, then `vec-slice`'s general-case
argument — the entire reason it was chosen over `splice` — is empty, and the
entry should have been `splice`.

## Scoring — 2026-07-30, after the fact

**Four of five held, and the two that mattered most held for the right reasons.**
That is a better record than the previous round and the reason is not skill: this
prediction was made about a mechanism already understood, where ADR-052's was made
about one that turned out to be misdiagnosed.

**Claim 1 — `RET` at 1,000 lines to ~0.10 ms, about 70×. Held, and badly
underestimated.** 7.244 → **0.0288 ms**, which is **252×**. I costed four copies
of a 1,000-element vector as if a copy were expensive; a `memcpy` of 1,000
pointers is not. The direction was right and the magnitude was out by 3.5×.

**Claim 2 — `RET` becomes linear, 3.5–4.5× per 4× buffer. Held.** 2.71×, 3.47×,
3.80×, converging on 4.0 from below as the fixed overhead washes out. This was the
claim written to catch a large constant-factor win masquerading as a fix, and the
exponent really did go.

**Claim 3 — `take`/`drop` linear, ≥50× at 4,000 elements. Held, by 34×.** The
actual figure is **1,706×** at 4,000 and 6,792× at 16,000, at 3.81× per 4×
elements against the loop's 15.2×. This is the claim that justified choosing
`vec-slice` over `splice`, and it is the one that came in furthest ahead.

**Claim 4 — prelude shorter, core longer, net +15 to +25 landing at 6,631–6,641.
Half held.** Core landed at **6,637**, inside the band. The prelude got *longer*,
195 → 201, because the comment explaining where clamping lives is bigger than the
two loops it replaced. Second time running that a "this gets smaller" prediction
lost to prose; the estimate should be of the change, not of the code.

**Claim 5 — the editor's `.out` does not move. Held.** Four phase goldens moved
and the transcript did not, so the oracle certifies this as behaviour-preserving
rather than being asked to take it on trust.

**The refutation threshold was not met.** Above 1 ms for `RET` at 1,000 lines
would have said `splice` was the right shape and ADR-053 the wrong call; the
answer is 0.0288 ms, nearly two orders under. And the secondary condition — that
`take`/`drop` show no improvement, which would have emptied the general-case
argument entirely — failed by three orders of magnitude.

## The finding that was not predicted

**Typing is now linear in the buffer, and it was not when last measured.**

| buffer | typing ms/key | `RET` ms/key |
|---:|---:|---:|
| 250 | 0.0042 | 0.0106 |
| 1,000 | 0.0110 | 0.0288 |
| 4,000 | 0.0341 | 0.0999 |
| 16,000 | 0.1269 | 0.3796 |

Typing grows 3.72× per 4× buffer. `insert-str` calls `assoc` on the lines vector,
which is a native O(n) copy — that was always true and was invisible while the
quadratic string path dominated it. `the-editor-program.md` originally claimed
"typing does not depend on the buffer at all"; that claim has now been narrowed
twice by measurement, first to exclude `RET` and now to exclude itself.

**Three entries, three times.** ADR-052 made typing 20× faster and exposed `RET`.
ADR-053 made `RET` 252× faster and exposed `assoc`. Each fix was correct and each
revealed that the thing it fixed had been hiding the next one. The general lesson
is not about strings or vectors: **a benchmark on a system with one dominant
quadratic measures the quadratic, and every other cost in the program is unmeasured
until it is gone.** Nothing in the first editor benchmark could have found the
`assoc` cost, because nothing could see past the scalar loops.
