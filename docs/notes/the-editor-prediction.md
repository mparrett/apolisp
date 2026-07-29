# The editor, predicted before it is written

A pre-registration, filed 2026-07-28. **No editor exists.** This is what is
expected to happen if one is built, written down first so that the refutations
are worth something — `BUILD.md` asks for this before a benchmark or a mutation
check, and the same argument applies to a program written to find gaps. The
three programs before this one (`first-programs.md`, `the-report-program.md`,
`the-pager-program.md`) were all written without one, and in each case the
interesting result had to be reconstructed afterwards from what surprised
whoever was typing.

The program: a simple full-screen text editor. Open a file, move a cursor,
insert and delete, save, quit. The next rung of the terminal workload after the
pager, and the first program that would be *used* rather than run once.

## Ground truth at the time of writing

Checked, not assumed, so a later reader can tell whether the ground moved rather
than whether the prediction was wrong:

- `cond` and `case` do not exist. `when`, `unless`, `and`, `or` do.
- `set-cell!` exists and is a core form, so mutable state is available.
- `term/read-key` maps Char, Enter, Backspace, Left, Right, Up, Down, Tab and
  Esc. **Delete, Home, End, PageUp, PageDown and every function key collapse to
  `:key :other` with no detail.** Only `CONTROL` is exposed — no Alt, no Shift.
- Painting is `(term/open)` and ANSI by hand (ADR-051).
- `str-slice` takes byte indices; `str-scalar-len` and `str-scalars` are the
  character-aware surface (ADR-049).
- Errata **E-11**: copy-on-write `make_mut` always copies today, because the
  value is still bound to a live local. The O(n²) named by **Q6** is still there.

## What is predicted *not* to break

Held with reasonable confidence, and listed because a pre-registration that only
names risks is a worry list rather than a prediction.

**The event loop.** `(loop [state s] (draw state) (recur (handle state (read-key))))`
inside a `try` that restores raw mode. Measured flat this session — 3.42 MB peak
at 10⁵ iterations inside a `try`, 3.39 MB at 10⁶ — so the shape an editor needs
is the shape that was just verified.

**Undo.** Nearly free, and this is the one most likely to be assumed expensive.
An undo stack holds vectors of `Rc<StrObj>` lines, so the lines are shared and
only the spine is copied: 100 states of a 5,000-line file is on the order of
8 MB.

**Painting, saving, and reading.** Settled by ADR-051, ADR-016 and ADR-018
respectively. Cursor positioning is more ANSI through the same handle.

## The three frictions

Each is expected to be cheap, and each should be scoped by the program that
wanted it rather than built ahead — the rule that produced ADR-046 through
ADR-051.

**1. The first thing written will be `cond`.** The pager had four key bindings
and was already three `if`s deep; an editor has around twenty. Expected to be a
six-line prelude macro.

**2. Key coverage is what will make it feel broken** rather than merely
incomplete, and this is predicted to bite *first* — before performance, before
anything structural. Backspace exists and Delete does not, and they are
different keys. Home, End, PageUp and PageDown are all `:other`. Expected fix is
about ten lines in `term.rs`, outside the budget.

**3. Character index versus byte index, round two.** The cursor is naturally a
character column and `str-slice` takes bytes, so every insert and delete
converts through `str-scalars` → `take`/`drop` → `scalars-str`. **The predicted
right answer is to hold the line under the cursor as a vector of scalars and
convert only at the edges — and the prediction is that this is not what gets
written first.** `TRAPS.md` already carries the `str-len` lesson; this is the
next mile of the same road.

## The cliff, and the number on it

The one quantitative prediction, and the one worth the file.

Editing one line of an N-line buffer is `(assoc lines i new)`, which by E-11
copies the whole spine: N `Value`s plus **N refcount bumps**. Three claims,
falsifiable:

1. **Typing stays comfortable much further than expected.** Roughly 10³ bumps at
   1,000 lines and 10⁴ at 10,000 — on the order of 100 µs, against a keystroke
   budget of 100 ms. Predicted usable interactively to **~50,000 lines**.
2. **Any whole-file pass falls off a cliff.** Search-and-replace, reindent —
   anything shaped `reduce` plus `assoc` over every line — is O(n²). At 10,000
   lines that is ~10⁸ `Value` clones with refcount traffic, predicted to be
   **tens of seconds in the debug VM** and clearly noticeable somewhere around
   **5,000–10,000 lines**.
3. **It will be misdiagnosed.** The symptom is a whole-file operation and the
   cause is a per-element clone, so the expected first explanation is "the VM is
   slow" rather than "E-11's copy-on-write does not pay yet". Recorded because
   this is the part a later reader can most usefully check.

This feeds Q6 and E-11 rather than opening anything new. If claim 2 holds it is
the first time the O(n²) has been reached by a real program instead of named in
a document.

## What actually breaks, and it is not the language

**An editor cannot be pinned by the corpus.** It is interactive, so rung 3 has
no `.out` for it — the oracle that localizes every other regression to one phase
does not apply. The prediction is that **testability, not capability, is what
shapes the program**: written as a pure `state × key → state` core with a thin
I/O shell, rung 4 can test the core in-language, and written any other way
nothing can test it at all. If that is right, it is an argument about program
structure that the language imposed, which is a different kind of finding from
the three above.

## Two smaller ones

**Raw mode plus a Rust panic wrecks the terminal.** `try`/`finally` covers a
throw and not a panic. ADR-045 records this as the sharpest reason the terminal
is an adapter; an editor is the program that turns it from theoretical into
daily.

**An editor is the first program that would want the thing ADR-051 forbids.**
"Reopen where I was" is suspend-and-resume, and an editor holds a tty handle, so
`capture` refuses it. ADR-051 argues that non-snapshottability is intended
rather than incidental. A program that genuinely wants the opposite is the test
of whether that argument holds, and this is the first one.

## How to score this

Afterwards, record against each numbered claim: **held**, **refuted**, or **not
reached**. The refutations are the point. In particular, the prediction is
*wrong as a whole* if the editor turns out to be blocked by something absent
from this file — in which case the useful finding is not the blocker but the
fact that reading the source and writing three programs did not surface it.
