# Open questions

Undecided. When one resolves, it becomes an ADR and leaves this file. Sources:
SPEC v0.1 Part V, plus the spec review of 2026-07-25.

---

## Blocking now

**Q1 — Reader table scope in the REPL.**
ADR-008 freezes reader config per file. Is a REPL *session* its own parse unit,
with a freely mutable table? Proposed: yes — mutate freely at the REPL, declare at
the top of a file. Nothing snapshots a REPL session, so the oracle is unaffected,
and it is smaller than the current rule. The alternative under consideration was
overruling ADR-008 outright; that makes `.forms` snapshots session-ordered, which
is the failure mode ADR-008 exists to prevent.

**Q2 — Are forms `Value`s?**
`form` and `value` are separate modules, `Value` has no `Form` variant and no
metadata slot — but macros are language functions running in the VM, so they
receive and return forms *as values*. Either forms are `Value`s (and ADR-009's
metadata needs a home in that enum or a side table), or there is a Form↔Value
conversion at every macro boundary, which is exactly where source spans die.
ADR-009 is unimplementable until this is settled, and it gets expensive to change
once the reader and expander exist. **Decide first.**

**Q3 — Keywords.**
`Value` has no `Keyword` variant, and ADR-016's own example is `(io/open path
:read)`. Keyword-keyed maps are the most common idiom in the dialect being
inherited. Separate variant, or interned symbols with a flag bit? Interacts with
the symbols-vs-strings trap.

**Q4 — Tail calls.**
Never mentioned anywhere in v0.1. No laziness (ADR-012) means iteration is
recursion; `loop`/`recur` as a macro lowers to a self-call, which is constant
space only with frame reuse. Under ADR-004 frame reuse is nearly free, but it has
to be an actual decision. **Blocks Q5.**

**Q5 — `loop`/`recur` as macro or core form.**
Attempt it as a macro first; admit it as a core form only on evidence, from a real
attempt rather than in advance. Cannot be settled before Q4.

---

## Blocking a milestone

**Q6 — Collection representation and transients.** *(Milestone 6)*
ADR-011 says one representation each and never names it. With ADR-012 making
reduce-into-a-collection the default idiom, an assoc-vec map with copy-on-`assoc`
and no transients is O(n²) for the most common operation in the language. Name the
representation; decide whether transients exist, even if the answer is "not yet."

**Q7 — What does the suspension test suspend on?** *(Milestone 8)*
v1 is blocking I/O only, so ADR-017 never produces a `Pending`. Proposed: a step
limit that suspends at an arbitrary instruction boundary — free under ADR-004, and
a far stronger test than suspending at points built for it. Without this, the
property test quietly becomes "suspend at the one place we designed for."

**Q8 — Serialization: sharing and cycles.** *(Milestone 8)*
"Plain data" hides two things. `Rc` graphs mean the canonical encoding must
preserve DAG structure — a tree walk blows up exponentially on shared structure
and breaks identity semantics for cells and atoms. And ADR-003 explicitly permits
cell cycles, on which a tree walk does not terminate.

**Q9 — Migration across builds and into populated VMs.** *(Milestone 8)*
`SymId` identity is intern-order dependent. Deserializing into a *fresh* VM works
because the intern table travels; resuming into a *populated* VM needs remapping.
Separately: must a snapshot survive a bytecode format change? Proposed for both:
same-build, fresh-VM only. Snapshots are disposable. This is the only place a
compatibility contract exists at all, so state it rather than inheriting it by
accident.

**Q10 — Integer overflow semantics.** *(Milestone 3)*
Wrap, saturate, or throw. Rust panics in debug and wraps in release, so this must
be explicit or the two builds disagree. Test the release build.

**Q11 — How globals are created.**
Core forms include `global` (a reference) but nothing that makes one. Presumably
`def` lowers to a cell write — say which, since it interacts with hot redefinition
and with what "module-level bindings" means in ADR-002's recursion exception.

**Q12 — Namespaces and modules.**
`global`, "modules" in VM-owned state (ADR-020), and namespace-qualified symbols
for hygiene (ADR-009) all presuppose a namespace system that has no design, no
ADR, and no line in the budget. For a Clojure-alike this is a subsystem.

**Q13 — Numeric equality and hashing.**
Does `1` equal `1.0`? Decide with hashing in the same breath — if they compare
equal they must hash equal, or they must not compare equal.

---

## Deferred

**Q14 — The name.** `apolisp` is the repo; `lispylang` was the working name in
SPEC v0.1; `.xs` is the working extension.

**Q15 — Multimethods.** Likely the right dispatch mechanism — they subsume
protocols and records at a cost recoverable via inline caching — but not scoped
for v1.

**Q16 — Differential testing against Babashka.** Worth scoping explicitly before
building. With no laziness, no bignum promotion, and possibly no keywords, the
overlapping subset shrinks fast; it may be worth it for arithmetic and core-fn
edge cases only.
