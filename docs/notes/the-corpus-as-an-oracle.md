# The corpus as an oracle — two dead properties, found two different ways

**Not normative.** Working notes on the checking loop, not on the language.

Two properties that swept `corpus_files()` were green for three days while
checking bytecode the VM never runs. This is the second time a corpus-wide
property in this project has been dead, and the two were found by different
means, which is the part worth recording.

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
