# A developer's guide to apolisp

**Living document.** Unlike the write-ups in `docs/*.html`, which are frozen and
dated, this file is meant to track the language. If something here is wrong, it
is a defect — fix it. See ADR-064.

Every code block below was run against the binary on 2026-08-05. If you change
the language and a block here goes stale, that is the same class of problem as a
stale golden. Counts of things are deliberately absent — a tally nobody can act
on is the part that rots first, and `apolisp prelude` and `just lines` print the
current ones.

**What this is not.** A tutorial for learning Lisp, and not a reference — the
reference is `src/lib.rs`, which is one file and is meant to be read. This is
the shortest path from a clone to a program that does something.

**What apolisp is.** A small Lisp in the Clojure dialect with its own register
VM, in Rust. One user, no stability promise, no package manager, no ecosystem.
It exists to be a substrate for terminal applications, web services, and
simulators, and to stay small enough that one person and one language model can
hold all of it at once.

---

## Build and run

```
git clone https://github.com/mparrett/apolisp && cd apolisp
cargo build
just verify        # the full gate: fmt, clippy, tests, subtraction build, Value size
```

`just verify` is what has to be green before a commit. It takes a couple of
minutes. `just test` is the faster inner loop.

## The commands

```
apolisp run FILE.xs [args...]   # compile and run
apolisp repl                    # interactive session
apolisp read FILE.xs            # parse and print the forms back
apolisp spans FILE.xs           # forms with source positions
apolisp expand FILE.xs          # after macro expansion, before compilation
apolisp compile FILE.xs         # disassembly
apolisp prelude                 # disassembly of the built-in prelude
apolisp sizes                   # Value/Instr sizes against their ADR limits
```

`read`, `expand`, and `compile` are the language's own debugging surface: each
one stops the pipeline a stage earlier and prints what it has. Reach for
`expand` the first time a macro surprises you.

## Your first program

```clojure
;; hello.xs
(def greet (fn [who] (str "hello, " who)))
(println (greet "world"))
```

```
$ apolisp run hello.xs
--- stdout
hello, world
--- value
nil
--- exit
0
```

That three-part transcript is the whole output contract: what the program
printed, what the last form evaluated to, and the exit code. The goldens in
`tests/` are these transcripts, which is why the format is stable and boring.

## The REPL

```
$ apolisp repl
> (+ 1 2)
3
> (def xs [3 1 2])
[3 1 2]
> (sort xs)
[1 2 3]
```

The prelude is loaded, so `map`, `sort`, `when`, and `defmacro` are all there.
There is no readline: no history, no arrow keys, no tab completion. It reads a
form, evaluates it, prints the result, and repeats.

An error prints and the session continues:

```
> (+ 1 nope)
--- threw
{:type :vm-error :kind :unbound :message "`nope` is not bound"}
```

---

## The language in one page

**Literals.** `42` `3.5` `"text"` `:keyword` `true` `false` `nil`, vectors
`[1 2 3]`, lists `(1 2 3)`, maps `{:a 1 :b 2}`.

**Special forms** — the entire set:

