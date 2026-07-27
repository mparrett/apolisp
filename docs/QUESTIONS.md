# Open questions

Undecided, ordered by **the milestone that forces the answer**. When one
resolves, it becomes an ADR and leaves this file; the number is retired, not
reused, so gaps are expected.

Sources: SPEC v0.1 Part V, the spec review of 2026-07-25, and the pre-project
design review of 2026-07-25.

*Resolved: Q1 (→ADR-008 stands, REPL clause deferred), Q2 (→ADR-023, ADR-026),
Q3 (→ADR-025), Q4 (→ADR-028), Q7 (→ADR-029), Q9 (→ADR-029), Q10 (→ADR-037),
Q11 (→ADR-027), Q13 (→ADR-041), Q17 (→ADR-027), Q21 (→ADR-030), Q23 (→ADR-039),
Q24 (→ADR-038), Q25 (→ADR-038), Q26 (→ADR-041), Q6 (→ADR-041), Q27 (→ADR-042),
Q8 (→ADR-043), Q22 (→ADR-043), Q1 (→ADR-044).*

---

## Before milestone 1 — reader, printer, forms

**Q20 — The v1 semantic surface.** *(Partly resolved by ADR-033 and ADR-035.)*
"A small Lisp in the Clojure dialect" orients but does not decide. Unspecified
and about to be filled in differently by whoever writes the code first.

**Answered — ADR-033**, because milestone 2 forces them: evaluation order, arity
behaviour on under/over-supply, and variadic functions. A compiler has to emit
*something* for each, so whichever got written first would have become the
answer by default.

**Also answered — ADR-035**, because the compiler could not emit a vector
literal without it: `[a b]` in code position is `(vector a b)`. That says what a
literal *means*, not which ones exist.

**Still open:** which collection literals exist beyond list, vector, and map,
and whether characters are worth superseding ADR-025 for. Everything else in
this list has an entry now: exception values in **ADR-039**, and duplicate map
keys, sets, and cross-type collection equality in **ADR-041**.

Add one to the milestone-6 list: **do the core functions accept `nil` where they
accept a collection** — `(count nil)` → 0, `(empty? nil)` → true? Erratum E-11
makes this the thing that keeps ADR-033's rest-argument rule from being a
semantic fork, so it is no longer only a convenience question.

Wanted: **one compact table** — in v1 / deliberately different from Clojure /
deferred — covering only the edges milestones 1–6 actually hit. Explicitly not a
grammar, not a standard-library plan. If it starts becoming either, stop.

Note the cost of one entry in that list. **Characters** are cheap to answer and
expensive to answer late: ADR-025 froze the `Value` enum without a `Char`
variant, and the size is asserted. The original design conversation recommended
including one — "it gives reader and Unicode APIs a clear scalar-value type" —
and no entry records rejecting it, so this is an omission rather than a
decision. Answering "yes, characters exist" means superseding ADR-025.

## Before milestone 5 — macros

**Q12 — When does one namespace stop being enough?**
ADR-027 fixes v1 at a single namespace with fully qualified interned globals,
which is what read-time syntax-quote resolution (ADR-024) needs. The open part is
what forces a real module system, and whether `require`/aliasing can stay a
library concern over the global table.

**Q28 — Does the expander evaluate the top level as it goes?**
ADR-040 expands a unit form by form but never *runs* one, so a macro body sees
primitives and previously defined macros and nothing else: a function defined
with `def` earlier in the same file does not exist when a macro runs. Clojure
compiles and evaluates the top level form by form, which removes the limit and
buys it a real cost — every top-level form's side effects happen at compile
time, and a file that launches a thread or opens a socket does so while being
compiled.

Decide when a macro actually wants a helper. Two smaller options exist and
should be weighed first: writing the helper as a macro, or letting a macro
body's helpers live in the prelude.

**Q5 — `loop`/`recur`: macro or core form?**
ADR-028 settles proper tail calls, which is what this was waiting on — a
self-call in tail position now runs in constant space, so the machinery exists.
Attempt `loop`/`recur` as a macro over the core forms and admit it as a fourteenth
special form only on evidence from a real attempt. Note the interaction from
ADR-028 rule 2: a `recur` inside a `try` with a `finally` is not a tail call, so
the macro has to either reject that shape or accept the frame.

## Before milestone 9 — REPL

