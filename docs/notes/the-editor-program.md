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

Sizes and measurements in this note describe the program as it was benchmarked.
The core has grown twice since — the key hint, then the goal column — and now
lives in the corpus at `tests/corpus/editor.xs`, which is canonical. The shell
does not, for the reason the next section gives.

## The pure core pays for itself immediately

The prediction said testability rather than capability would shape the program.
It did, and the payoff is concrete: because `dispatch` and `frame` touch nothing,
the editor has a **golden**.

```
--- frame
hello
!world
C-o save  C-x C-c quit  C-g hide
demo.txt * 2:6
```

Nineteen chords replayed through `reduce dispatch`, no terminal anywhere,
deterministic. The same two functions drive the interactive shell. So the thing
the prediction called a structural breakdown — an editor cannot be pinned by the
corpus — is only true of the 62-line shell, and the core is ordinary rung-3
material. legmacs reaches the same place with 19 test files; here it is one
`.out`.

The key hint in that frame came later — on at startup, toggled with `C-g`, the
change that took the core from 227 lines to 245. It cost one thing worth
recording. `scroll-to-cursor` and `frame` had each computed the text height as
`(dec rows)` independently, which is fine while the footer is exactly one line
tall and wrong the moment it is not: the two disagree and the painted cursor
lands on the wrong row. Both now ask a single `text-rows`. The duplication was
invisible until something varied, and what made it a five-minute fix rather than
a debugging session is that both functions are pure — the golden showed the
disagreement without a terminal in the loop. The benchmark below predates the
hint and is unaffected by it.

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

**3. Typing does not depend on the buffer at all.** Line fixed at 25 characters:

| buffer lines | µs/keystroke |
|---:|---:|
| 250 | 123.5 |
| 500 | 132.0 |
| 1,000 | 133.4 |
| 2,000 | 138.0 |

Eight times the buffer, 1.12× the cost.

**This measured `insert-str`, and the note originally reported it as
"per-keystroke".** That generalization was wrong, and an external review caught
it (`ed-review-2026-07-29`) by pointing out that `split-line` rebuilds the whole
line array — `(vec (concat before [head] [tail] after))` — which typing never
touches. Measured afterwards, 200 keystrokes each:

| buffer lines | typing | `RET` |
|---:|---:|---:|
| 250 | 0.198 ms | 1.165 ms |
| 1,000 | 0.227 ms | 13.228 ms |
| 4,000 | — | did not finish in two minutes |

Four times the buffer costs typing 1.15× and `RET` **11.4×**. They are two
different cost classes, and at 1,000 lines Enter is roughly sixty times the price
of a character. The claim "the buffer is free" holds only for the keystroke that
was actually measured.

Worth separating from the other corrections in this note: claims 1 through 3 of
the pre-registration were scored against a benchmark, and this one was a benchmark
scored against a reader. Measuring the wrong operation is not a failure a more
careful prediction would have caught — nothing in the pre-registration
distinguished typing from splitting either.

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

The external review proposed a **line zipper** — the line split into the
characters before and after the cursor — arriving independently at the axis the
benchmark had corrected the pre-registration onto. Two parties reaching the same
axis separately is the strongest evidence the note has that the axis is right.

It still does not work here, and for a reason worth writing down. The zipper's
payoff is that inserting becomes a push onto the end of one side, "an $O(1)$
operation". `conj` is not O(1) in this language — measured, best-of-five, startup
subtracted:

| n | build | ratio |
|---:|---:|---:|
| 1,000 | 2.4 ms | |
| 2,000 | 11.2 ms | 4.67× |
| 4,000 | 38.4 ms | 3.43× |
| 8,000 | 121.1 ms | 3.15× |

Converging on 4.0 per doubling, which is Q6/E-11 again. A zipper built on `conj`
inherits that curve, so it relocates the cost rather than removing it. **Fixing
Q6/E-11 is a prerequisite for the zipper, and fixing Q6/E-11 removes most of the
reason to want one.** The reviewer assumed Clojure's persistent vector; the
surface is Clojure's, and the complexity behind it is not.

## The goal column, which no amount of benchmarking would have found

The same review found a real defect, and not a performance one. At column 15,
`down` onto a two-character line and `down` again onto a long one left the cursor
at column 2: `clamp-cursor` clamped against `:cursor-col`, the value the short
line had already destroyed. Fixed with a `:goal-col`, and the golden's chord
script had to grow a second half to reach it — the old nineteen chords never
crossed lines of differing length.

The interesting part is the detection channel. Nineteen chords through `reduce
dispatch`, a pty session, a benchmark and a write-up all missed a bug that a
reader found by knowing what editors do. The golden pins what the program does,
not what it should do, and no oracle in this project has an opinion about the
second. That is the standing limit of the whole rung-3 apparatus, stated here
because this is the first time something walked straight through it.
