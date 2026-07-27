# Milestone 8 — mutating the snapshot

Eighth pass. The interesting thing about this one before it ran: the round-trip
property is unusually strong — it cuts at *every* instruction boundary, over
nine programs, in two forms — so the honest question is not "does it catch
mutations" but "which pieces of state does the corpus of programs never put in
an `Image` in the first place". A field nothing exercises can be dropped from
the encoding and the strongest property in the project will not notice.

Counted with `cargo test --no-fail-fast`, against a clean tree.

## Predictions

| # | Mutation | Predicted | Why |
|---|---|---|---|
| 1 | `capture` drops `Vm::out` | caught | the transcript comparison *is* the buffered output |
| 2 | `capture` drops `gensym` | **survives** | no program in the list calls the `gensym` primitive at run time |
| 3 | `restore` drops `pending` | caught | "a parked unwind mid-cleanup" exists for this |
| 4 | `restore` drops the fingerprint check | caught | `an_image_refuses_a_chunk_it_was_not_taken_from` |
| 5 | the encoder ignores `seen`, so nothing is shared | caught | the sharing delta test |
| 6 | fuel never decrements | caught | `steps()` never terminates and trips its own guard |
| 7 | `capture` drops `free_handles` | **survives** | nothing opens a file, closes it, snapshots, and opens another |
| 8 | `capture` drops `handle_generations` | **survives** | same gap |
| 9 | `restore` builds a fresh interner instead of restoring names | caught | every printed keyword resolves through it |
| 10 | `-0.0` canonicalised to `0.0` on encode | **survives** | no program in the list produces a negative zero |

Four predicted survivors, all of the same kind: **state the encoder handles
correctly that no program in the corpus exercises**. None is a defect. That is
the prediction, and it is worth writing down before the run precisely because
"the property is strong" is the belief most likely to be doing the work here
instead of the property.

10 is the one to watch. ADR-032 exists because `##Inf` had to survive a round
trip, and milestone 6's first in-language run found a miscompilation where
`-0.0` and `0.0` shared a constant-pool entry. Negative zero has bitten this
project once already, and the snapshot corpus does not contain one.

## Results

Ten for ten, survivors included — the second pass running where every
prediction held. Same caveat as milestone 7's, and it is the whole value of
pre-registering: the four survivors were predicted because writing the table
down is when the holes became visible. Filled in afterwards, all four would
read as discoveries.

| # | Mutation | Predicted | Actual |
|---|---|---|---|
| 1 | `capture` drops `Vm::out` | caught | caught — three tests |
| 2 | `capture` drops `gensym` | survives | **survived** |
| 3 | `restore` drops `pending` | caught | caught |
| 4 | `restore` drops the fingerprint check | caught | caught |
| 5 | the encoder ignores `seen` | caught | caught — the sharing delta only |
| 6 | fuel never decrements | caught | caught |
| 7 | `capture` drops `free_handles` | survives | **survived** |
| 8 | `capture` drops `handle_generations` | survives | **survived** |
| 9 | `restore` builds a fresh interner | caught | caught |
| 10 | `-0.0` canonicalised on encode | survives | **survived** |

**The strongest property in the project caught none of the four.** It cuts at
every instruction boundary, over nine programs, in two forms — and a property
can only see the state its programs create. Every survivor is a field the
encoding handles correctly and the corpus never populated. That is worth
naming as a limit of the rung, alongside the milestone-6 one about performance
claims: *a round-trip property tests the encoding of whatever the corpus
happens to construct, and nothing about what it does not.*

### 2 — the counter a program can advance

`gensym` is a primitive, so the counter is live at run time and not only during
expansion. No program in the list called it. A resume that restarted the
counter reissues a name it has already handed out, which is the one thing a
fresh symbol may never do — and every `.expanded` golden would still be green,
because expansion happens before the run.

### 7 and 8 — the handle table's arithmetic

Both were reasoned about when the encoding was written, and both were carried
deliberately, for a scenario the corpus did not contain: open a file, close it,
snapshot, open another. The second open reuses the freed slot under a bumped
generation, and that id reaches the transcript. Dropping the free list prints
`2:0` where the uninterrupted run printed `2:1`; dropping the generations
indexes past the end of a table restore had just shortened.

The existing test used *one* `with-open`, so no slot was ever reused. Making it
two closed both holes. Worth noting that this is code written *because* the
gap was anticipated, sitting untested for exactly the reason it was written.

### 10 — negative zero, again

Milestone 6's first in-language run found `-0.0` and `0.0` sharing a
constant-pool entry, so `(/ 1.0 0.0)` could produce `##-Inf`. ADR-032 exists
because float spellings have to survive a round trip. And the snapshot corpus
contained no negative zero, so an encoder that normalised the sign away passed
everything.

The fix computes it — `(* -1.0 0.0)` — rather than writing the literal, so the
test does not depend on the reader, and checks it through `(/ 1.0 z)`, where
the sign is the difference between `##-Inf` and `##Inf`. A test that only
printed `z` would pass against an encoder that stored an `f64` instead of its
bits.

## Procedural

Clean tree throughout; only `src/lib.rs` was ever reverted, and the working
copy of `tests/snapshot.rs` was verified present after each pass rather than
assumed. Mutant 6 needed a timeout — fuel that never decrements makes the step
counter run to its own guard rather than hang, but the guard was added for a
different reason and it is luck that it was there.

Nothing in `tests/lang/` participates in this milestone: an `Image` is not
reachable from the language, so rung 4 has no opinion about it. Second
milestone running where a Rust test is the only rung that can hold the claim.
