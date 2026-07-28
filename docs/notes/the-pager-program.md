# The pager program

Q31's candidate 1 again, aimed at the workload nothing has touched. `ETHOS.md`
names three — terminal applications, web services, simulators — and after
`first-programs.md` covered a simulator and a service, the terminal was the one
left. It is also where the least-exercised code in the repo lives: `term.rs` is
120 lines behind three natives, and the whole suite's coverage of it is one
arity check.

The program is a pager. Show one screenful of a file, scroll with `j`/`k`/space,
quit with `q`. The most ordinary interactive terminal program there is, chosen
for the same reason the report program was.

**It works, and the first version of it cannot work.** The gap between those two
sentences is the finding.

## Pre-registration

Written before running anything. Five predictions; four held, and the fifth is
the one worth the note.

1. No ESC literal — the reader takes `\n \t \r \\ \"` and nothing else. **Held.**
2. No cursor or clear natives; the adapter abstracts input and not output.
   **Held.**
3. Raw mode has no scope form, so `try`/`finally` has to carry it, and will
   work. **Held.**
4. `io/stdout` is buffered until the program ends, which would make an
   interactive program unwritable. **Held, and it is the finding.**
5. No `mod`, only `rem`. Irrelevant here — a pager clamps rather than wraps.

## The finding: `io/stdout` does not reach the terminal until the program is over

`io/write` to `io/stdout` calls `vm.emit`, which appends to `vm.out`. `main.rs`
prints that buffer *after* `run_unit` returns, under the `--- stdout` header.
Nothing reaches the process's stdout while the program is running.

Measured rather than read. Two `println`s separated by 24 seconds of work
arrive at the same instant, at the end:

```
056.177  --- stdout
056.213  first
056.246  second
```

For a batch program this is invisible. For a terminal program it is fatal, and
the way it fails is worth seeing. Driven under a real pty, the pager was fed
`j j space k q` at 0.6-second intervals:

```
  0.63  KEY  'j'
  1.27  KEY  'j'
  2.00  KEY  ' '
  2.65  KEY  'k'
  3.37  KEY  'q'
  3.37  OUT  223b  '--- stdout\r\n<ESC>[2J<ESC>[H ... -- 1/61 -- ... -- 2/61 -- ...'
```

Five keystrokes against a blank screen, and then all five frames painted at
once, into a terminal that has already stopped caring. `term/read-key` blocks
**live** on the real tty for a keystroke the user was never shown a prompt for,
because the prompt is still in a buffer.

The program itself is correct. The frames say `1, 2, 3, 12, 11` — exactly what
`j j space k` should do against a ten-row window — raw mode is restored, and it
exits 0. Correctness was never the problem.

**The asymmetry is the shape of it.** Input is live and output is buffered. That
is not a missing feature in `term.rs`; it is two subsystems that were each right
on their own and have never been in the same program.

### It also gets the line endings wrong, for the same reason

The pager writes `\r\n`, which is correct under raw mode. The captured bytes are
`\r\r\n`. The buffer is flushed by `main.rs` *after* the `finally` has already
turned raw mode off, so the tty applies `ONLCR` to a newline that was never
supposed to get it. A program cannot write correct bytes for a terminal mode it
is guaranteed not to be in when the bytes are sent.

## The escape hatch, and what it costs

`io/write` takes a different path for `Host::File` than for `Host::Stdout` —
files get `write_all` on the real descriptor, and only stdout goes through
`vm.emit`. So opening the terminal *as a file* bypasses the buffer entirely:

```clojure
(def tty (io/open "/dev/tty" :write))
(def out (fn out [s] (io/write tty s)))
```

Two lines, and the same pager becomes a real interactive application:

```
  0.01  OUT      7b  '<ESC>[2J<ESC>[H'
  0.01  OUT    350b  '; A full-screen pager. Show one screenful of a file, ...'
  0.67  KEY  'j'
  0.68  OUT      7b  '<ESC>[2J<ESC>[H'
  0.68  OUT    329b  '; quit with q. It pages its own source, because ...'
```

