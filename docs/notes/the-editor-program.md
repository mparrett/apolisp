# The editor program

`the-editor-prediction.md` is the pre-registration. This is what happened, and
the headline is that **the prediction was wrong about the mechanism and the
measurement found something better.**

A nano-shaped editor, 289 lines — 227 pure core, 62 shell. Opens a file, moves,
types, deletes, splits and joins lines, scrolls, saves, quits. Driven under a
pty: typed into it, saved with `C-o`, quit through the `C-x C-c` prefix, and the
file on disk changed. Written in the style of `../3p/legmacs`, a 2,700-line
Emacs-flavoured editor in let-go, so the architecture is borrowed rather than
invented: state is one map, every buffer operation is pure `state → state`, a
chord is a string, the keymap is a nested map, and the loop is a fold.

## The pure core pays for itself immediately

The prediction said testability rather than capability would shape the program.
It did, and the payoff is concrete: because `dispatch` and `frame` touch nothing,
the editor has a **golden**.

```
--- frame
hello
!world
demo.txt * 2:6
```

Nineteen chords replayed through `reduce dispatch`, no terminal anywhere,
deterministic. The same two functions drive the interactive shell. So the thing
the prediction called a structural breakdown — an editor cannot be pinned by the
corpus — is only true of the 62-line shell, and the 227-line core is ordinary
rung-3 material. legmacs reaches the same place with 19 test files; here it is
one `.out`.

## Two gaps that were not predicted

**`concat` returns a list, and `assoc` refuses lists.** `split-line` builds the
new `:lines` with `concat`, which silently turns the vector into a list, and the
editor then throws `:type` on the *next* keystroke — one operation away from the
cause. Every structural edit needs `(vec (concat …))`. The collection surface is
not closed under its own operations, and the failure is delayed.

**There are no type predicates at all.** No `map?`, `nil?`, `keyword?`,
`string?`, `type`, `kind-name`. The only way to ask what a value is, is to `try`
an operation that fails on the wrong one. This blocked the obvious keymap design
— a sub-map means a prefix — and forced tagged entries, `[:command kw]` and
`[:prefix submap]`. That is what legmacs does anyway, so the gap removed the
worse option rather than the better one. It is still a hole: nothing in the
language can dispatch on a heterogeneous value.

Neither was on the pre-registration. By its own scoring rule that is the honest
result to record, and it is the argument for writing programs rather than
reasoning about them.

## The benchmark

Release build, startup subtracted, each measurement its own process because the
language has no clock.

**1. `conj` in a loop is quadratic.** The ratio converges on 4.0, which is the
signature.

| n | time | ratio vs half |
|---:|---:|---:|
| 1,000 | 2.3 ms | |
| 2,000 | 7.2 ms | 3.14× |
| 4,000 | 25.7 ms | 3.57× |
| 8,000 | 98.9 ms | 3.85× |

**2. The prelude inherits it.** `map` is a `conj` loop, so `map`, `filter`,
`range`, `repeat`, `take`, `drop` and `join` are all quadratic in output length.

| n | `map` | ratio |
|---:|---:|---:|
| 1,000 | 4.6 ms | |
| 2,000 | 18.4 ms | 3.97× |
| 4,000 | 70.3 ms | 3.82× |
| 8,000 | 271.7 ms | 3.87× |

This is **Q6's O(n²) and errata E-11 reached by measurement** rather than named
in a document, which is the first time.

**3. Per-keystroke cost does not depend on the buffer at all.** Line fixed at 25
characters:

| buffer lines | µs/keystroke |
|---:|---:|
| 250 | 123.5 |
| 500 | 132.0 |
| 1,000 | 133.4 |
| 2,000 | 138.0 |

Eight times the buffer, 1.12× the cost.

**4. It depends on the line, and superlinearly.** Buffer fixed at 200 lines:

| line length | µs/keystroke |
|---:|---:|
| 25 | 133.2 |
| 50 | 170.5 |
| 100 | 263.5 |
| 200 | 514.3 |
| 400 | 1,249.2 |

**5. The 10,000-line case the prediction named.** A whole-file pass, release:
26 ms at 2,500 lines, 102 ms at 5,000, **440 ms at 10,000** — quadratic. In the
debug VM, build plus pass is **4.3 s**.

## Scoring the prediction

**Claim 1 — "typing stays comfortable to ~50,000 lines." Held, for the wrong
reason.** Buffer size is nearly free, so it would be comfortable at any line
count. The stated mechanism — `assoc` copying the spine per keystroke — is
**refuted**: that cost exists and is swamped by something else.

**Claim 2 — "any whole-file pass is O(n²), tens of seconds in the debug VM at
10,000 lines." Held directionally, magnitude overestimated about fivefold.**
Quadratic confirmed cleanly; 4.3 s rather than tens.

**Claim 3 — "it will be misdiagnosed." Held, and the author was the one who
misdiagnosed it.** The prediction attributed the cliff to `assoc` on the lines
vector. It is not there. Recording this is the point of having written the
number down first.

## The finding: the safe string path is the slow one

Per-keystroke cost tracks line length because the cursor is a **character**
column and `str-slice` takes **byte** indices (ADR-049). Character-level surgery
therefore has to go through `str-scalars` → `take`/`drop` → `scalars-str`, and
`take` and `drop` are language-level `conj` loops — so an edit is quadratic in
the column.

The language already has the fast primitive. `str-slice` is native. Rewriting
the same insert against it, on an ASCII line so the indices coincide:

| line length | scalars `take`/`drop` | native `str-slice` | speedup |
|---:|---:|---:|---:|
| 100 | 272.6 µs | 3.74 µs | 73× |
| 400 | 1,244.2 µs | 3.59 µs | 347× |
| 1,600 | 12,604.8 µs | 4.34 µs | 2,905× |

**The native path is flat.** ~4 µs regardless of line length, against a curve
that reaches 12.6 ms.

So the language has a primitive that is *fast and byte-indexed* and a surface
that is *correct about Unicode and quadratic*, and they are not the same one.
Nothing noticed because no program had done character-level surgery in a loop
before — `TRAPS.md` already records that `str-len` made the unsafe path the
quiet one, and this is the same fault line one layer down: **ADR-049 made the
safe path the slow path.**

The fix is one primitive — a scalar-indexed slice, or a scalar→byte offset
conversion — and it collapses a 2,905× gap. Filed as **Q34** rather than built,
because a new primitive is a decision.

## What this says about the gap-buffer idea

The pre-registration proposed a line-level gap buffer to dodge the per-keystroke
`assoc`. **That would have optimised the wrong axis.** Buffer size is already
free; the cost is inside a single line. If anything wants a gap buffer here it is
the *line*, not the vector of lines — and a scalar-indexed slice removes the
reason to want one at all.

This is the clearest thing the benchmark bought: a plausible optimisation,
pre-registered, and refuted before anyone spent a day building it.
