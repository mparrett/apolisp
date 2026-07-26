# Internal review — SPEC v0.1

**Reviewing:** `docs/apolisp-spec.md`, Draft v0.1
**Date:** 2026-07-25
**Scope:** whole document; correctness, internal consistency, and gaps that get
expensive later. Not a copy edit.

---

## Verdict

Strong, and unusually so for a draft. The constraints are stated *as*
constraints, with one named as binding. The ADRs carry costs and rejected
alternatives rather than just conclusions. Phase 1 correctly identifies that
printability and testability are the same feature, which is the observation the
whole verification story rests on.

Parts I–III would survive being handed to an implementer cold. The findings
below are ranked by how much it hurts to discover them late, not by how wrong
they are.

---

## Tier 1 — would change the design

### 1. Forms vs. Values — the homoiconicity hole

`form.rs` and `value.rs` are separate modules. `Value` has no `Form` variant and
no metadata slot. But macros are language functions that run in the VM (ADR-004
even provides for macroexpansion trampolining), so a macro receives forms *as
values* and returns them.

Two ways out, and the spec picks neither:

- Forms **are** Values — then ADR-009's "every form carries metadata" has to
  live somewhere in that enum, costing a variant or a side table keyed by
  identity.
- Forms are converted to Values at every macro boundary — then that conversion
  is precisely where source spans die, which defeats ADR-009's stated purpose.

This is the largest unresolved item in the document and it is load-bearing for
ADR-009, for error quality, and for hygiene. It also gets expensive to change
once the reader and expander exist.

### 2. Keywords do not exist

The `Value` enum has no `Keyword`. The spec's own host-interface example is
`(io/open path :read)`. In a Clojure dialect, keyword-keyed maps are the single
most common idiom in the language.

Decide: separate variant, or interned symbols with a flag bit. Note it interacts
with the semantic-trap checklist entry on symbols vs. strings.

### 3. ADR-011 never names the representation

The ADR rejects HAMT + RRB but does not say what the one representation per
collection type actually is. That matters more than usual here, because ADR-012
removes laziness and makes reduce-into-a-collection the default idiom: if map is
an assoc-vec with copy-on-`assoc` and there are no transients, building a map in
a fold is O(n²).

Name the representation, and say something about transients even if the answer
is "accept it until profiling names the pressure."

### 4. Tail calls are never mentioned

No laziness means iteration is recursion. `loop`/`recur` as a macro (Part V, open
question 2) lowers to a self-call. Under ADR-004 the frame stack is a VM-owned
`Vec`, so frame reuse on a tail call is nearly free — but it has to be an actual
decision.

Open question 2 cannot be settled without this one. Resolve tail calls first.

### 5. "VM state is plain data" hides sharing and cycles

Two specific consequences ADR-005 doesn't address:

- **Sharing.** `Rc` graphs mean the canonical encoding must preserve DAG
  structure. A naive tree walk gives exponential blowup on shared structure and
  breaks identity semantics for cells and atoms.
- **Cycles.** ADR-003 explicitly permits cell cycles ("accept the leak"). A tree
  walk does not terminate on one.

Related: `SymId` identity is intern-order dependent. Deserializing into a *fresh*
VM works because the intern table travels with the snapshot; resuming into a
*populated* VM requires remapping. State the fresh-VM-only restriction for v1
rather than leaving it implied by the property test's wording. This connects to
open question 6.

### 6. What does milestone 8 suspend *on*?

v1 ships blocking I/O only, so there is no `Pending` to suspend at. The
serialization round-trip property test therefore has no suspension point unless
one is built for it.

The natural answer — a step limit that suspends at an arbitrary instruction
boundary — is free under ADR-004 and produces a far stronger test than
suspending at designated points. Say so explicitly, or the property test quietly
degrades into "suspend at the one place we built for it."

---

## Tier 2 — consistency and clarity

- **Two priority orders, adjacent, both called priority.** Governing constraint
  #4 (correctness > simplicity > efficiency > scale) and the four-item ranking
  (line count > serializable > speed > ergonomics) are different axes, but they
  appear back-to-back and the reader has to work that out. Part IV then repeats
  the first. Label them distinctly or fold them together.

- **The pipeline diagram implies a linear pure function.** It isn't: macro
  expansion runs user code, so compilation requires a live VM and the compiler is
  re-entrant with it. Fine as a design, but an implementer should learn it from
  the diagram rather than from a borrow-checker fight in week three.

- **No `def`.** Core forms include `global` (a reference) but nothing that
  creates one. Presumably it lowers to a cell write — say which, since it
  interacts with hot redefinition and with what "module-level bindings" means in
  the recursion exception.

- **No namespace / module design.** `global`, "modules" in VM-owned state, and
  namespace-qualified symbols for hygiene all assume a namespace system. It has
  no ADR and no row in the line budget. For a Clojure-alike this is a subsystem,
  not a detail.

- **The Babashka differential oracle is weaker than claimed.** No laziness, no
  bignum promotion, and (currently) no keywords means the overlapping subset
  shrinks fast. Probably still worth it for arithmetic and core-fn edge cases —
  but scope it explicitly, or it reads as a general oracle that it isn't.

- **"Null collector" is a misnomer.** There is no collector. To get the win you
  skip refcount traffic entirely, which is a different `Drop` story for `Value`,
  not just "never reclaim."

- **Nothing enforces the line budget.** Constraint #1 is the binding constraint
  and success criterion #1, and it is the only one with no test. A per-module
  line-count assertion is roughly twenty lines and promotes the budget table into
  an oracle rung.

---

## Recommended order of attack

1. **Forms vs. Values (Tier 1.1)** — most expensive to change after the reader
   and expander exist.
2. **Keywords (1.2)** and **tail calls (1.4)** — small decisions now, and both
   quietly constrain milestones 1–3.
3. Everything else can ride until the pilot.
