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
Q8 (→ADR-043), Q22 (→ADR-043), Q1 (→ADR-044), Q5 (→ADR-047), Q29 (→ADR-048),
Q34 (→ADR-052), Q36 (→ADR-053), Q35 (→ADR-054, which resolves it as a
refusal), Q18 (→ADR-055, after eight milestones of evidence).*

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

The string half of that list is done: **ADR-049** removed `str-len` for
`str-byte-len` and `str-scalar-len`, added `str-index-of`, and put `split`,
`pad-left` and `pad-right` in the prelude — which took the report program from
54 lines to 41 and its hand-written library from 30 to 19.

`sort`, `take` and `drop` followed in **ADR-050**, which took the report program
to 21 lines with no hand-written library at all. `apply` did not, and why is
**Q32**.

That is the same shape of evidence that produced ADR-046 through ADR-048 — each
piece scoped by a program that had already wanted it — so another round of
candidate 2 is available whenever it is wanted, and now has its list.

**Candidate 1 again, 2026-07-28, at the third ETHOS workload**
(`notes/the-pager-program.md`). A pager, and the ratio moved the rest of the
way: 46 lines of code, **four** of them host shim, nothing hand-rolled. The
standard library carried a whole program for the first time. What it found is
therefore not on the language surface at all — `io/stdout` is buffered until the
program ends, so the first version painted five frames into a terminal that had
already stopped caring, and the only way to paint one live is to open
`/dev/tty` as a file, which is the `fs` feature and which escapes ADR-029's
round-trip property. That is **Q33**, answered for the terminal by ADR-051, and
it is the first finding from this practice that is a question about the *host
boundary* rather than a missing function.

**The next program is pre-registered rather than written**
(`notes/the-editor-prediction.md`). Three programs in, the interesting result
has each time been reconstructed afterwards from whatever surprised the person
typing, which is the weakest form of the evidence this practice runs on. A text
editor is the obvious next rung of the terminal workload and the first program
that would be *used* rather than run once, so what it is expected to find is
written down first — with a number on the one quantitative claim, which is that
E-11's copy-on-write turns any whole-file pass into the O(n²) **Q6** has named
since before anything could reach it. Nothing is committed to building it.

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

**Q32 — Does `apply` earn a dynamic call?** *(Filed 2026-07-28 by ADR-050.)*
`(apply f xs)` calls `f` with the elements of `xs` as its arguments, and this
machine cannot express that.

Two things are in the way, and the second is the real one. `Instr::Call` carries
`argc` **in the instruction**, so an argument count is fixed at compile time.
And a `Proto` carries `slots`, also fixed at lowering, which is what
`Execution::slots` is sized from — so a runtime-length argument list has **no
reserved slots to be splatted into**. Supporting one means a frame that grows
after it is created, which reaches ADR-006's monotonic slots, ADR-034's `Proto`,
and the `Image` that serializes frames. That last is constraint #2's machinery,
which is the part of this system least worth disturbing for a convenience.

A primitive cannot route around it: ADR-041 part 6 forbids a native calling a
language closure, for the same reason it forbids a native `map`.

**Against building it.** Nothing has asked. It appeared in a capability probe —
a list of names checked for boundness — and not in any program. Every use it is
usually reached for is already spelled: `(apply str xs)` is `(join "" xs)`,
`(apply + xs)` is `(reduce + 0 xs)`, `(apply max xs)` is a `reduce`. Against
that, every other piece of the standard library built this week was scoped by a
program that had already wanted it, and that rule is the one keeping this from
growing a surface nobody needed.

**For building it.** Genuinely dynamic arity — calling a function value with a
computed argument list — has no spelling at all, and a language that cannot do
it forecloses whatever would have wanted it. The cost is knowable rather than
open-ended: one opcode, and a frame that can grow.

**Decide when a program wants it**, and not before. If one does, the thing to
measure first is whether it wants dynamic arity or just a fold.

**Q33 — How does a program paint a terminal?** *(Filed 2026-07-28 from
`notes/the-pager-program.md`, which is the program that wanted it. **The
terminal half is answered by ADR-051**; what remains open is the general case,
below.)*

**Answered for the terminal, 2026-07-28: shape 1, the workaround ratified.**
`(term/open)` returns a handle on `/dev/tty` and painting is `io/write` to it.
The argument that decided it is that the `Image` serializes `Vm::out`, so
buffered output is resumable machine state and the invariant that has to hold is
*if output escaped the buffer, refuse the snapshot* — which a handle enforces via
machinery ADR-016 already built, and which shape 3 would have broken silently.
Painting is now `term`'s capability rather than `fs`'s, and `just subtract`
gained `term` alone as a fourth point.

**Still open: `io/stdout` is all-or-nothing.** A program that wants incremental
output to a *pipe* — a progress line, a log, anything long-running that is not a
terminal — has no answer at all, and `/dev/tty` is not one. That is shape 4
below, it reaches ADR-016, and it has to supersede `io/close`'s reasoning that
dropping the descriptor is what flushes it. Nothing has asked yet. The rule that
produced ADR-046 through ADR-051 applies: wait for the program.

The original statement of the question follows.


`io/stdout` is buffered until the program ends, so an interactive program
written against it paints into a terminal that has stopped caring. The workaround
is `(io/open "/dev/tty" :write)`, which takes the `Host::File` path and writes
live. It works — the pager is a correct full-screen application on it — and it
costs two things: the `fs` feature, and the round-trip property.

**The second cost is the real one, and it is why this is a question rather than a
missing native.** ADR-029 makes emitted effects part of the serialization
comparison *rather than something that escapes it*, and that is the only reason
constraint #2 is a property instead of an aspiration. `/dev/tty` escapes it. So
the choice is not "should there be a `term/write`" — it is what the terminal
workload is allowed to cost the oracle.

Four shapes, and they are not equally cheap:

1. **Leave it.** `/dev/tty` is the documented answer, terminal programs are
   outside the round-trip property by construction, and `TRAPS.md` carries the
   warning. Costs nothing, and concedes that one of the three ETHOS workloads
   cannot be snapshotted or replayed.
2. **An unbuffered stdout mode**, chosen by the driver rather than by the
   program — the buffered host stays exactly as it is for the property, and
   `apolisp run` gains a way to say "this program is interactive". The `.out`
   transcript is the thing to think about first: `main.rs` argues a golden that
   depends on a step limit is not a golden, and a golden that depends on flush
   timing has the same problem.
3. **`term/write` in the adapter**, alongside `term/size` and `term/read-key`.
   Symmetric with the input half, does not need `fs`, and keeps the terminal
   entirely inside the thing that is already excluded from the budget. It also
   puts a second live output path next to `vm.emit`, which is the part to argue
   about.
4. **Make the buffer a real stream with a flush point.** Largest, reaches
   ADR-016 and the handle table, and `io/close`'s comment already explains why
   there is no `io/flush`.

Not gating anything: the pager works today. What it gates is whether a *second*
terminal program is written the same way by copying the workaround, at which
point the workaround is the design and nobody decided it.

Adjacent, and worth settling in the same breath: **a program has no argv and no
environment.** `main.rs` takes a command and a path. The pager pages its own
source because that is the only file it can name, and every terminal program
takes an argument.

**The argv half is answered by ADR-058**, as a global rather than a native so
that the `Image` gets it for free. The **environment** half is still open, and
it is not the same question: an argument vector is fixed at process start and a
process-wide mutable table is not, which is a question about serializable state
rather than about a missing name.

## No milestone — decide when evidence arrives

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
