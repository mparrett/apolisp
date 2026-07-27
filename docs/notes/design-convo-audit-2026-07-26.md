# What the original design conversation said, and what survived

**Not normative.** An audit of `archive/lispy-language-vm-convo-2026-07-25.md`
against the decisions that came out of it, done at `d7bd707` so nobody has to
re-read 1,840 lines to find out whether an idea was rejected or just lost.

The conversation predates SPEC v0.1 and every ADR. It is where the four
constraints, the closed core, flat closures, the slot VM, the handle table, and
the crate-delegation list were first argued. Most of it landed.

## Landed, traceably

Nearly everything structural. The closed-core/open-macro split (ADR-007), scoped
reader configuration (ADR-008), the slot VM with monotonic allocation and the
exact `CONST r0 / ADD r2, r0, r1` sketch (ADR-006), flat closures with the
mutual-recursion carve-out (ADR-002), `&mut Vm` instead of `Rc<RefCell<_>>`
everywhere (ADR-020), the three layers of mutation (ADR-020), asserting
`size_of::<Value>()` rather than assuming 16 bytes (ADR-010, ADR-025 — and the
conversation's suggested `<= 24` is the number in the code), the
Ready/Pending/Error host protocol (ADR-017), text/bytes/buffer as three things
(ADR-018), the dependency list down to the individual crates and the "would
implementing this teach us something about our language" test (ADR-014), and the
one-file-with-inline-mods progression (ADR-015).

Two of the four operational criteria it proposed adding — determinism and
inspectability — became `BUILD.md`'s spine rather than ADR entries, which is the
right home for them.

## Deliberately overruled

- **Feature-gating language facilities** (macros, exceptions, metadata as Cargo
  features, with "minimal scripting" and "embedded runtime" profiles). ADR-013
  overrules this: gating semantics turns one system into 2ⁿ systems. The
  conversation half-argued itself out of this too — its own "important
  architectural constraint" section says to gate coarse layers, not branches.
- **A workspace of four crates.** ADR-015's progression stops at the first level
  that suffices; no deployment boundary has asked yet.
- **A generated amalgamated single-file reading view.** ADR-015 rejected it as
  unnecessary while the source is one file.
- **`logos` for tokenizing.** ADR-014 rejected it — a generated lexer fights
  character-level reader-macro dispatch.

## Lost rather than decided

Three things the conversation raised that no entry accepts, rejects, or parks.
Now filed:

1. **A `Char` value variant.** Recommended explicitly — "it gives reader and
   Unicode APIs a clear scalar-value type." ADR-025 froze the enum without one
   and without mentioning it. Q20 lists "characters" among the undecided
   surface, so the question exists; what was missing is that answering it "yes"
   now means superseding an ADR whose size is asserted. Noted in Q20.
2. **Deterministic simulation services** — seeded RNG, virtual clock, explicit
   injection of nondeterministic inputs, replay. Simulators are one of three
   named target workloads and this appears once in the decisions, as "RNG" in a
   list of gateable capabilities. Opened as **Q22**, because it is not only a
   simulator concern: the serialization round-trip property compares a resumed
   transcript against an uninterrupted one, and an unseeded RNG or wall clock
   makes those differ for reasons unrelated to the snapshot. That is the oracle
   for constraint #2 flapping, which `BUILD.md` says is how oracles die.
3. **Structured error values with a closed taxonomy of kinds.** The conversation
   gave a concrete shape and a nine-kind vocabulary, and argued errors should be
   data with the raw host code as metadata rather than formatted strings.
   `ETHOS.md` puts error quality outside the priority ranking and ADR-014 calls
   error semantics never-delegated, but nothing says what an error *is*. Opened
   as **Q23**; milestone 4 forces it, since a `.out` transcript cannot pin a
   thrown value of undecided shape.

## The pattern worth noticing

All three losses are **things the conversation raised once, in passing, inside a
long answer about something else**. `Char` is one sentence in a section about
strings and I/O. The simulation services are one bullet in a list of possible
future crates. The error taxonomy is item 10 of twelve. Nothing that got its own
heading was lost.

That is a cheap rule for the next long document: the ideas that need a home are
the ones that arrived as asides.

`milestone-1-pilot.md` found that ADR *code sketches* did not survive first
contact while the prose did. This is the complementary failure — prose that
never became a sketch, an entry, or a question at all. Both are failures of
transcription rather than of reasoning, and both were found by going back and
checking rather than by reading forward.
