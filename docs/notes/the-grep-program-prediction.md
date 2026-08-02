# The grep program, pre-registered

*Written 2026-08-02, before the program. Scored in
`the-grep-program.md`, which is the file to read if you only want the result.*

Q31's practice, and the fifth program. The first four were a pair of probes,
a report, a pager, and an editor; this one exists because ADR-058 and ADR-060
just landed and were scoped by a **review** rather than by a program that wanted
them — which Q31 now records as the weaker kind of evidence. A program is how
that gets checked.

What it is: `xgrep PATTERN DIR`, which lists a directory, reads each file, and
prints every line containing a fixed substring, prefixed with the file name and
a line number. No regex — `str-index-of` is what exists. It is the shape Q31
names as the program that would ask for a flush point: a batch tool over many
files, long enough to want to say how far it has got.

The claims, in the order they will be scored.

## P1 — it needs no hand-rolled library, and fits in 60 lines

The pager was 46 lines with four of them host shim and nothing hand-rolled, and
that was before arguments and directory listings existed. This should be no
worse. **Refuted if** the program has to define anything that belongs in a
standard library, or if it runs past 60 lines excluding comments.

## P2 — the quadratic prelude is the wall, and `split` is where

`split` accumulates with `conj` in a `loop`, and E-13 measured that
`Rc::make_mut` never has a unique reference at a native call, so every `conj`
copies the whole vector. Splitting a file of *n* lines is therefore
**n²/2 element copies**, and this is the first program here that runs over real
files rather than corpus-sized ones.

Two numbers, so this can be wrong rather than vague:

- **A file of 8,000 lines takes more than 3× a file of 4,000 lines.** Linear
  would be 2×. The editor's `map` measurements came out at 3.8× per doubling,
  so 3× is a conservative version of the same claim.
- **`docs/ADR.md` alone — about 3,650 lines — costs more than 50 ms** in
  `split`, against a file read that should be well under 1 ms.

**Refuted if** the doubling ratio is at or under 2×, or if ADR.md splits in
under 50 ms. Either would mean E-13's "it never pays" has stopped being true, or
that the constant is small enough that Q6 is not the wall at these sizes.

## P3 — every progress line arrives at once, at the end

`io/stdout` is buffered into `Vm::out` and printed after the run, so a progress
line per file cannot be a progress line. This is documented in TRAPS.md and is
as close to certain as anything here; it is written down because it is the
premise of P4 and a premise that goes unstated is one nobody scores.

**Refuted if** anything appears before the process exits.

## P4 — the run is slow enough that the buffering actually matters

This is the one worth writing down, because it is the one I expect to be
**wrong**, and it is the whole reason to write this program rather than reason
about it.

The claim: over `docs/` the run takes **more than a second**, which is long
enough that a person watching it would want to know where it had got to — which
is what would make this the program Q33 shape 4 has been waiting for.

**Refuted if** it comes in under a second. And if it is refuted, the conclusion
is not "add a flush point anyway" — it is that a flush point is still waiting
for a program, and that this one is not it either. Q33's rule survives being
inconvenient, or it is not a rule.

The failure mode this is guarding against is specific: having just built three
things, the tempting next move is to build the fourth, and a prediction written
after the run would be reconstructed to justify it.

## P5 — `:kind` is enough to skip a subdirectory

There are no type predicates (TRAPS.md), so a program that had to ask "is this
a file" any other way would have to `try` an `io/open` on every entry and treat
the failure as the answer. ADR-060 says the listing carries the kind for exactly
this. **Refuted if** the program needs a `try` to decide what to read.

## What I expect to want and not have

Recorded so that "it was obvious afterwards" is checkable. In rough order of
confidence:

1. **`str-contains?`** — `(= nil (str-index-of s pat 0))` is the spelling, and
   it reads like a search when it is a test.
2. **No way to ask a file's size.** `io/read-all` is the only way to find out
   how big something is, which means a grep over a directory containing a large
   binary reads it all into memory to discover it did not want it.
3. **Nothing to write to stderr.** A file that cannot be read has to report
   into the same stream as the matches, which is the one thing a grep must not
   do if its output is going to be piped anywhere.

Nothing is committed to fixing any of these. The point of listing them first is
that the interesting result is the one that is *not* on this list.
