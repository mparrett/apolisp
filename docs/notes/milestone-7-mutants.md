# Milestone 7 — mutating the handle table

Seventh pass. Same shape as the six before it: pick the lines that are supposed
to be load-bearing, break each one on a clean tree, and record whether anything
goes red. Predictions written before the first run; the refutations are the
part worth keeping.

Counted with `cargo test --no-fail-fast`, because it stops at the first failing
*binary* and silently under-reports otherwise (the milestone-4 scar).

## Predictions

| # | Mutation | Predicted | Why |
|---|---|---|---|
| 1 | `open_handle`: drop the generation bump on reuse | caught | the whole stale section of `io.xs`, plus `tests/vm.rs` |
| 2 | `close_handle`: drop the staleness check | caught | `(is (throws? (io/close stale)))` and the `vm.rs` assert |
| 3 | `close_handle`: push to the free list unconditionally | caught by `vm.rs` only | no language program can see a slot queued twice |
| 4 | `with-open`: close on the normal path only | caught | `io.xs` paths 2 and 3 |
| 5 | `host_mut`: drop the generation check | **survives** | nothing reads or writes *through* a stale handle |
| 6 | `fault_value`: emit `:path nil` instead of omitting the key | **survives** | `(get e :path)` answers nil either way — the test cannot see the difference it was written for |
| 7 | `classify`: map every host error to `:other` | caught | `(is= :not-found ...)` |

Two predicted survivors, and they are different in kind. 5 is a hole in the
corpus — a path that exists and is never walked. 6 is worse and is the
milestone-1 pattern again: a test written specifically to pin "the key is
absent" that cannot distinguish absent from present-and-nil, so it passes
against the thing it was meant to forbid.

## Results

Seven for seven, survivors included. That is the first pass where nothing was
refuted, and it is not the good news it looks like — the two predicted
survivors were predicted because writing the prediction down is when I noticed
the holes. Had the table been filled in afterwards, both would read as
discoveries.

| # | Mutation | Predicted | Actual |
|---|---|---|---|
| 1 | drop the generation bump on reuse | caught | caught — `io.xs` and `tests/vm.rs` |
| 2 | drop the staleness check in `close` | caught | caught — both |
| 3 | push to the free list unconditionally | `vm.rs` only | caught — `vm.rs` only, exactly |
| 4 | `with-open` closes on the normal path only | caught | caught — `io.xs` |
| 5 | `host_mut` ignores the generation | survives | **survived** |
| 6 | `:path nil` instead of an absent key | survives | **survived** |
| 7 | `classify` collapses to `:other` | caught | caught — `io.xs` |

### 5 — the read path was never walked with a dead handle

The suite tested `io/close` on a stale handle and `io/open?` on one, and both
of those go through `Vm::host`. Nothing read or wrote *through* a stale id, so
`Vm::host_mut` — the function every actual io operation uses — could ignore the
generation entirely and stay green.

This is the aliasing bug ADR-016 put a generation in the id to catch, sitting
on the path a program is overwhelmingly more likely to walk, and the corpus
covered the rarer one instead. Fixed by opening a handle, closing it, opening a
second that takes its slot, and reading through the first: the mutant returns
the second file's contents.

### 6 — a test that could not fail against the shape it forbade

ADR-042 part 1 says `:path` is present only when the operation names one. The
assertion written for it was `(is (= nil (get read-failure :path)))`, and `get`
answers `nil` for an absent key *and* for a key present with a nil value. So
the entry's own decision — that a nil path is a key that says nothing and is
therefore not carried — had a test that passed either way.

This is the milestone-1 pattern for the second time: a test written to pin a
rule, sitting next to a mechanism it cannot observe. Fixed with `contains?`,
plus a `count` on both shapes so the key set is pinned as a whole rather than
one key at a time.

**Both are corpus holes rather than defects**, which makes this pass different
in kind from milestones 2, 4, and 6. Nothing in the implementation was wrong.
What was wrong was the belief that the suite was checking it.

### An aside from mutant 4

Rewriting `with-open` to use a gensym flipped `expanded_snapshots_match` on
corpus programs that never call it. The prelude's gensym counter runs while the
prelude expands, so *adding a gensym anywhere in the prelude* renumbers `v#` in
`and` and `or` for every unit after it. The shipped macro needs no gensym, so
no golden moved — but the next prelude macro that wants one will move all of
them, and that will look like a regression rather than renumbering.

## Procedural

The clean-tree rule held: only `src/lib.rs` and `src/prelude.xs` were ever
reverted, and both were committed before the first mutant ran. Counted with
`cargo test --no-fail-fast` throughout.

Sixth pass in a row where `tests/lang/` is the only rung that catches a
*semantic* mutation, and the second where a Rust test in `tests/vm.rs` is the
only thing that catches an invariant no program can observe (mutant 3). The two
rungs are not redundant and the milestone-6 note's conclusion needs that
qualifier.

