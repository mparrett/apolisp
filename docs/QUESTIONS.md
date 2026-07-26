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

**Q2 — Are forms `Value`s?** *Resolved 2026-07-25 → ADR-023, ADR-024.*

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

## Raised by prior art

**Q17 — Is module-level-only mutual recursion a rule rather than a hedge?**
ADR-002 says v1 *may* restrict mutual recursion to module-level bindings.
`../let-rs` supplies the reason to make that binding: its ADR-021 traces the
`letrec` cycle node by node and concludes no clean fix exists, and its ADR-015
broke the *globals* cycle only because the Vm outlives every closure and can hold
the owning reference. Module level is the one shape where the cycle has an owner.
Related and unpleasant: under identity cells, every self-recursive closure held
in its own cell is a two-node cycle, so ADR-003's accepted leak is systemic
rather than exotic — every `defn` leaks unless the VM owns the binding.

**Q18 — Mutation checks as an oracle rung.**
`../reg-lisp` found a mutant that never restored the compiler's line counter and
*passed the entire suite*, because every test program had its subexpressions on
the same line as the enclosing form. The corpus was green and the mechanism was
dead. Its answer is a `verify.sh mutate` that deletes the load-bearing line and
shows the test flipping. Our iron rule keeps golden files honest about changes;
it says nothing about whether a test could ever fail. Cheap to add for a handful
of load-bearing lines; not worth a general mutation-testing framework.

**Q19 — Does `Rc` survive contact with the evidence?** *(Reopens ADR-003.)*
`../wallisp` measured refcounting as the *slowest* of four GC strategies
(~1.1–1.25× worse than mark-sweep, penalty tracking call volume rather than
allocation), and its refcount engine was not smaller than its mark-sweep engine —
560 lines against 596. ADR-003 justified `Rc` primarily on line count.

Against reversing: in C every value is an arena cell, so tracing is cheap there in
a way it is not in safe Rust, where the 500–2,000 lines come from walking
Rust-owned structures. For reversing: ADR-004 hands us a precise root set for
free, since an explicit frame stack *is* the root set — wallisp's tree-walker had
to build a shadow stack at every `eval` entry to get what we already have.

Under ADR-021 this is a change that reaches every subsystem, so it needs an
argument rather than a benchmark. This is the beginning of one. Not for v1.

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