```clojure
(set-global! name value)            ; bind a global — `def` is a macro over this
(set-cell! cell value)              ; write through a cell
(fn [a b] body)                     ; a function
(if test then else)
(let [a 1 b 2] body)                ; sequential binding
(do a b c)                          ; last value wins
(loop [i 0 acc []]                  ; the only looping construct
  (if (== i 3) acc (recur (+ i 1) (conj acc i))))
(try body (catch e handler) (finally cleanup))
(throw value)                       ; throw any value, not just an error type
(quote form)  'form
(quasiquote form)  `form            ; with ~ and ~@ inside
(set-macro! name)                   ; the macro primitive
```

Note what is *not* here: `def` and `defmacro` are prelude macros over
`set-global!` and `set-macro!`, not special forms. ADR-027 made that a decision
rather than an accident — run `apolisp expand` on any file and watch `(def x 1)`
become `(set-global! x 1)`.

`recur` is how you loop, and a self-call in tail position is optimised — but a
call in tail position inside a `try` with a `finally` is **not** a tail call,
because the frame is still needed to run the cleanup.

**Functions and macros.**

```clojure
(defn sq [n] (* n n))
(def anon (fn [n] (* n n)))          ; `defn` expands to exactly this
(defmacro unless [test body] `(if ~test nil ~body))
```

`defmacro` is written in the language, in the prelude, on top of `set-macro!`.

## Things that will bite you

The dialect is Clojure's, the surface is much smaller, and the gaps are where
you will lose your first hour. All of these are deliberate.

| You'll reach for | Reality |
|---|---|
| `(:key m)` | **Keywords aren't callable.** Write `(get m :key)`. |
| `(count "text")` | **Errors on purpose** — say the unit: `str-byte-len` or `str-scalar-len`. |
| `(/ 7 2)` | `3.5` — `/` is float division. Integer division is `quot`, remainder is `rem`. |
| `(map and ...)` | `and`, `or`, and `when` are **macros**, so they work in call position but are not values you can pass or print. |

The `count` refusal is the house style for the whole language: where a question
has two defensible answers, it declines to guess and the error message names
both. `"héllo"` is 6 bytes and 5 scalars, and the language will not pick one for
you.

## Data and the prelude

Natives (in Rust) cover the primitives: arithmetic (`+ - * / quot rem`),
comparison (`< <= > >= = == not=`), `not`, `str`, `count`, `first`, `rest`,
`nth`, `conj`, `get`, `assoc`, `dissoc`, `keys`, `vals`, `contains?`, `empty?`,
`concat`, `vec`, `vector`, `list`, `hash-map`, `vec-slice`, `compare`,
`gensym`, `cell`, `cell-get`, `println`, `parse-number`, the `str-*` and
`bytes-*` families, and `io/*`. That is the whole core set; `apolisp run` on an
unbound name will tell you so with a source position.

The host adapters add more, and they are on by default: `json/encode`,
`json/decode`, `tcp/connect`, `tcp/listen`, `tcp/accept`, `tcp/local-addr`,
`tcp/set-timeout`, `term/open`, `term/size`, `term/raw-mode`, `term/read-key`.
These sit outside the line budget (ADR-045) and each is behind a cargo feature,
so a subtracted build drops the names and keeps the language.

The prelude adds the rest, written in apolisp itself and compiled into every
unit. Macros: `def`, `defn`, `defmacro`, `when`, `unless`, `and`, `or`, `cond`,
`with-open`. Functions: `inc`, `dec`, `map`, `filter`, `reduce`, `range`,
`repeat`, `join`, `split`, `take`, `drop`, `sort`, `sort-by`, `sort-with`,
`merge-sorted`, `pad-left`, `pad-right`. Read it with `apolisp prelude`.

```clojure
(map (fn [n] (* 2 n)) [1 2 3])      ; => [2 4 6]
(filter (fn [n] (< n 3)) [1 2 3 4]) ; => [1 2]
(reduce + 0 [1 2 3])                ; => 6
(sort [3 1 2])                      ; => [1 2 3]
(range 4)                           ; => [0 1 2 3]
```

Note that `(rest [1 2 3])` gives the list `(2 3)`, not a vector.

## Errors

A thrown value is data, usually a map, and it carries a source position:

```
$ apolisp run broken.xs
--- threw
{:type :vm-error :kind :unbound :message "`nope` is not bound"}
--- at
broken.xs:2:6
```

Catch it and it's an ordinary value:

```clojure
(try (throw {:kind :boom}) (catch e (get e :kind)))   ; => :boom
```

`finally` runs on every path out, exactly once.

## Arguments and I/O

```clojure
(println *command-line-args*)     ; apolisp run f.xs one two  =>  ["one" "two"]
```

Always bound, to `[]` when there are none.

I/O goes through a handle table: `io/open`, `io/read`, `io/read-all`,
`io/write`, `io/close`, `io/read-dir`, `io/open?`. Host capabilities are
compile-time features, and `just subtract` builds the language with one cut out
to prove the seam is real. If you're adding a host capability, that harness is
the thing to run.

---

## Where to go next

Read in this order, and only as far as you need:

| | |
|---|---|
| `docs/ETHOS.md` | The four constraints and what gets sacrificed when they conflict. Short; read all of it. |
| `docs/TRAPS.md` | Semantic landmines. Read before touching equality, arithmetic, unwinding, or serialization. |
| `docs/ADR.md` | Every settled decision, with cost and rejected alternatives. Append-only. Check the status line before trusting a body. |
| `docs/BUILD.md` | The line budget and the milestone you're on. |
| `docs/QUESTIONS.md` | What is deliberately undecided. If your task needs one of these, it's a question, not a judgment call. |
| [The write-ups](https://mparrett.github.io/apolisp/) | Essays on the verification loop. Frozen, dated, and the best account of why any of this looks the way it does. |

If you are changing the language rather than using it, `AGENTS.md` (with
`CLAUDE.md` as a symlink to it) is the short version of the rules, and the first
one is that a decision not in `ADR.md` has not been made.
