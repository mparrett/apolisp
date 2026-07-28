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
Q8 (→ADR-043), Q22 (→ADR-043), Q1 (→ADR-044), Q5 (→ADR-047), Q29 (→ADR-048).*

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

## After milestone 10 — what the project is for now

**Q31 — What comes after the build order?** *(Filed 2026-07-27. Gates nothing
mechanically and everything in practice.)*
`BUILD.md`'s ten milestones are all Done and there is no queue. Nothing in the
docs says what this project is for past milestone 10, so the next thing built
will be whatever someone happens to start — which is the one way a project with
no compatibility contract can still acquire decisions nobody agreed to.

Two programs were written to make this concrete rather than theoretical
(`notes/first-programs.md`). Both worked on the first run. What they found was
not a missing capability but a missing *surface*: six of Life's seventeen
definitions are standard library rebuilt by hand, five of them exist only
because there is no looping form (**closed by ADR-047**), and two findings were
not ergonomics at all — `io/read` is a short read that no program can frame a protocol on
(`TRAPS.md`), and there is no string→number conversion anywhere except
`json/decode`, which is an optional host adapter. ADR-013 says features gate
host capability and never language semantics; that is the first place it has
stopped being true, and `just subtract` cannot see it because nothing in the
suite parses a number from a string either.

**Answered 2026-07-27: build the standard library** (candidate 2 below). The
string→number hole is closed by ADR-046, which also records the limit it
exposed — the subtraction harness can prove a capability is removable and
cannot notice that a semantic went missing with it. `loop`/`recur` is closed by
ADR-047, as a core form rather than the prelude macro, and the sequence library
by ADR-048 — which weighed Q29's cost by measuring it (4 functions = 160 golden
lines per program, or zero if the prelude is appended after the unit) rather
than by arguing about it.

**All three pieces of candidate 2 are done**, and candidate 1 was run again on
top of them (`notes/the-report-program.md`): a CSV report, 54 lines, of which
**30 are standard library the program had to define first**. The ratio has moved
since the first two programs but not as far as it looks — what ADR-047 and
ADR-048 removed was iteration boilerplate, and what it exposed underneath is a
different layer.

Missing now, in the order a program meets them: `split` (and the substring
search a multi-character separator needs), `sort`, `take`/`drop`, `apply`, and a
string-padding function. Plus one trap rather than a gap: `str-len` is bytes, so
the column-padding idiom misaligns on non-ASCII silently, which is the one place
ADR-018's surface is quiet where the rest of it is loud (`TRAPS.md`).

That is the same shape of evidence that produced ADR-046 through ADR-048 — each
piece scoped by a program that had already wanted it — so another round of
candidate 2 is available whenever it is wanted, and now has its list.

The candidates, and what each is really a bet on:

1. **Write programs, fix what they break.** The bet is that the test suite has
   stopped being a source of information about the language, because it
   exercises the VM rather than the surface — which the two probes support, in
   that none of the gaps above were on any list. Cheapest, and the only option
   that keeps producing evidence rather than consuming it.
2. **Build the standard library.** `loop`/`recur` (→ADR-047), a sequence library
   (→ADR-048), and the string→number hole (→ADR-046). ***Chosen, and done.***
   The stated risk was building from taste rather than from use; what happened
   instead is that each piece was scoped by a program that had already wanted
   it, and the one genuinely open cost — Q29's — turned out to be avoidable by
   ordering rather than payable. Six functions, and no golden moved.
3. **Performance.** ADR-021 removed the gate, Q19 is open, and the tooling now
   exists (`valgrind --tool=dhat` via the soak image). Genuinely fun per ETHOS
   constraint #3, and the least urgent — nothing has been slow yet because
   nothing real has run long enough to be.
4. **Tag v0.1 and stop adding.** The ladder is `merge → soak → tag`, the soak
   now runs and is green on two platforms, and this is a natural stopping point.

These are not exclusive; the ordering is the decision. Not resolvable by
argument from inside the repository — it depends on what the project is *for*,
which is the one thing no document here records.

## No milestone — decide when evidence arrives

**Q18 — Mutation checks as an oracle rung.** *(Evidence arrived eight times —
milestones 1, 2, 4, 5, 6, 7, 8, and 9. At this point the rung is not in
question; what remains open is only its shape — and milestone 9 found that the
shape has a failure mode of its own, see below.)*
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

Milestone 9 found the rung's own failure mode, and it argues for keeping this
hand-rolled rather than for building the framework. **A substitution that does
not match leaves a green suite, which is exactly what a surviving mutant
leaves.** Two of twelve were no-ops and were recorded as survivors until one of
them — predicted to survive for a specific reason — was checked against the
file. Whatever shape this rung ends up taking has to assert that the mutation
happened; `../reg-lisp`'s `verify.sh mutate` deletes a named line, which fails
loudly when the line has moved, and that is one more argument for its shape
over a pattern-matching script.

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

