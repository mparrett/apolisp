# The grep program

`the-grep-program-prediction.md` is the pre-registration. This is what happened.

`examples/xgrep.xs`, 68 lines of which 38 are code: `xgrep PATTERN DIR` lists a
directory, reads each file, and prints every line containing a fixed substring
as `name:lineno:line`. It worked on the first run. The fifth program written
under Q31's practice, and the first to use ADR-058's arguments and ADR-060's
listing — which is why it was written, since those two were scoped by a review
rather than by a program that wanted them.

**The headline is P4, and P4 was refuted.** The run this was written to justify
is not slow enough to want the thing it was written to justify.

## The scores

| | Claim | Result |
|---|---|---|
| P1 | ≤60 code lines, nothing hand-rolled | **Split** — 38 lines, but one hand-rolled function |
| P2 | The quadratic is the wall, `split` is where | **Confirmed** on the ratio, **refuted** on the absolute number |
| P3 | All progress arrives at once, at exit | **Confirmed** |
| P4 | The run is slow enough for that to matter | **Refuted** |
| P5 | `:kind` is enough to skip a subdirectory | **Confirmed** |

## P2 — confirmed, and the ratio is the part worth keeping

One file, one match, timed end to end:

| lines | time | vs. previous |
|---:|---:|---:|
| 1,000 | 0.04 s | |
| 2,000 | 0.14 s | 3.50× |
| 4,000 | 0.50 s | 3.57× |
| 8,000 | 1.92 s | 3.84× |

Predicted "more than 3× per doubling"; measured 3.84× at the top. That is the
same figure the editor's `map` came in at (3.87×), from a different function in
a different program, which is worth more than either number alone: it is
the same constant showing up twice independently (now Q37; E-11/Q6 was the wrong citation, since Q6 is retired).

Attributed, on `docs/ADR.md` at 3,843 lines:

```
read the file            0.00 s   (below the timer's resolution)
read and split it        0.16 s
```

So `split` is essentially the whole cost, as predicted.

**Re-run in release, later the same day**, once the toolchain was fixed. The
debug caveat this note originally carried is withdrawn: the profile changes the
constant and not the exponent, so nothing above was an artefact.

| lines in one file | release | ns per line² |
|---:|---:|---:|
| 2,000 | 0.02 s | 5.00 |
| 4,000 | 0.07 s | 4.38 |
| 8,000 | 0.29 s | 4.53 |
| 16,000 | 1.20 s | 4.69 |
| 32,000 | 4.84 s | 4.73 |
| 64,000 | 19.09 s | 4.66 |

A stable 4.4–4.7 ns per line² across a 16× range, which is as clean an n² fit
as this project has measured anything. Release is about 6.6× faster than debug
and the ratio per doubling goes *up* rather than down — 4.0×, the asymptote —
because the quadratic term dominates a smaller constant sooner. The half of P2
that predicted ">50 ms for ADR.md" is refuted in release, where it is about
25 ms.

## P4 — refuted, and that is the useful result

Over `docs/` — 22 files, about 14,000 lines — the whole run takes **0.72 s in
debug**, and would be well under 0.2 s in release. The prediction was more than
a second, on the theory that this would finally be a program slow enough that a
person would want to know where it had got to.

It is not. And the pre-registration committed in advance to what that means:

> If it is refuted, the conclusion is not "add a flush point anyway" — it is
> that a flush point is still waiting for a program, and that this one is not it
> either.

So Q33 shape 4 stays open and stays unbuilt. What this program did establish is
the *size* of the workload that would ask. **The program that wants a flush
point is not one that reads many files, it is one that reads a big one.**

**A correction, and it is mine.** This note first put that threshold at "about
30,000 lines", and it is **14,500** in release. The error was arithmetic rather
than measurement: the debug run had already passed a second at 8,000 lines, and
the extrapolation went upward from there instead of downward. It was wrong in
the flattering direction — it made the wall further away than it is — which is
the direction to be suspicious of. Corrected in `TRAPS.md` and Q37 as well as
here.

P3 held exactly as documented: the first line of output arrived 0.747 s into a
0.75 s run, i.e. at exit, all 703 lines at once.

## P1 — split, and the split is the informative half

38 code lines against a budget of 60. But one function had to be defined that
belongs in a standard library:

```clojure
(def has?
  (fn has? [s sub]
    (not (= nil (str-index-of s sub 0)))))
```

That is `str-contains?`, and it was **item 1 on the list of things I expected to
want and not have** — which is exactly what makes it uninteresting. A prediction
that lands is not a finding. Recording it as a candidate for the prelude on the
ADR-049 pattern, and not building it: one program is the evidence bar those
entries used, and this is one program.

## The unpredicted finding: a local shadows a primitive, permanently

The natural way to write the file loop is:

```clojure
(loop [rest names]
  (if (empty? rest) nil (recur (rest rest))))
```

and it fails at run time with:

```
{:type :vm-error :kind :not-callable :message "cannot call a vector"}
```

The loop variable `rest` shadows the primitive `rest` for the whole body, so
`(rest rest)` calls the vector. The error names the symptom and not the cause —
a person reads "cannot call a vector" and goes looking for why a vector is in
function position, and the answer is that their loop variable ate the function
they were trying to call.

**What makes this worse here than in Clojure is a decision rather than an
accident.** In Clojure the shadowed core function is still reachable as
`clojure.core/rest`. ADR-027 fixes v1 at one namespace with no qualification, so
here there is *no spelling at all* for the shadowed primitive inside that
scope — the name is not obscured, it is gone. The workaround is to rename the
binding, which is what `xgrep` does (`todo`), and the cost is that every program
has to know which names are load-bearing before it picks a variable name.

This is now in `TRAPS.md`. It was not on the list of things I expected to be
missing, and it is the one thing this program found that reading the surface
would not have.

## What the three new entries actually bought

Worth stating plainly, since the point of writing this was to check work scoped
by a review rather than by a program.

- **ADR-058** carried its weight immediately: `xgrep` takes a pattern and a
  directory, and neither is expressible any other way. Without it this program
  cannot exist.
- **ADR-060** carried its weight and P5 is the specific reason. `:kind` meant
  skipping the subdirectory was one `filter`; without it, with no type
  predicates in the language, the program would have had to `try` an `io/open`
  on every entry and read a failure as "that was a directory" — which cannot
  tell a directory apart from a file it lacks permission on, and this program
  has a separate branch for exactly that case.
- **ADR-059** is untouched by this program. A batch tool over files does not
  accept sockets, so the accept deadline is still evidenced only by the review
  that found it. It stays the one of the three with no program behind it.
