# The corpus as an oracle — checks that were true about the wrong thing

**Not normative.** Working notes on the checking loop, not on the language.

Two properties that swept `corpus_files()` were green for three days while
checking bytecode the VM never runs. This is the second time a corpus-wide
property in this project has been dead, and the two were found by different
means, which is the part worth recording.

**Six instances now**, four of them from the session that produced this note.
They are tabulated under *Six instances, and a third channel* below, along with
the shape they share: every one was a correct assertion applied to the wrong
subject, and reviewing an assertion does not read its subject. Start there if you
are looking for the general claim rather than this particular incident.

## What happened

`tests/compile.rs` has a helper, `compile_ok`, that reads a source string and
compiles it. It does not expand. That is the right shape for the hand-written
snippets that fill most of the file — they are core forms, and routing them
through the expander would test the expander instead of the compiler.

Two tests, `every_instruction_has_an_origin` and
`slot_operands_stay_inside_the_frame`, used the same helper to sweep every file
in `tests/corpus`. Since milestone 5 those files contain macros, so from
`6c4785f` (2026-07-27) onward both properties were compiling forms that no
longer resembled what the compiler is handed at run time.

Not a weakened check — an absent one. The bytecode being asserted over was
produced from pre-expansion source, so nothing about post-expansion codegen was
covered by either property, for any program, ever.

## Why nothing noticed

`macros.xs` entered the corpus in the same commit that made expansion exist, so
the trigger and the thing that should have fired it arrived together. It did not
fire, because every template in `macros.xs` expands to something the compiler
*also* accepts unexpanded:

```
(defmacro when  [test & body] `(if ~test (do ~@body) nil))
(defmacro twice [e]           `(let [v# ~e] (+ v# v#)))
(defmacro pair  [a b]         `[~a ~b])
```

Unexpanded, `` `(if ~test ...) `` is a list whose head is `if` and whose members
are `(unquote test)` — odd, but structurally a form the compiler will lower.
`twice` has a vector in binding position, but it is a *literal* vector that
survives syntax-quote as a vector.

Breaking it needs an unquote standing where a **parameter vector** goes, and
only one macro in the project does that:

```
(defmacro defn [name params & body]
  `(def ~name (fn ~name ~params ~@body)))
