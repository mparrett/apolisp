# `loop`/`recur` as a macro — the attempt Q5 asked for

Q5 says: attempt it as a macro over the core forms, and admit a fourteenth
special form **only on evidence from a real attempt**. This is the attempt. It
works, and what it cannot do is the finding.

## The macro

No tree walk. `loop` names its function; `recur` expands to a call to that name.

```clojure
(defmacro loop [bindings & body]
  (let [evens (fn evens [xs]
                (if (empty? xs) '() (conj (evens (rest (rest xs))) (first xs))))
        odds (fn odds [xs]
               (if (empty? xs) '() (conj (odds (rest (rest xs))) (first (rest xs)))))]
    `((fn recur-target [~@(evens bindings)] ~@body) ~@(odds bindings))))

(defmacro recur [& args] `(recur-target ~@args))
```

The obvious implementation walks the body rewriting `recur`, so that `loop` can
gensym the name. Not walking it is what makes this eight lines — and **nesting
then falls out of lexical scope for free**. An inner `loop` shadows the name, so
an inner `recur` reaches the inner loop. That is Clojure's rule, arrived at by
not implementing it.

## What it does

Verified by running it:

- **Constant space.** 200,000 iterations complete. `recur` in tail position is a
  self-call in tail position, which ADR-028 already made a tail call.
- **Nesting is correct.** A `loop` inside a `loop`'s `recur` argument reaches the
  right one.
- **Arity is checked**, by the ordinary callee-side check.
- **`recur` inside `try`/`finally` completes.** ADR-028 rule 2 says that is not a
  tail call, so frames accumulate; 5,000 iterations survive it. This is Q5's
  "reject that shape or accept the frame" — it accepts the frame, and behaves
  exactly as a hand-written self-call in the same position does.
- **Life goes from 17 top-level definitions to 12.** The five that disappear are
  the five that were never about Life (`notes/first-programs.md`).

## What it cannot do

Three defects. All three are diagnostic, which in this project is not a mild
category — `ETHOS.md` puts error quality outside the priority ranking entirely,
on the grounds that it is the feedback loop everything else depends on.

**1. Errors name an identifier the user never wrote.**

```
(recur 1)          → :unbound  "`recur-target` is not bound"
(loop [i 0] (recur 1 2)) → :arity "`recur-target` takes 1 argument(s), given 2"
```

The second is the common mistake — a `recur` whose arity drifted from its
bindings — and it reports a name that appears nowhere in the source.

**2. Non-tail `recur` is silently accepted.** `(loop [i 0] (+ 1 (recur ...)))`
compiles and grows the stack until it runs out. Clojure rejects this at compile
time, and that diagnostic is how most people learn where tail position *is*. A
macro cannot produce it: a macro does not know its own position.

**3. It captures a user binding of the same name.**

```clojure
(let [recur-target 99]
  (loop [i 0] (if (= i 2) [i recur-target] (recur (+ i 1)))))
;; => [2 #<fn>]
```

The loop's function shadows the user's binding, silently. A gensym would fix it
and a gensym is unavailable, because `loop` and `recur` expand separately and
cannot share one.

## What fixing them would cost

All three are fixed by the same thing: `loop` walks its body. That buys a
gensymmed name (defect 3), a rewrite so `recur-target` never appears (defect 1),
and a tail-position check (defect 2).

The walk has to know which positions are tail positions in `if`, `do`, `let`,
`try` and `fn`. **The compiler already knows this** — it is what ADR-028 is
implemented as. Writing it again in the prelude puts the definition of "tail
position" in two places, in two languages, with no test that they agree, and the
failure mode when they drift is a `recur` the macro accepts and the compiler does
not make a tail call. That is a silent stack leak in the one construct people
reach for to avoid one.

So the choice Q5 poses is not "macro or special form" on ergonomics. It is:

- **Macro**, eight prelude lines, correct semantics, three diagnostics that
  cannot be fixed without duplicating the compiler's tail-position analysis.
- **Fourteenth special form**, which puts `recur` where tail position is already
  decided and makes all three diagnostics fall out — at the cost of a core form,
  in the layer that is already the largest (`compile`, 942 lines).

The macro is not a cheap approximation of the special form. It is a complete one
that is missing exactly the errors, and the errors are the part this project
says it will not trade away.

## Outcome

**Core forms, by ADR-047.** The implementation is the shape this macro found:
`loop` is a `let` around an immediate call to an anonymous function, and `recur`
is a tail call to it. What the compiler adds is not a different mechanism but the
refusals — and, because the loop *is* a function, "tail position for `recur`"
turned out to be the flag the compiler already had, so the second definition of
tail position feared above never had to be written.

One thing the macro could not have reached: a `recur` from a `catch` is
**allowed**, because this VM pops a handler record when it dispatches. That falls
out of the region counter rather than being decided, and no macro could have
consulted it.

## Note for whoever implements it

The macro uses no gensym, so adding it to `prelude.xs` does **not** trigger the
`.expanded` golden renumbering `TRAPS.md` warns about. That trap applies to a new
prelude macro using `x#`; this one uses none.
