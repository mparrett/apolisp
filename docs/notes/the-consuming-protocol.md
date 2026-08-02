# The consuming protocol, scored

`the-consuming-protocol-prediction.md` is the pre-registration. This is what
happened.

**ADR-061 works, and the pre-registered benchmark was measuring something other
than what it claimed.** `split` is linear and 382× faster at 64,000 lines; `map`
is still quadratic, for a reason that has nothing to do with this entry and that
nothing in the project had diagnosed.

## The scores

| | Claim | Result |
|---|---|---|
| C1 | `split` 4.0× per doubling → 2.0× | **Confirmed** — 2.0×, 1.88× |
| C2 | 64,000 lines from 19.09 s to under 0.1 s | **Confirmed** — 0.05 s |
| C3 | `map` → 2.0×, 8,000 under 20 ms | **Refuted** — still 4.0×, and not because of ADR-061 |
| C4 | The refcount at `make_mut` reaches 1 | **Confirmed** |
| C5 | Every `.disasm` moves, no `.out` does | **Confirmed** |

## There were three quadratics, and the docs blamed all of it on one

This is the result worth keeping. E-13, E-18, Q37, `TRAPS.md` and three separate
measurements all attributed the O(n²) to `conj`'s copy-on-write never firing.
That was true and it was a third of the story.

**1. `conj` copying every time.** ADR-061's target. Fixed: C4 measured the count
at `Rc::make_mut` as 1, and a probe over a 3,000-line `split` found exactly one
non-unique `conj` in 3,001 calls — the terminating branch, which is not inside a
`recur` and runs once.

**2. `seq_items` cloning the whole collection.** `nth` called it and it returned
`Vec<Value>`, so indexing one element deep-copied the sequence: `nth` was O(n)
where ADR-041 part 1's flat `Vec` promises O(1). A defect against a standing
decision rather than a design choice, and invisible because the name says
"items" and the cost was in the word `clone`. Fixed here by returning `&[Value]`.

**This is why C1 looked refuted at first.** With `conj` fixed and `nth` still
cloning, `xgrep` stayed at 4.0× per doubling and the first reading was "ADR-061
did not work". `split` measured *alone* was already 0.01 s at 32,000 lines. The
benchmark named in the pre-registration ran the whole program, and the whole
program contained a second quadratic — so the number it produced was not the
number the claim was about.

**3. `rest` copying the tail.** `(rest xs)` builds a new collection from
`xs[1..]`, so it is O(n) per call, and `map` and `filter` traverse with it:

```clojure
(loop [in xs out []] (if (empty? in) out (recur (rest in) (conj out (f (first in))))))
```

Measured against the identical algorithm traversing by index instead:

| elements | via `rest` (prelude `map`) | via `nth` |
|---:|---:|---:|
| 8,000 | 0.09 s | 0.00 s |
| 16,000 | 0.36 s | 0.01 s |
| 32,000 | 1.45 s | 0.01 s |

4.0× per doubling against flat. `conj` is fixed in both — the index version
proves it — and `rest` is what is left. This one is **structural**: on a flat
`Vec` with no shared tails, `rest` cannot be O(1), which is ADR-011 and ADR-041
territory rather than a bug.

## C1 and C2 — confirmed, and the number is larger than predicted

`xgrep` over one file, release, best of three after a warm run:

| lines | before | after |
|---:|---:|---:|
| 8,000 | 0.29 s | 0.01 s |
| 16,000 | 1.20 s | 0.01 s |
| 32,000 | 4.84 s | 0.03 s |
| 64,000 | **19.09 s** | **0.05 s** |
| 128,000 | — | 0.12 s |
| 256,000 | — | 0.24 s |
| 512,000 | — | 0.45 s |

382× at 64,000 lines, and the ratio above the timer's resolution is 2.0× and
1.88× per doubling — C1 predicted 2.0×. The one-second threshold moves from
14,500 lines to somewhere past a million.

## C3 — refuted, and the abandon rule does not fire

The pre-registration wrote down what a C3 failure would mean:

> If C1 lands but C3 does not — `split` linear, `map` still quadratic — then
> ADR-061 bought the case that native `split` and `join` would have bought for a
> fraction of the cost and a fraction of the blast radius. At that point the
> honest move is to supersede ADR-061.

**The condition fired and the premise is false**, so applying the rule
mechanically would be wrong. It assumed a C3 failure would mean ADR-061 does not
reach an accumulator that is live across a bytecode call. It does — `map`'s
`conj` gets its kill and mutates in place, which the index-based comparison
above isolates. `map` is slow because of `rest`, and **native `split` and
`join` would not have fixed `map` either**. The counterfactual the rule was
weighing does not exist.

Recording this rather than quietly not applying it, because a pre-registered
trigger that gets ignored on the day is worth less than no trigger at all. The
rule was right to be written and its reasoning was incomplete in a way that only
the measurement could show.

## C5 — and the mitigation that was pre-registered actually got run

43 golden lines changed, every one `MOVE` → `MOVEKILL`, checked by categorising
every changed line rather than by reading them:

```
 43 + MOVEKILL
 43 - MOVE
```

No `.out` moved, the snapshot round-trip passed, the in-language suite passed.

## What the mutation rung caught, which review did not

The first version of the handler-region assertions **passed against a mutation
that removed the guard entirely**. They were of this shape:

```clojure
(recur (+ i 1) (try (conj out i) (catch e out)))
```

The body never throws, so the catch never runs, so a cleared slot is never read.
A true assertion about a path the test cannot reach — the fifth kind ADR-055
names, and the fourth time this project has hit it. The version that has teeth
throws *after* the read:

```clojure
(try (do (conj out i) (throw :x)) (catch e out))
```

which under the mutation gives `:expected [] :actual nil` — a wrong answer and
not a crash, which is exactly the failure mode ADR-061 predicted and the reason
it asked for mutations before landing.

## Two declared survivors, both honest

`MoveKill` clearing the slot, and `seq_items` borrowing. Both are pure cost
changes: they alter no answer anywhere, so no assertion can separate them from
their opposites, and the benchmark is the only check. Declared in `mutate.sh`
with that reason. This is the first kind of legitimate survivor ADR-055 names,
and it is worth noticing that **the second quadratic hid behind exactly this
property for three measurements** — a cost with no observable behaviour is a
thing a test suite cannot see at all.

## What is now open

`rest` is O(n) and `map`, `filter`, `join` and `repeat` ride on it. That is the
remaining quadratic, it is structural rather than a defect, and fixing it means
either a traversal that does not use `rest` (a prelude change, cheap, and it
makes the library disagree with what a user would write) or a representation
with shared tails (ADR-011 and ADR-041, expensive). Filed as **Q38**.
