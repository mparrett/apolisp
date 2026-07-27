# Milestone 6 — mutation check on collections, equality, and the tower

**Code:** `de916cc` · **Not normative.** A Q18 mutation pass over milestone 6's
load-bearing lines, predictions written before running, plus the measurement
that produced erratum E-13.

| Mutant | Predicted | Actual |
|---|---|---|
| M1 — `equal` compares `Int` to `Float` numerically | fails, in the language suite and nowhere else | **failed** — exactly there |
| M2 — `equal` refuses to cross list and vector | fails (numbers, collections) | **failed** |
| M3 — map equality becomes positional, so insertion order is identity | fails (numbers) | **failed** |
| M4 — `put` appends instead of replacing | fails (collections) | **failed** |
| M5 — `same_const` compares floats with `==` again | fails (numbers) | **failed** |
| M6 — the integer fast path is dropped, so `(+ 1 2)` is `3.0` | fails widely | **failed** (12 tests) |
| M7 — `conj` on a vector prepends | fails (collections) | **failed** |
| M8 — `Rc::make_mut` replaced by an unconditional clone | (added after the measurement) | **passed — the whole suite stayed green** |

**Every one of M1 through M5 and M7 was caught by the in-language suite and by
nothing else.** That is the argument for rung 4 in one line: the semantics this
milestone added are language-level, and the Rust tests were never going to see
them. It is also a warning — `tests/lang/` is now a single point of failure for
a whole layer of meaning, and a suite that stops at the first failure hides
everything after it.

## The refutation: a transient win that is not there

ADR-041 part 1 argued that `Rc::make_mut` removes Q6's O(n²), because a fresh
accumulator has one reference and the loop mutates a buffer it already owns.
Before writing more prose about it, instrument and check.

**Pre-registered prediction:** the refcount at a `conj` is 2, not 1, because the
argument is live in the caller's slot window while the primitive holds its own
clone — so `make_mut` copies every time and the win does not exist.

**Result: confirmed, and worse than predicted.** Building a five-element vector
printed `copied=true` on every iteration. It is not only the argument window: in
`(build (conj acc i) (+ i 1))` the accumulator is *also* still bound to `acc`,
which is live until the tail call replaces it. Even nested construction —
`(conj (conj [] 1) 2)` — copies, because the inner result sits in a slot and the
primitive clones out of it.

M8 is the confirmation: replacing `make_mut` with an unconditional clone changed
nothing any test could see. The two are indistinguishable in the language as
built.

E-13 records the correction. The decision stands — flat `Vec` with
copy-on-write is still the right shape and `make_mut` is still how you write it
— but the reason was wrong, and a wrong reason is what gets reused. Getting the
win needs the compiler to kill a slot on its last use, or a call protocol where
a primitive consumes its arguments. Both are ADR-021 experiments; neither is a
collections question.

**What this says about the habit.** The measurement took about ten minutes and
falsified a paragraph that had already been committed, reviewed, and reasoned
from. Nothing in the test suite could have caught it, because it is a
performance claim and the suite tests behaviour. Pre-registration is the only
instrument that was going to find it, which is the argument `BUILD.md` makes for
it and this is the first time it has paid here.

## Method

Commit first, then mutate, then `git checkout -- src/lib.rs`; count with
`cargo test --no-fail-fast` (`milestone-5-mutants.md` explains why).

New this pass: when a mutant is a *performance* claim rather than a behavioural
one, a mutation check cannot answer it — M8 survives by construction. Reach for
instrumentation and a number instead, and write the prediction down first.