```

Unexpanded, `~params` is `(unquote params)` — a list — and the compiler says
`` `fn` takes a parameter vector, not a list ``. `editor.xs` is the first corpus
program to define `defn`, and it failed on the first run after being added.

## The two detection mechanisms

Milestone 1's span property was dead in the same way: green over the whole
corpus, checking arity and nothing else, with two of three mutants passing the
entire suite (`milestone-1-pilot.md`). That one was found by **deliberate
mutation** — sitting down and trying to break the test.

This one was found by **adding an input unlike the existing ones**. No mutation
would have caught it, because the code was not wrong: `compile_ok` does exactly
what it says, and every assertion in both properties was correct. The defect was
in the *reach* of the property, and reach is invisible to mutation of the code
under test. It only becomes visible when the input set grows a member that
exercises a path the others do not.

So the two habits are complements, not substitutes:

- Mutation finds properties that assert too little about the programs they see.
- New unlike inputs find properties that see too few programs.

"Holds over the whole corpus" is a claim whose strength is set by the corpus's
diversity, and nine programs that each pin one small feature are not diverse
merely by being nine. The editor is 270 lines against 212 for the other nine
combined, and it broke something on the way in.

## Six instances, and a third channel

*(Added 2026-07-30. The section above was written after the second instance. Four
more arrived in the same working session, which is the reason this one exists —
two data points are an anecdote and six are a property of how the work is done.)*

| # | The check | How it was found | What it would have hidden |
|---|---|---|---|
| 1 | Milestone 1's span property | mutation | every origin `Unknown`, every span starting at byte 0 — two of three mutants passed the whole suite |
| 2 | `every_instruction_has_an_origin`, `slot_operands_stay_inside_the_frame` | an unlike input | all post-expansion codegen, for every program, since milestone 5 |
| 3 | `str-scalar-slice`'s separate `from > to` check | mutation | nothing — and that was the finding |
| 4 | The editor's two-row pin | mutation | the `max2 1` clamp, the entire bug the fix was for |
| 5 | `the_language_carries_no_dependencies` | asking what it observed | every dependency, if the manifest were ever reformatted |
| 6 | The editor shell's compile check | mutation | a shell calling a core function that no longer exists |

**Every one of these was a true assertion about the wrong subject.** Not one was
a wrong assertion. #2's bounds checks were correct, applied to bytecode the VM
never runs. #4's assertions were correct, at a height where the clamp cannot
fire. #5's "every dependency is optional" is correct and vacuous over an empty
list. #6 compiled exactly what it claimed to compile, and compiling is not what
catches an unbound global in a language that resolves them at call time.

A green test is a claim about a pair — the assertion, and the subject it was
applied to. **Review reads the assertion. Nothing reads the subject.** That is
the whole of it, and it is why all six survived being written and read by someone
who understood them.

### The third channel

Instance 5 was not found by mutation or by a new input. It was found by printing
what the parse actually saw before trusting the assertion built on it — two
dependency lines, which is the number that makes the check mean something.

That is cheaper than either other channel and it is the only one that works
*before* a test has ever been green for the wrong reason. Mutation needs the code
to exist and a hypothesis about how it might break; an unlike input needs the
input. Asking a check to show its working needs a `println` and thirty seconds.

- **Mutation** finds a check that asserts too little about what it sees.
- **An unlike input** finds a check that sees too few things.
- **Asking what it observed** finds a check that is looking at nothing at all.

### A survivor does not always mean "add an assertion"

Instance 3 is the one that does not fit the pattern, and it is worth keeping for
that. `str-scalar-slice` had a `from > to` branch of its own, and removing it
changed no behaviour any test could see — the scan reaches the same error either
way, and ADR-039 clause 3 says the message is prose rather than contract. The
naive reading of a survivor is "the test is too weak". Here the correct reading
was "the code is redundant", and deleting it was the fix.

Deleting it then exposed something a stronger test would not have: without the
loop's early `break`, `s[f..t]` runs backwards and **panics**, which is a process
abort where ADR-039 requires a throw. That is now held by an `f <= t` guard
rather than by an argument about loop ordering. A survivor is a question about
which of the two is wrong, and answering "the test" by reflex would have kept a
redundant branch and missed a real one.

### The cheap part is when

Four of the six were caught by mutation, and none of the four took five minutes.
Three of the six were caught in the same session the check was written — before
the thing had ever been trusted. The habit costs least at the moment the
assertion is written and most after it has been green for three days, which is
exactly backwards from when it feels necessary.

## The comment that named its own trigger and did not fire

The assertion carried this, from milestone 2:

> Nothing in the corpus is macro-generated yet, so every instruction traces to
> real source text. When milestone 5 lands, `Generated` becomes legal here and
> this assertion is the thing that has to be widened deliberately.

That is a correct prediction, with the right trigger, sitting at the right line
of code. Milestone 5 landed **the next day**. The comment did not fire, because
a comment has no mechanism to fire — it is read by whoever is already looking at
the line, and after milestone 2 nobody was.

Worth pairing with the milestone-1 finding that ADR-026 had *already specified*
the fix for the dead span property in its verification list. Twice now the
project has written down the thing that would have prevented a defect, in a
place that was not consulted at the moment it mattered.

## What was changed

`compile_expanded` mirrors `apolisp compile` — read, expand, compile — and the
two corpus sweeps use it. The prelude stays out; `compile_unit` would fold it in
and the span assertion would then be checking prelude origins against the
program's source length.

The origin property now accepts `Generated`, which is the widening the comment
asked for. `Unknown` is still refused. One assertion was added that at least one
instruction per program traces to `Source`, because legalizing `Generated`
without it would let a mutant that stamped every instruction `Generated` pass —
the exact failure the milestone-1 note is about, reintroduced by the fix for a
different one.

## Questions for the loop

1. Should a corpus entry have to state what it reaches that no existing entry
   does? The `.out` list is already an asserted decision; coverage intent is
   not, and "adding an unlike input" only works if unlikeness is deliberate.
2. Is there a cheap check that a corpus-wide property is compiling through the
   same pipeline as the binary? Both defects here were a test helper diverging
   from `main.rs`, and that divergence is mechanically detectable.
3. Comments that name a future trigger have now failed twice. Should a trigger
   be a failing test with an `#[ignore]` and a reason, so that it is the suite
   asking rather than the reader remembering?
4. Milestone 1 asked whether mutation should be a standing rung rather than Q18.
   This adds a second question next to it: should *corpus growth* be scheduled,
   rather than happening when a program is written for other reasons?
5. Six instances is enough that "kill one mutant before trusting a new check"
   is a habit with evidence, not a preference. Does it belong in `CLAUDE.md`'s
   habits list, which currently says *try to break your own test* on the strength
   of one? The stronger claim the six support is narrower and more actionable:
   **do it at the moment the check is written**, because three of the six were
   caught that way and the other three had already been believed for days.
6. Instance 5 was found by printing what a parse observed, which no rung covers
   and which cost thirty seconds. Is "show the check its own working before
   trusting it" a third habit, or just what mutation looks like when the code is
   too young to mutate?
