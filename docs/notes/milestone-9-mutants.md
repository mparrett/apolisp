# Milestone 9 — mutating the session

Ninth pass. Milestones 7 and 8 both found holes in the corpus rather than
defects in the code, and both times the survivors were predicted because
writing the table down is what exposed them. So the question going in is the
same one: **which of ADR-044's promises does the suite only appear to check?**

Counted with `cargo test --no-fail-fast`, against a clean tree.

## Predictions

| # | Mutation | Predicted | Why |
|---|---|---|---|
| 1 | `compile_into` seeds an empty `Lower`, so indices restart | caught | a function from an earlier input names the wrong proto |
| 2 | `start_at` always starts at proto 0 | caught | every input after the first re-runs input 1 |
| 3 | `Session::eval` uses `expand_all` instead of `expand_in` | caught | the macro test, and the gensym test |
| 4 | `expand_in` resets the gensym counter | caught | the gensym test exists for exactly this |
| 5 | `Macros::with_prelude` skips the prelude | caught | `def` stops existing, so nothing runs |
| 6 | the reader's `.truncated()` dropped from the unclosed-string site | **survives?** | `wants_more` tests `"unterminated` — so caught, *if* that case is in the list |
| 7 | `truncated` set on *every* reader error | caught | the "wrong, not unfinished" half of the `wants_more` test |
| 8 | the driver never clears `buffered` after eval | caught | the second input would re-run the first |
| 9 | `wants_more` reads into the session's interner | **survives** | nothing looks at the symbol table after an abandoned line |
| 10 | `Session::eval` drops the throw and returns nil | caught | the throw test |
| 11 | a blank line is evaluated rather than skipped | caught | the blank-line test, which was written for this |
| 12 | `Ended::Threw` loses `suppressed` | **survives** | no session test throws from inside a cleanup |

Three predicted survivors, and 6 is a genuine open question rather than a
prediction — I put `"unterminated` in the `wants_more` list deliberately, so it
should die; I am recording it as uncertain because if it survives, the list is
decorative.

9 and 12 are the familiar shape: state the code handles that no test observes.
12 is the one that would actually bite — a `finally` that throws while
unwinding produces a suppressed chain, ADR-028 invariant 3 exists for it, and
the REPL prints it. Nothing in `tests/repl.rs` constructs one.

## Results

Ten mutants applied, one survivor, one that turned out not to be expressible —
and **two of the twelve did not apply at all**, which is the finding worth
keeping from this pass.

| # | Mutation | Predicted | Actual |
|---|---|---|---|
| 1 | `compile_into` restarts indices | caught | caught |
| 2 | `start_at` always uses proto 0 | caught | caught |
| 3 | `eval` uses `expand_all` | caught | caught |
| 4 | `expand_in` resets the gensym counter | caught | caught — *and* an `.expanded` golden |
| 5 | `with_prelude` skips the prelude | caught | caught |
| 6 | unclosed string not marked truncated | uncertain | caught — the list is not decorative |
| 7 | every reader error marked truncated | caught | caught |
| 8 | the driver never clears `buffered` | caught | caught |
| 9 | `wants_more` uses the session's interner | survives | **not expressible** |
| 10 | `eval` drops the throw | caught | caught |
| 11 | a blank line is evaluated | caught | caught |
| 12 | the prompt drops the suppressed chain | survives | **survived** |

### The procedural finding: a mutation that does not apply looks exactly like a survivor

Mutants 9 and 12 were first run through `perl -0pi` with a pattern containing
`\\\\n`, intended to match the literal backslash-n inside a Rust string. Under
single quotes the shell passes that through unchanged, so perl saw a regex
matching *two* backslashes where the file has one. Neither substitution fired.
Both runs reported a green suite, which is precisely what a surviving mutant
reports.

Nothing distinguishes the two outcomes from the outside, and the wrong one is
the flattering one: "the mutation survived" is a finding, "the mutation never
happened" is a wasted run being recorded as a finding. Caught here only because
12 was *predicted* to survive for a specific reason, and checking that reason
meant looking at the file.

**The rule this pass adds: assert the substitution happened.** Re-run under
Python with an `assert old in s` before writing, which fails loudly on a
pattern that no longer matches. Every earlier note in this directory was
produced with unchecked `perl -0pi` substitutions, and their survivor counts
should be read with that in mind — a survivor there may be a no-op.

### 12 — the suppressed chain the prompt threw away

Real, and verified twice: once against the suite as it stood, and again against
`git show HEAD:tests/repl.rs` after the fix, to check the fix was the thing
that killed it rather than something else that had changed.

ADR-028 invariant 3 says a cleanup that throws while unwinding wins and keeps
the error it displaced. A `.out` transcript prints both under `--- suppressed`.
The REPL printed the winner and dropped the chain, so `(try (throw :original)
(finally (throw :from-cleanup)))` reported `:from-cleanup` and lost `:original`
entirely — the *first* failure, which is usually the one worth reading.

Nothing in the suite constructed a suppressed chain. Third milestone running
where the survivor is a hole in the corpus rather than a defect in the code.

### 9 — the mutation the signature forbids

`wants_more` takes `&str` and nothing else; it builds its own throwaway
interner internally. Making it pollute the session's table is not a mutation of
a line, it is a change to the function's signature — so there is no edit that
introduces the bug while leaving the API alone.

That is the milestone-4 outcome rather than a survivor: the honest response to
a defect no test can observe is to remove the way to write it. Here it was
removed by accident, when the parameter was dropped for a different reason
(an abandoned line should not leave its symbols interned). Worth noticing that
the good property came from the smaller API rather than from foresight.

## Procedural

Clean tree throughout. `src/main.rs` and `tests/repl.rs` were saved to a file
copy and restored from it rather than stashed — the stash stack is repo-global
and shared across worktrees, so a pop can retrieve someone else's entry.
