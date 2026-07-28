# The report program

Q31's candidate 1, run again now that ADR-046, ADR-047 and ADR-048 have landed:
write a program, see what breaks.

This one is deliberately not a probe shaped by what the language has. It is the
most ordinary program there is — **read a CSV, total by name, print a report
sorted by total descending** — chosen because it leans on the newest code (the
sequence library) and the least-exercised (strings, `fs`).

It works, and the output is correct. The accounting is the finding.

## 54 lines of code. 30 of them are standard library.

| | lines |
|---|---:|
| `split`, `merge2`, `sort-by` — things the language does not have | 30 |
| the program that reads a CSV and prints a report | 24 |

The ratio has moved since `first-programs.md`, where six of Life's seventeen
definitions were library. It has not moved as far as it looks: what ADR-047 and
ADR-048 removed was the *iteration* boilerplate, and what is left is a different
layer — the string and ordering functions that iteration was hiding.

## What was missing

**`split`.** Hand-rolled over `str-scalars`. The workaround is *itself*
limited: it takes a single separator **character**, because comparing a
multi-character separator needs a substring search the language also does not
have. So a CSV works and `", "` does not.

**`sort`.** The version reached for first is selection sort — repeatedly take
the maximum and remove it with `filter`. That silently drops duplicates,
because `filter` removes every element equal to the maximum rather than one of
them. The program above therefore contains a merge sort, which needs to halve a
vector, which needs `take` and `drop`, which also do not exist — so both halves
are built with an explicit `loop` and `nth`.

**`apply`.** No variadic dispatch, so a function cannot be called with a
computed argument list.

**String padding.** `(join "" (repeat (- 12 (str-len name)) " "))` is the idiom
every report writes, and it is spelled out longhand every time.

## The finding: `str-len` is bytes, and padding is silently wrong

ADR-018 makes strings not-sequences and ADR-041 part 5 spells the surface:
`str-len` is **bytes**, `str-slice` takes **byte indices**. Both are decided,
both are right, and neither is the trap.

The trap is that the language is *loud* about this everywhere except here.
`str-slice` raises an error when a cut lands inside a character. `count` on a
string is refused outright, because ADR-041 says that is where Unicode
assumptions get made by accident. And `str-len` quietly answers in bytes:

```
str-len "dave"  → 4
str-len "josé"  → 5      ; four characters
```

So the padding idiom produces this, with no error and nothing to notice:

```
dave        |
josé       |
```

One space short, one row misaligned, in the one idiom that exists to line
columns up. The correct spelling is `(count (str-scalars s))` — which allocates
a vector of every code point in order to count them.

`str-slice`'s error and `count`'s refusal exist because ADR-018 knew this was
where Unicode goes wrong. `str-len` is the hole in that argument: same hazard,
same file, no noise.

## Two smaller ones

**`io/read-all` answers with bytes.** Every text file read is
`(bytes-str (io/read-all f))`. Correct — ADR-018 makes text/bytes conversion
explicit — and it is the first thing every program does with a file.

**`keys` returns a list, in insertion order.** Deterministic, so BUILD.md's rule
5 holds and a report is reproducible. Worth writing down because "map iteration
order" is the phrase that usually means the opposite, and because a program
that wants sorted keys has to sort them itself.

## What worked

Nothing about the new code needed thinking about. `parse-number` was used
without noticing it was new. `loop`/`recur` and the sequence library carried the
whole program; `reduce` with a map as the seed is how the aggregation is
written, and it reads the way it would anywhere else. `with-open` did its job.

That is the useful shape of the result: the pieces added on evidence were all
load-bearing immediately, and the next round of gaps is one layer up rather
than a repeat of the last.