**Q29 — Can compiled code be shared between chunks?** *(REPL half resolved by
ADR-044.)*
A `Closure` names its proto by index into the chunk it was compiled in
(ADR-034), so a closure is meaningless outside that chunk. The expander already
works around this by keeping a macro's chunk beside the macro.

**The REPL half is gone rather than answered.** ADR-044 gives a session one
chunk and appends each input's protos to it, so indices never move and no
closure ever leaves its chunk. The registry this question proposed was not
built, because a closure carrying a chunk id has to resolve that id after an
`Image` is restored, which puts every `Chunk` into the DTO.

**What remains is the prelude function.** `map` and `reduce` still cannot live
in `prelude.xs`, and the reason has changed: ADR-044's mechanism would fix it —
compile the prelude into the unit's chunk — but then the prelude's protos land
in every `.disasm` golden in the corpus and grow with the prelude forever. That
is a cost to weigh, not an impossibility. Weigh it when something actually
wants a function there; writing `map` per file has not hurt yet.

## No milestone — decide when evidence arrives

**Q18 — Mutation checks as an oracle rung.** *(Evidence arrived seven times —
milestones 1, 2, 4, 5, 6, 7, and 8. At this point the rung is not in question;
what remains open is only its shape.)*
Run against milestone 1's own span tests, two of three mutants survived: every
origin could be `Unknown`, or every span could start at byte 0, with the suite
staying green. Only arity was actually checked. Fixed by adding `.spans`
goldens, which ADR-026 had already specified and the implementation had skipped.
See `docs/notes/milestone-1-pilot.md`.

Milestone 2 ran four mutants against the compiler and one survived — and it
found something different in kind. ADR-028 rule 2 was enforced *twice*, by the
`tail` flag and by the region counter, so deleting either left the suite green
and no test could say which was doing the work. The fix was to delete the
redundancy, not to add a test. See `docs/notes/milestone-2-mutants.md`.

Milestone 4 ran five against the handler stack and one survived, predicted to:
unwinding that drops frames but keeps their slots is a real leak that **no test
can observe** — bounded, so no high-water mark moves, and nil-filled, so no
value is wrong. The test written for it was dead too. Fixed by making the
release of a frame one shared mechanism rather than two that agree, so the
mutation stops being expressible. See `docs/notes/milestone-4-mutants.md`.

Milestone 5 ran six against the expander. The interesting one was not a
survivor at first: a mutant that dropped everything after a splice expanded the
whole corpus correctly, because every splice in it happened to be last — the
corpus claimed in a comment to cover the case and did not. The survivor was
worse. **A macro receiving *expanded* arguments instead of the forms as written
passed all 75 tests**, because every macro in the suite used its arguments as
code, where expanding early and expanding late agree. See
`docs/notes/milestone-5-mutants.md`.

Milestone 6 ran eight. Seven died; the eighth could not have done anything
else — it mutated a *performance* claim (`Rc::make_mut` versus an unconditional
clone), and the two are behaviourally identical, so no test can separate them.
The interesting result came from instrumentation instead, and refuted ADR-041's
own rationale (erratum E-13). Worth naming as a limit of the rung: **a mutation
check answers behavioural claims only.**

Milestones 7 and 8 found a fourth kind, twice, and it is the one that scales
worst: **a hole in the corpus rather than a defect in the code**. Seven's read
path could ignore the handle generation entirely, because nothing ever read
*through* a stale handle; eight dropped four separate fields from an `Image`
and the strongest property in the project — cutting at every instruction
boundary, over nine programs, in two forms — noticed none of them. Nothing was
wrong with the implementation either time. What was wrong was the belief that
the suite was checking it, and no amount of strengthening a property fixes
that, because a property only sees the state its inputs create.

Both passes also predicted every survivor, which is the argument for
pre-registration rather than for mutation testing: writing the table down is
when the holes became visible. Filled in afterwards they would have read as
discoveries.

**So the finding is broader than "tests can be dead."** A mutation check also
finds duplicated enforcement, which nothing else in this loop looks for, and
which a test written to pin the rule will happily pass while the mechanism it
was written for is dead. It finds a third thing too: a defect that is real and
*unobservable*, where the honest response is to remove the way to write it
rather than to add a test that cannot fail. Three milestones, three kinds of
finding, nothing else caught any of them. The remaining question is only what shape the rung takes — `../reg-lisp`
uses a `verify.sh mutate` over a hand-listed set of load-bearing lines, and that
still looks right here: worth doing for a handful of lines per milestone, not
worth a general framework.

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
