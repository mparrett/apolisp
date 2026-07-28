# The first programs written in it

`ETHOS.md` names three target workloads — terminal applications, web services,
simulators — and after ten milestones nothing had been written in any of them.
Everything the language had run was a test of the language. This is two
programs written to find out what happens, and what they found.

Neither is in the repository. They are evidence for a decision (Q31), and
committing them would settle by drift the question they were written to ask.

## What worked

**Conway's Life, on a wrapping 20×12 board, three generations.** Correct on the
first run — the glider glides. About sixty lines, no surprises in the core: tail
calls kept every loop flat, `assoc` on a vector worked, `+` is variadic, `let`
takes multiple bindings, `or` works inside a function despite being a macro,
`str` concatenates anything, and the arithmetic never needed a cast.

**A request/response round trip over TCP with a JSON body**, framed with a
four-digit ASCII length header, client and server in one process. Also correct
on the first run. `tcp/listen` / `tcp/connect` / `tcp/accept` compose with the
ordinary `io/read` and `io/write`, exactly as ADR-042 said they would, and
`json/encode` round-tripped a map with a nested vector without ceremony.

The headline is that both worked. The language is not missing anything that
stops a real program being written in it. What it is missing is everything that
would stop you writing the same six functions first.

## What had to be built first

Life is twelve top-level definitions. Six of them are standard library:

| Written by hand | Would have been |
|---|---|
| `zeros` — build a vector of *n* zeros by recursion | `(vec (repeat n 0))` |
| `step-row`, `step-all` — nested iteration with an accumulator | `map` / `for` |
| `render-row` — build a string a character at a time | `(apply str …)` / `join` |
| `run` — count down *n* generations | `dotimes` |
| `wrap` — negative-safe index wrap | `mod` (`rem` exists; it keeps the sign) |
| `total` (in the service probe) — sum a vector | `reduce` |

**The single biggest gap is `loop`/`recur`.** There is no looping form at all,
so every iteration becomes a *named top-level function with an accumulator
parameter threaded by hand*. Four of Life's twelve definitions exist only
because of this, and none of them is about Life. It is also why the program
reads as flat and repetitive rather than wrong: the shape is fine, there is
just far too much of it.

This is Q29's remaining half — no prelude *functions* — meeting Q12's no-module
answer. Neither entry predicted that the cost would show up as control flow
rather than as missing utilities.

## Two findings that are not ergonomics

**`io/read` is a short read, and the probe passed by luck.** The frame header
said 27 bytes; the code asked for 42 and got 27, because `io/read` returns as
soon as any bytes arrive. It looked correct. It would have failed the first time
a payload crossed a packet boundary, and nothing in the language reads exactly
*n* bytes — every framed protocol has to hand-roll the loop, and the hand-rolled
loop cannot distinguish a short read from a peer that stopped. Filed in
`TRAPS.md`.

**There is no way to turn a string into a number.** Not a missing convenience —
there is no primitive at all. The prim table goes value→string via `str` and
never back. A length header, a config file, a command-line argument, and a line
of user input are all unreachable.

The available workaround is the interesting part:

```clojure
(+ 1 (json/decode "27"))   ; => 28
```

`json/decode` is the only string→number path in the language, and JSON is an
**optional host adapter**. So `--no-default-features` removes the only way to
parse a number from text.

ADR-013 says features gate host capability and never language semantics, and
this is the first case where that has quietly stopped being true — not because
a feature gates a semantic, but because a semantic was never given a home and
has been squatting in a feature. `just subtract` cannot catch it: the build
without `json` is green, because nothing in the suite parses a number from a
string either. It took writing a program.

## The shape of the finding

The core is in better condition than the surface. Ten milestones of building the
machine produced a machine that works; what no milestone produced was the thin
layer everyone actually types against, and its absence is invisible from inside
the test suite — because a test suite written to exercise the VM naturally calls
primitives directly and never needs `map` twice.

That asymmetry is the argument for Q31 being about programs rather than about
features. The gaps above were not on any list. They were found in an afternoon
by two programs that both worked.
