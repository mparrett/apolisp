# Milestone 10 — mutating the adapters

Tenth and last pass. First one run under milestone 9's new rule: **every
substitution asserts the old text was present before writing the new**, because
a pattern that no longer matches leaves a green suite and reads exactly like a
survivor.

Adapters are outside the line budget, which makes the question here different
from every previous milestone. Nothing in `src/adapters/` decides what a
program means, so a mutation there cannot break the language — it can only
break a conversion or a classification. Those are precisely the things a
corpus tends not to cover, because covering them means provoking a host into a
specific failure.

## Predictions

| # | Mutation | Predicted | Why |
|---|---|---|---|
| 1 | `from_json` drops the key sort | **survives** | `serde_json`'s default `Map` is a `BTreeMap`, so iteration is already sorted — the sort may be redundant |
| 2 | `to_json` emits `null` for `##NaN` instead of throwing | caught | the refusal test |
| 3 | `to_json` stringifies a keyword key instead of refusing | caught | the refusal test |
| 4 | `from_json` makes every number a `Float` | caught | `[1,2.5,-3]` would print `1.0` |
| 5 | `io/read` on a socket returns empty rather than reading | caught | the ping/pong test |
| 6 | `classify` maps `ConnectionReset` to `:other` | **survives** | nothing here provokes a reset |
| 7 | `classify` maps `TimedOut`/`WouldBlock` to `:other` | caught | the read-deadline test |
| 8 | `ms_err` drops the address, so `:path` is absent | **survives** | nothing checks `:path` on a socket error |
| 9 | `adapters::install` skips TCP | caught | the feature-matrix test |

Three predicted survivors, and 1 is the interesting one — not a hole in the
corpus but possibly a *redundancy*, which is the milestone-2 shape. If the sort
is unreachable because the library already sorts, the honest response is not a
test; it is deciding whether the line is guarding against something real.

## Results

Nine for nine, survivors included — third pass running where every prediction
held, and the third where writing the table down is what made the holes
visible.

| # | Mutation | Predicted | Actual |
|---|---|---|---|
| 1 | `from_json` drops the key sort | survives | **survived** — redundant, not untested |
| 2 | `to_json` emits `null` for `##NaN` | caught | caught |
| 3 | `to_json` stringifies a keyword key | caught | caught |
| 4 | `from_json` makes every number a `Float` | caught | caught |
| 5 | `io/read` on a socket returns empty | caught | caught |
| 6 | `classify`: `ConnectionReset` → `:other` | survives | **survived** |
| 7 | `classify`: `TimedOut`/`WouldBlock` → `:other` | caught | caught |
| 8 | `ms_err` drops the address | survives | **survived** |
| 9 | `install` skips TCP | caught | caught |

Run under milestone 9's rule: every substitution asserted the old text was
present before writing. Nine patterns, nine matches, nothing silently skipped.

### 1 — a redundancy, which is a different finding from a hole

The sort was unreachable. `serde_json`'s default `Map` is a `BTreeMap`, so
keys already arrive sorted and the line could be deleted with the whole suite
green — not because nothing tested the order, but because the library was
enforcing it too.

This is milestone 2's shape, and it gets milestone 2's answer: **delete the
redundancy, do not add a test.** Two mechanisms enforcing one rule means no
test can say which one is working, and a test written to pin the order would
have passed against a version with the sort removed *and* against one with the
library's guarantee removed.

What replaces it is a comment naming the dependency, because the guarantee is
now someone else's: enabling `serde_json`'s `preserve_order` feature swaps the
`BTreeMap` for an `IndexMap`, decoded objects arrive in document order, and
every golden holding one moves. Deterministic either way — but different, and
nothing in this repo would predict it.

### 6 — the kind a program most wants to retry on

`:connection-reset` could be mapped to `:other` with everything green. It is
the whole reason TCP is in this milestone: ADR-042 deferred three kinds on the
grounds that a kind nobody can raise is a guess, and shipping one nothing
provokes would have honoured the letter of that and not the point.

Provoked by closing the peer and writing: the first write lands in a buffer,
the peer answers RST, a later write fails. How many writes that takes is a
kernel detail, so the test tries a bounded number and asserts that one of them
failed the right way rather than asserting which one.

### 8 — the failure that did not say which peer

ADR-042 part 1 puts `:path` in a fault only when the operation names a
location. For a socket the address *is* the location, and dropping it survived
everything — a `:connection-reset` with no address names the failure and not
the thing that failed. Now pinned through a deterministically refused connect:
bind a listener, learn its address, close it, connect.

## Procedural

The mutator is a Python script that copies the file, asserts the pattern, runs
`cargo test --no-fail-fast`, and restores from the copy. No `git checkout`, no
stash — both have bitten this project before, and neither is needed when the
backup is a file. It is close to what `../reg-lisp`'s `verify.sh mutate` does,
and if this rung is ever made permanent that is the shape.