It paints before it blocks, responds in about ten milliseconds, scrolls, clips
to the window width, restores the terminal on `q`, and the line endings are now
`\r\n` because raw mode is genuinely in effect when the bytes go out.

**The cost is that this is the `fs` feature.** `/dev/tty` is `io/open`, which is
`#[cfg(feature = "fs")]`. So a build with `term` and without `fs` is a terminal
you can *read keys from and cannot paint to* — the capability is half-present,
and nothing says so.

This is the third instance of one pattern, and it is starting to look like a
class rather than three accidents. ADR-046 recorded the first: the language's
only string→number path lived in the JSON adapter, so `--no-default-features`
removed a semantic. `just subtract` cannot see this one either — its three
points are everything off, everything on, and `fs` alone, so `term` without `fs`
is never built, and no test paints anything anyway.

## The tension this reaches, which is not a bug

The buffered host is not an accident to be removed. It is ADR-029's oracle:
emitted effects are part of the serialization round-trip comparison *rather than
escaping it*, and that property is the only reason constraint #2 is a property
instead of an aspiration.

`/dev/tty` escapes it. A program written the only way a terminal program can
currently be written is a program whose output the round-trip property cannot
see. And by ADR-043 part 5, a live `/dev/tty` handle is not reconstructible, so
such a program cannot be snapshotted at all — which is consistent, and is the
handle table doing its job, but it means the terminal workload and constraint #2
do not currently meet anywhere.

That is a decision, not a fix, and it is not in `ADR.md`. Filed as **Q33**.

## The smaller ones

**No ESC literal.** `\e`, `\u`, and `\x` are all unknown escapes. The sequence
is reachable — `(scalars-str [27])` — and every terminal program will write that
line. `scalars-str` existing is what keeps this a wart rather than a wall.

**No argv.** `main.rs` takes the command and the path and stops; a program has
no way to be handed an argument, and no way to read the environment. The pager
pages its own source because that is the only file it can name. Every terminal
program takes an argument.

**Raw mode has no scope form.** `term.rs` already documents this and calls it
the sharpest reason the terminal is an adapter. `try`/`finally` does carry it —
verified on the quit path — but `with-open` works on handles and a mode is not a
handle, so every program has to remember, and the one that forgets leaves the
user's shell unusable.

**A fresh pty is 0×0.** Not a language finding — it cost twenty minutes of
believing the pager rendered nothing, because `(take 0 …)` is an empty page and
clamping means there is no error to see. Worth knowing before the next person
drives one.

## What worked

**`recur` inside a `try` is flat.** This is the one worth writing down, because
ADR-028 rule 2 makes you expect the opposite: a call in tail position inside
`try`/`finally` is *not* a tail call, since the cleanup still has to run. A
terminal event loop is always inside a `try` — that is the only way raw mode
gets restored — so if `recur` inherited that rule, every terminal program would
have a bounded lifetime. It does not, and this is measured rather than assumed,
because a heap-allocated frame that grows would not announce itself by
overflowing: peak RSS is 3.42 MB at 100,000 iterations inside a `try` and
3.39 MB at 1,000,000. Ten times the work, no growth. `recur` is a jump within
the frame and never touches the handler stack.

**The standard library carried the whole program.** `split`, `take`, `drop`,
`map`, `join`, `str-scalar-len` — nothing was hand-rolled, which is the first
time that has been true. `take` and `drop` clamping instead of raising is
exactly right for scrolling: `(drop top lines)` past the end is an empty page
rather than an error, and the clamp at the top of the loop is the only bounds
logic in the program.

**The key map is right.** `(get k :char)` and `(get k :key)` dispatch without
the program learning anything about terminals, exactly as ADR-045 claimed.

Of 46 lines of code, four are the host shim — the ESC constant, the clear
sequence, the `/dev/tty` handle, and `out`. The rest is the pager. That ratio is
the news: `first-programs.md` was six of seventeen definitions, the report
program was 30 lines of 54, and this one is four. **The gaps have moved off the
language surface and onto the host boundary**, which is where ADR-048 through
ADR-050 were supposed to leave them.