**Q30 — Is `Proto.lines` the wrong shape?** *(Filed 2026-07-27 from
<https://tidefield.dev/bytecode-to-source-mapping/>. Nothing has measured a
problem; this exists so the option is on record rather than rediscovered.)*
ADR-023 point 2 makes `lines[i]` the origin of instruction `i` — a parallel
array, one `SpanOrigin` per instruction, `O(n)` memory and `O(1)` lookup, kept
parallel structurally because `emit` is the only thing that pushes to either.

That article argues for `(offset, line)` pairs at run boundaries instead:
`O(r)` memory, `O(log r)` random lookup by binary search, `O(n)` sequential by
cursor. It is the right structure for what it stores. Measured against our
corpus, it is close to worthless for what *we* store:

```
instructions            497
runs of equal span      441   ratio 0.89   ← what we would compress
runs of equal line only 121   ratio 0.24   ← what it compresses
```

Consecutive instructions almost always come from different sub-expressions of
the same line, so a span-keyed run encoding has nothing to collapse: ~11%
saved, paid for with a search. Two further reasons it does not transfer.
Our `n` counts *instructions*, not bytes — `Instr` is a typed 16-byte enum
(ADR-034) — so the row that article calls `O(n)` is already several times
smaller here. And `SpanOrigin` carries `Generated` and `Unknown` beside
`Source` (ADR-026), which an `(offset, line)` pair has nowhere to put; a
run-boundary encoding would also blur exactly the macro-output boundary that
`.disasm` goldens exist to show.

**What would reopen it:** a measurement showing `Proto.lines` matters — memory
in a large chunk, or a `.disasm` pass that is slow enough to notice. The
structure applies cleanly to a *line projection* of the origins (0.24 there),
so the fallback is available without giving up spans: keep `lines` and derive a
compressed line table for whatever wants one. Do not trade spans for lines to
get the compression; that is paying in diagnostics for memory nobody has
missed.

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

The `cycle`-does-not-leak half is no longer prose. `tests/soak/cycle.xs` builds
a self-referential cell and comes back clean under valgrind — 2,406 allocs,
2,405 frees, nothing definitely lost. First evidence for a claim this entry has
been asserting since ADR-025.

**The filed reading is done, and it was filed against the wrong question.**
<https://pranitha.dev/posts/rust-and-memory-allocators/> is about RSS that never
comes back down after a bursty load: glibc arenas, the top-chunk deadbolt, and
cached chunks that never consolidate. It measures *memory returned to the OS*,
not what an allocation costs — which was the input this entry wanted. Most of
its mechanism is out of scope besides: thread arenas, work-stealing and
cross-thread frees all need threads, and multithreaded execution within one VM
is an ETHOS non-goal. The single-threaded top-chunk deadbolt does still apply to
any long-running apolisp process.

It supplied two things anyway.

*A trap for whatever benchmark eventually settles this.* Their heap went from a
1.4 GB peak to 10,798 bytes at exit while RSS stayed pinned at the container
limit. **RSS is not a measure of allocation behaviour.** A benchmark that reads
it would be measuring glibc's arena policy and reporting it as a fact about
`Rc`.

*An instrument, with no dependency to add.* dhat's `t-gmax`/`t-end` is what
separated their allocator behaviour from a real leak, and it is reachable as
`valgrind --tool=dhat`, which the soak image already carries — so ADR-014's
budget stays unspent. Pointed at `tests/soak/churn.xs`:

```
Total:     127,622,370 bytes in 1,482,053 blocks
At t-gmax:      67,202 bytes in       675 blocks
At t-end:          544 bytes in         1 block
```

Roughly 296 allocations per iteration against a live set that never exceeds
67 KB. Two consequences. The article's failure mode cannot bite here — a peak
that small is never going to strand pages. And the allocation *rate* is high
relative to the live set, which is exactly the regime where allocator cost is
visible, so this remains a real question rather than a settled one.

Caveat with the numbers: `churn.xs` is a synthetic allocation-heavy program
written for the leak check. It characterises itself, not a representative
workload, and there is still no representative workload to characterise — see
Q31.

Under ADR-021 this reaches every subsystem and needs an argument, not a
benchmark. Still not for v1.

**Q14 — The name.** `apolisp` is the repo; `lispylang` was the SPEC v0.1 working
name; `.xs` is the working extension.

**Q15 — Multimethods.** Likely the right dispatch mechanism — they subsume
protocols and records at a cost recoverable via inline caching — but not scoped
for v1.

**Q16 — Differential testing against Babashka.** Scope explicitly before
building. With no laziness, no bignum promotion, and a deliberately different
numeric tower, the overlapping subset shrinks fast; it may be worth it for
arithmetic and core-fn edge cases only.
