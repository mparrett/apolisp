# Open questions

Undecided, ordered by **the milestone that forces the answer**. When one
resolves, it becomes an ADR and leaves this file; the number is retired, not
reused, so gaps are expected.

Sources: SPEC v0.1 Part V, the spec review of 2026-07-25, and the pre-project
design review of 2026-07-25.

*Resolved: Q1 (→ADR-008 stands, REPL clause deferred), Q2 (→ADR-023, ADR-026),
Q3 (→ADR-025), Q4 (→ADR-028), Q7 (→ADR-029), Q9 (→ADR-029), Q11 (→ADR-027),
Q17 (→ADR-027), Q21 (→ADR-030).*

---

## Before milestone 1 — reader, printer, forms

**Q20 — The v1 semantic surface.**
"A small Lisp in the Clojure dialect" orients but does not decide. Unspecified
and about to be filled in differently by whoever writes the code first:
evaluation order, arity behavior on under/over-supply, duplicate map keys, which
collection literals exist, characters, sets, variadic functions, exception
values, and cross-type collection equality.

Wanted: **one compact table** — in v1 / deliberately different from Clojure /
deferred — covering only the edges milestones 1–6 actually hit. Explicitly not a
grammar, not a standard-library plan. If it starts becoming either, stop.

## Before milestone 3 — VM, calls, closures

**Q10 — Integer overflow semantics.**
Wrap, saturate, or throw. Rust panics in debug and wraps in release, so this
must be explicit or the two builds disagree. Test the release build.

**Q13 — Numeric equality and hashing.**
Does `1` equal `1.0`? Decide with hashing in the same breath — equal values must
hash equal. Include `NaN` and `-0.0`, which are where this actually bites.

## Before milestone 5 — macros

**Q12 — When does one namespace stop being enough?**
ADR-027 fixes v1 at a single namespace with fully qualified interned globals,
which is what read-time syntax-quote resolution (ADR-024) needs. The open part is
what forces a real module system, and whether `require`/aliasing can stay a
library concern over the global table.

**Q5 — `loop`/`recur`: macro or core form?**
ADR-028 settles proper tail calls, which is what this was waiting on — a
self-call in tail position now runs in constant space, so the machinery exists.
Attempt `loop`/`recur` as a macro over the core forms and admit it as a fourteenth
special form only on evidence from a real attempt. Note the interaction from
ADR-028 rule 2: a `recur` inside a `try` with a `finally` is not a tail call, so
the macro has to either reject that shape or accept the frame.

## Before milestone 6 — collections

**Q6 — Collection representation and transients.**
ADR-011 says one representation each and still does not name it. With ADR-012
making reduce-into-a-collection the default idiom, an assoc-vec map with
copy-on-`assoc` and no transients is O(n²) for the most common operation in the
language. Name the representation; decide whether transients exist.

## Before milestone 8 — serialization

**Q8 — Sharing in the snapshot encoding.**
ADR-029 makes the DTO object-id based, which handles cell cycles — they become
ordinary id edges. What remains: `Rc`-shared immutable structure (strings,
collections, closures) still needs identity preserved across a round-trip, or a
snapshot expands shared structure into copies. Decide whether that is a
correctness requirement or an accepted size cost in v1.

## Before milestone 9 — REPL

**Q1 — Reader table scope in the REPL.**
ADR-008 freezes reader config per file. Is a REPL *session* its own parse unit,
with a freely mutable table? Proposed: yes — mutate freely at the REPL, declare
at the top of a file.

Deliberately left open. ADR-008 as written is sufficient to build the reader:
config is per parse unit and frozen for the duration, and the code is identical
either way. Only an outright overrule would change milestone 1, and that costs
order-dependent `.forms` snapshots — the failure mode ADR-008 exists to prevent.

## No milestone — decide when evidence arrives

**Q18 — Mutation checks as an oracle rung.** *(Evidence arrived — milestone 1.)*
Run against milestone 1's own span tests, two of three mutants survived: every
origin could be `Unknown`, or every span could start at byte 0, with the suite
staying green. Only arity was actually checked. Fixed by adding `.spans`
goldens, which ADR-026 had already specified and the implementation had skipped.
See `docs/notes/milestone-1-pilot.md`. The question is now whether this becomes
a standing rung rather than whether it is worth doing once.

`../reg-lisp` found a mutant that never restored the compiler's line counter and
*passed its entire suite*, because every test program had subexpressions on the
same line as the enclosing form. Green corpus, dead mechanism. Its answer is a
`verify.sh mutate` that deletes the load-bearing line and shows the test
flipping. Our review-gated rule keeps golden files honest about changes; it says
nothing about whether a test can fail at all. Worth doing for a handful of
load-bearing lines — the span-restore path is the obvious first one. Not worth a
general mutation-testing framework.

**Q19 — Does `Rc` survive contact with the evidence?** *(Would reopen ADR-003.)*
`../wallisp` measured refcounting as the slowest of four GC strategies
(~1.1–1.25× worse than mark-sweep, penalty tracking call volume rather than
allocation), and its refcount engine was not smaller — 560 lines against 596.
ADR-003 justified `Rc` primarily on line count.

Weakened by ADR-025: with cells as arena ids there is no `Rc` cycle to leak, so
the strongest practical complaint is gone and what remains is throughput on a
workload we have not run. Against reversing: in C every value is an arena cell,
so tracing is cheap there in a way it is not in safe Rust. For reversing: ADR-004
hands us a precise root set for free.

Under ADR-021 this reaches every subsystem and needs an argument, not a
benchmark. Not for v1.

**Q14 — The name.** `apolisp` is the repo; `lispylang` was the SPEC v0.1 working
name; `.xs` is the working extension.

**Q15 — Multimethods.** Likely the right dispatch mechanism — they subsume
protocols and records at a cost recoverable via inline caching — but not scoped
for v1.

**Q16 — Differential testing against Babashka.** Scope explicitly before
building. With no laziness, no bignum promotion, and a deliberately different
numeric tower, the overlapping subset shrinks fast; it may be worth it for
arithmetic and core-fn edge cases only.
