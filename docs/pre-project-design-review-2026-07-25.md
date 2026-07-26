# Pre-project design scaffolding review

**Date:** 2026-07-25
**Review basis:** commit `b28ba27`
**Scope:** internal consistency, implementability, architectural shape, factual
claims, verification design, document mechanics, and usefulness as agent context

This is a review, not a replacement specification. It is intentionally more
pedantic than the documentation should become.

## Verdict

The scaffolding has the right center of gravity. `ETHOS.md` is unusually good:
short, opinionated, and concrete about what is being optimized. The explicit
frame stack, monotonically allocated slots, forms-as-values, closed compiler
core, opaque host boundary, and phase snapshots fit together into a system that
could plausibly retain nanochat-like grokkability while performing well enough to
be interesting. The decision/cost/rejected ADR format is also exactly the right
amount of retained reasoning.

I would not start implementation quite yet. Five issues cross subsystem
boundaries and are cheaper to settle in prose than in Rust:

1. the representation and ownership of mutable cells;
2. the actual boundary of serializable state;
3. source-span behavior through macros;
4. creation and resolution of globals/namespaces; and
5. the interaction among tail calls, exception handlers, `finally`, and
   suspension.

This should be a convergence pass, not another design phase. Resolve those five,
correct the factual and mechanical drift below, define the deliberately small v1
surface, then begin. Most other open questions should remain open until their
milestone.

## What should be protected

These are the strongest choices in the current design and should require real
counterevidence to disturb:

- **One holdable kernel.** The line budget and one-file-until-it-hurts rule are
  useful constraints, not aesthetics.
- **A heap-resident call stack.** ADR-004 is the architectural hinge. It supports
  depth safety, debugging, fuel limits, and pause/resume without platform stack
  machinery.
- **Slot bytecode without allocation cleverness.** ADR-006 is an excellent
  performance-per-line choice. Slot reuse can remain a local later pass.
- **Forms are values.** ADR-023 preserves the Lisp macro surface and avoids a
  second tree representation at the macro boundary.
- **Closed core, open macros.** ADR-007 gives agents a strong test for whether a
  feature belongs in the VM.
- **Opaque host capabilities.** ADR-016 is a clean boundary even if migration of
  live handles is deferred.
- **The four phase artifacts.** `.forms`, `.expanded`, `.disasm`, and `.out` are
  the best piece of process in the design. They make the implementation legible
  after it stops fitting in a single diff.
- **Optimization by blast radius.** ADR-021 fits this one-user project, provided
  optimizations still carry a falsifiable claim and the corpus is not treated as
  a complete semantic oracle.

## Finding taxonomy

- **P0 — settle before implementation:** the current text permits incompatible
  implementations or forces an early redesign.
- **P1 — settle before the affected milestone:** correctness or architecture is
  underspecified, but it need not block unrelated work.
- **P2 — repair during the button-up pass:** factual, editorial, navigation, or
  maintenance debt.

Type labels distinguish **Architecture**, **Correctness**, **Consistency**,
**Factual**, **Verification**, and **Mechanical** findings.

## P0 findings — settle before implementation

### P0.1 — The mutable-cell representation cannot satisfy the current ADRs

**Type:** Architecture · Correctness · Consistency
**Where:** `ADR.md` ADR-005, ADR-010, ADR-020; `QUESTIONS.md` Q8, Q17, Q19

ADR-010 says `Value::Cell(Rc<Cell>)`. ADR-020 says cells are mutable, all mutable
state is reached through `&mut Vm`, and nothing in the core uses
`Rc<RefCell<_>>`. ADR-005 separately names a VM-owned `cell heap`.

Those are three different ownership models. An aliased `Rc<Cell>` cannot expose
ordinary mutable access to a non-`Copy` `Value`; it needs interior mutability
(`RefCell`, `UnsafeCell`, or equivalent). The Rust documentation is explicit that
`Rc<T>` only provides immutable access, and that `Rc<RefCell<T>>` is the standard
shape for multiple owners of mutable data. A VM-owned heap instead implies
`Value::Cell(CellId)` and mutation through `&mut Vm`.

This choice determines cycle behavior, recursion, snapshot encoding, stale-cell
semantics, and whether ADR-003's accepted leaks are rare or systemic.

**Recommendation:** choose one model in a new ADR before defining `Value`.
The design that best matches the existing ethos is:

```text
Value::Cell(CellId)
Vm.cells: generational arena<CellId, Value>
all reads/writes: &mut Vm
```

For v1, explicitly accept that arena cells are retained for the VM lifetime and
instrument the count. That is simple, honest, serializable, and makes logical
cycles ordinary ID edges rather than `Rc` leaks. If retention is unacceptable
for long-running simulators, that is evidence for a small tracing cell arena—not
for pushing `RefCell` through all environments. Keep immutable strings,
collections, and closures `Rc`-backed independently.

If `Rc<RefCell<Value>>` is chosen instead, supersede the “no `RefCell`” and
VM-owned-cell-heap claims and budget for an identity-aware graph serializer.

### P0.2 — “Machine state is exactly …” is incomplete and contradicts later ADRs

**Type:** Architecture · Consistency
**Where:** `ADR.md` ADR-005, ADR-017, ADR-020, ADR-023

ADR-005's exact tuple omits state that later text says exists:

- task states, per-task frame stacks, runnable/waiting queues, and scheduler state;
- exception/finally handler state;
- modules, globals, namespaces, and hot-redefinition cells;
- pending host-call/reacquisition descriptors;
- deterministic counters that can affect resumed behavior;
- output or other effect-log state used by the round-trip oracle.

ADR-023 also says a snapshot may contain forms mid-macroexpansion, but the tuple
contains no reader, expander, compiler, or compiler-continuation state. Running a
macro in the VM does not automatically make the surrounding Rust expansion walk
serializable.

**Recommendation:** split the concepts and name the snapshot boundary:

```text
Vm        = intern table + globals/modules + cell/handle arenas + registry config
Execution = code identity + frames + slots + pc + handler stack + status/fuel
Image     = serializable DTO for one Vm + one suspended Execution
```

For the first implementation, snapshot only at VM instruction boundaries while
executing already-compiled code. Do not promise mid-read, mid-expand, or
mid-compile snapshots. Caches and host registries should be declared either
reconstructible or excluded. Replace “exactly” only after the inventory is
actually exhaustive.

### P0.3 — Parent-indexed spans are not closed under ordinary macro operations

**Type:** Correctness · Architecture · Verification
**Where:** `ADR.md` ADR-009, ADR-023; `BUILD.md` milestone 1 and reader property;
`TRAPS.md` metadata loss

The parent-indexed idea is promising, but four cases have no defined behavior:

1. The root can be an immediate, yet “the root carries its own” does not say what
   object carries that span.
2. Maps are forms, but ADR-023 assigns `child_spans` only to lists and vectors.
3. Language-level `list`, `vector`, `cons`, and quasiquote construction cannot
   provide source spans because language code cannot attach metadata.
4. Returning a macro argument directly detaches it from the parent that held the
   argument's own span.

The property test is also impossible as stated. If equality includes spans,
`read(print(read(s))) == read(s)` generally fails because printing changes
locations. If equality ignores spans, the property cannot catch metadata loss.
The ordinary `.forms` printer likewise cannot pin span behavior unless it has a
debug mode that displays origins.

**Recommendation:** keep forms-as-values and the positional storage, but add a
small expansion-only carrier:

```text
LocatedForm { root: Value, root_span: SpanOrigin }
SpanOrigin = Source(Span) | Generated(Span) | Unknown
```

Aggregates, including maps, hold one origin per syntactic child. Macro-created
nodes receive `Generated(call_site)` (or `Unknown` when no call site exists).
The expander carries a root origin alongside every value it treats as code.
Then split the tests:

- print/read equality ignores spans and tests data round-tripping;
- a span-invariants property checks source bounds and child arity;
- selected `.forms`/`.expanded` debug snapshots include span origins;
- a macro diagnostic test pins call-site attribution.

ADR-009 should be fully superseded, not merely “refined”: “every form carries
metadata” is not the same decision as “metadata belongs to positions in a tree,”
and its Q2/hygiene rationale is now stale.

### P0.4 — The language cannot currently bootstrap globals or macros

**Type:** Architecture · Correctness
**Where:** `ADR.md` ADR-002, ADR-007, ADR-024; `BUILD.md` milestones 3 and 5;
`QUESTIONS.md` Q11, Q12, Q17

Milestone 3 requires a recursive function and milestone 5 requires in-language
`defmacro`, but the closed core can read a `global` and cannot create one. The
reader must also resolve syntax-quoted names in a current namespace before the
namespace/module model exists. Q11 and Q12 are therefore already blocking, even
though they sit under the generic “blocking a milestone” section without milestone
labels.

Q17 also overstates the `let-rs` transfer. Its “every `defn` leaks” conclusion is
conditional on a self-recursive closure capturing its own cell. ADR-002 says
self-recursion uses the function's own identity instead. Those two designs must
not be treated as the same case.

**Recommendation:** specify the smallest boot model, not a Clojure namespace
system:

- one current module/namespace in v1;
- fully qualified interned global names;
- a VM-owned global cell table;
- one explicit top-level create-or-rebind operation;
- self-tail recursion targets the current function identity;
- mutual recursion is module-level only in v1;
- `def` and `defmacro` are library macros over the explicit top-level operation,
  or are compiler-driver directives if that is smaller.

Also define the reader-config preamble as fixed built-in syntax. A declaration
that changes the reader cannot itself require the changed reader to parse it.

### P0.5 — Tail calls, `try`/`finally`, and suspension share one missing runtime structure

**Type:** Architecture · Correctness
**Where:** `ADR.md` ADR-004, ADR-007, ADR-016; `BUILD.md` milestones 3, 4, and 7;
`QUESTIONS.md` Q4

`with-open` promises lowering to `try`/`finally`, but the core-form list contains
only `try`, milestone 4 names only `try`/`throw`, and no document defines where
active handlers/finalizers live. They must survive exceptions, tail calls, and a
snapshot. A tail call cannot blindly reuse a frame when leaving a dynamic extent
that has pending cleanup.

**Recommendation:** decide that frames contain or reference an explicit handler
stack, and specify these invariants before frame layout:

- every nonlocal exit runs pending `finally` blocks exactly once;
- a tail call first discharges cleanups for scopes it exits;
- cleanup may throw, with a defined winner between old and new errors;
- suspension is allowed only at instruction boundaries where handler state is in
  the `Execution` image;
- Rust panics are host bugs and never become language unwinding.

The surface syntax can remain open. The runtime invariant cannot.

## P1 findings — settle by the affected milestone

### P1.1 — Preserve pause/resume, but stop calling it async and migration “for free”

**Type:** Architecture · Scope control
**Where:** `ETHOS.md` constraint 2; `ADR.md` ADR-004, ADR-005, ADR-017;
`QUESTIONS.md` Q7–Q9

Explicit frames make the continuation representable. They do not make graph
encoding, external effects, task scheduling, resource reacquisition, or build
compatibility free. ADR-017's “Cost: none beyond ADR-004” contradicts ADR-005's
own costs and Q8/Q9.

The novel feature is still worth keeping. A clean progression is:

1. **Pause/resume:** one execution, fuel exhaustion at an instruction boundary.
2. **Serde checkpoint:** convert to an ID-based snapshot DTO; same build, fresh
   VM, deterministic pure computation, no live handles.
3. **Multiple tasks/async:** only when a nonblocking adapter exists.
4. **Migration:** only when a real use case justifies handle and effect policy.

This is one representation growing capabilities, not four capabilities promised
at once. It preserves the day-one frame constraint without burdening v1 with a
scheduler.

Serde should be the format plumbing, not the graph model. Serde's documented
`rc` feature serializes the inner value at every reference and does **not**
preserve identity. Build an explicit object-ID DTO first, then derive
`Serialize`/`Deserialize` on that DTO.

### P1.2 — Live handles should be rejected by the first snapshot format

**Type:** Architecture · Correctness
**Where:** `ADR.md` ADR-005, ADR-016; `TRAPS.md` handle validity

A generational table key detects reuse inside one table; it is not resource
identity and carries no reacquisition intent by itself. Reopening a path also
does not reproduce a file descriptor's offset, flags, locks, or external world.
Redialing a socket cannot reproduce a connection.

**Recommendation:** milestone 8 should return a typed `SnapshotHasLiveHandles`
error. Add adapter-specific checkpointing only as a later opt-in capability.
This makes the first pause/resume test strong without quietly defining a
distributed-effects protocol.

### P1.3 — `Value` is presented as closed but already omits settled types

**Type:** Consistency · Architecture
**Where:** `ADR.md` ADR-010, ADR-018; `QUESTIONS.md` Q3

The “concrete enum” omits `Bytes` and `Buffer`, which ADR-018 has already settled,
and omits `Keyword`, which the examples and inherited surface already require.
It also says `Cell(Rc<Cell>)` despite the cell-heap design conflict above.

**Recommendation:** either supersede ADR-010 with the final v1 enum before
milestone 1, or label the listing explicitly illustrative and non-exhaustive.
Keywords should be a distinct variant backed by the same intern table; a flag bit
inside `SymId` saves little and makes type predicates, printing, and host
conversion less obvious.

### P1.4 — Define a minimum semantic surface, not a broad Clojure promise

**Type:** Correctness · Agent usability
**Where:** `ETHOS.md`; `ADR.md` ADR-007, ADR-012, ADR-018; `QUESTIONS.md` Q3,
Q10–Q13, Q16

“A small Lisp in the Clojure dialect” is good orientation but too broad as an
implementation oracle. Sets, characters, variadic functions, evaluation order,
duplicate map keys, `NaN`/signed-zero equality, exception values, and collection
cross-type equality are not specified. Agents will fill those gaps differently.

**Recommendation:** add one compact v1 surface table with three columns:
**in**, **deliberately different**, **deferred**. Define only syntax and semantic
edges needed by milestones 1–6. In particular, settle left-to-right evaluation,
arity behavior, integer overflow, float equality/hashing (including NaN and
`-0.0`), and which collection literals exist. Do not write a full grammar or
standard library plan.

### P1.5 — Open questions are grouped by topic rather than actual dependency

**Type:** Mechanical · Agent usability
**Where:** `QUESTIONS.md`; `BUILD.md`

Q1 is now correctly deferred to milestone 9. The rest still need the same
treatment: Q10 blocks milestone 3; Q11 blocks milestones 3 and 5; Q12 blocks
read-time syntax-quote and milestone 5; Q13 blocks maps/equality. Q2 is resolved
even though the file says resolved questions leave. Q17 and Q19 reopen active
ADRs but are separated from the questions they affect.

**Recommendation:** organize the file by **must decide before milestone N** and
put the milestone on every question. Remove Q2 while preserving the number gap.
Move Q17 beside recursion and Q19 beside the cell/ownership decision. This turns
`QUESTIONS.md` into a work queue instead of another essay.

### P1.6 — The corpus does not yet specify failure output or nondeterministic effects

**Type:** Verification
**Where:** `BUILD.md` corpus and serialization property

Milestone 4 puts failures in the corpus, but the four files provide only stdout
and a final value. There is no canonical outcome record for thrown values,
diagnostics, exit status, or stderr. The resume oracle also compares stdout after
host I/O exists, without defining whether already-emitted output is replayed,
captured, or external.

**Recommendation:** make `.out` a canonical execution transcript containing
status, final/thrown value, stdout, and diagnostics. Run the first serialization
property against a buffered in-memory host so effects are part of the comparison.
Real filesystem/stdio checkpoint behavior should not be implied.

## P2 findings — factual, internal, and mechanical cleanup

### P2.1 — `Rc` equality is described incorrectly

**Type:** Factual
**Where:** `TRAPS.md` “Equality vs. identity”; `GLOSSARY.md` structural equality

Rust `Rc<T>`'s `PartialEq` compares the pointees using `T::eq`; pointer identity is
the explicit `Rc::ptr_eq` operation. Deriving `PartialEq` on `Value` can still be
wrong for the language—most obviously because enum derivation cannot make a list
equal a vector—but the stated mechanism is false.

**Recommendation:** replace the trap with: “Derived equality follows Rust variant
and payload equality; language equality may cross representation/variant
boundaries. Use `Rc::ptr_eq` only for explicit identity.”

### P2.2 — The WasmGC rationale is false even if the decision is reasonable

**Type:** Factual · Architecture
**Where:** `ADR.md` ADR-019

WasmGC provides managed struct and array references. It does not require compiling
guest-language calls onto the WebAssembly call stack; an interpreter can keep its
explicit frames in GC-managed arrays just as it can in linear memory. Therefore
WasmGC does not inherently forfeit serializable explicit frames.

**Recommendation:** keep linear memory, but justify it with the real trade-offs:
Rust/toolchain support, portability, control over layout, ease of producing a
canonical byte-oriented image, and avoiding dependence on host GC behavior.

### P2.3 — TinyGo/size and speed claims need scope labels

**Type:** Factual · Editorial
**Where:** `ADR.md` ADR-001, ADR-006, ADR-012; `PRIOR-ART.md`

- The Go `~2MB` floor is documented for the standard Go wasm toolchain, but
  TinyGo commonly produces much smaller binaries.
- TinyGo uses Asyncify for goroutines on WebAssembly, but its scheduler can be
  disabled. “TinyGo drags in Asyncify” is therefore too categorical.
- “Rust costs ~30% more lines,” “transducers are faster,” and several absolute
  line-cost claims are hypotheses or local measurements, not general facts.
- ADR-006 says monotonic slots avoid wider encoding, but lack of reuse can
  increase the maximum slot index and therefore operand width.

**Recommendation:** tag measurements with repo/toolchain/date/workload, and use
“expected” or “in the sibling measurement” for projections. The decision does
not become weaker when its evidence is accurately scoped.

### P2.4 — “Hygiene” overstates the Clojure-style guarantee

**Type:** Factual · Terminology
**Where:** `ADR.md` ADR-024; `GLOSSARY.md`

The Clojure reader does resolve syntax-quoted symbols and supports auto-gensyms.
That is useful capture avoidance, but Clojure macros are not hygienic in the
strong `syntax-rules` sense and can still construct capturing symbols.

**Recommendation:** title ADR-024 “Clojure-style capture avoidance rides on
qualified symbols and gensym.” Reserve “hygienic” for the stronger property or
say explicitly that correct macro authors must use syntax-quote/gensym.

### P2.5 — Recent supersession has left stale normative statements

**Type:** Consistency
**Where:** `ADR.md` introduction and ADR-009; `GLOSSARY.md` lines 9–18;
`QUESTIONS.md` Q2

- The ADR introduction says ADR-021 is new and omits ADR-022–024.
- ADR-009 still says Q2 is unresolved and that hygiene rides on metadata.
- The glossary still calls Q2 unresolved even though later glossary entries cite
  ADR-023/024 correctly.
- `QUESTIONS.md` says resolved questions leave, but retains Q2.

**Recommendation:** treat ADR-023 and ADR-024 as superseding ADR-009, which permits
the old text to remain historically intact with one status line. Update the ADR
provenance paragraph, glossary, and questions file in the same commit. Add a tiny
cross-reference check that fails when `Qn, unresolved` coexists with a resolving
ADR.

### P2.6 — The line budget arithmetic is wrong

**Type:** Mechanical
**Where:** `BUILD.md` line-budget table

The rows total **5,300**, not `~5,200`.

**Recommendation:** correct the total or alter a row intentionally. If line count
is an oracle, compute the displayed total from the same configuration used by the
check rather than maintaining two numbers.

### P2.7 — “Append-only oracle” is misleading terminology

**Type:** Editorial · Process
**Where:** `BUILD.md` “Two rules”

Golden files must change when this intentionally unstable language changes. The
paragraph correctly requires a reviewed diff, so the oracle is not actually
append-only.

**Recommendation:** call it **review-gated**: “No golden update without a reviewed
behavioral diff and reason.” Keep “append-only” only for the ADR history, where it
has its literal meaning.

### P2.8 — Normative and historical documents are mixed in one flat directory

**Type:** Mechanical · Agent usability
**Where:** `docs/ethos-review.md`, `docs/spec-review-v0.1.md`, `docs/PRIOR-ART.md`

The two old reviews discuss a removed spec and contain recommendations already
superseded by current ADRs. A cold agent using `docs/*.md` as context will ingest
them as if they were current. `PRIOR-ART.md` is valuable evidence, but it is not
required to implement most milestones.

**Recommendation:** add a 10–20 line root `README.md` or `docs/README.md` with:

1. normative reading order: `ETHOS` → active ADRs → current milestone in `BUILD`;
2. consult-on-demand: `TRAPS`, then relevant `QUESTIONS`;
3. evidence only: `PRIOR-ART`;
4. archive: the two historical reviews.

Move old reviews under `docs/archive/` or put an unmistakable non-normative banner
on them. Do not add more narrative; add routing.

### P2.9 — Dependency and adapter lists do not describe one timeline

**Type:** Consistency
**Where:** `ADR.md` ADR-013/014; `BUILD.md` milestones 7 and 10; `ETHOS.md`

ADR-014 calls `crossterm` and `serde_json` “initial” dependencies, while terminal
and JSON are milestone 10. Host features name HTTP, but the build plan names TCP;
the ethos names web services. This is small, but agents use such lists as scope.

**Recommendation:** change “initial dependencies” to a milestone-indexed list or
say “allowed dependencies when their subsystem lands.” Decide whether the first
network adapter is TCP or HTTP and use one term consistently.

## Recommended smallest coherent v1

The following is not a request for more documentation. It is a suggested line
around the first coherent system:

| Keep in the kernel | State explicitly | Defer without apology |
|---|---|---|
| Reader/printer and span origins | One namespace and global creation rule | General module system |
| Forms-as-values + closed core AST | Left-to-right evaluation and arity | Sets, protocols, multimethods |
| Slot compiler/disassembler | Tail-call and handler invariants | Transients until collection pressure is real |
| One explicit-frame execution | Cell ownership/lifetime | Multi-task scheduler and nonblocking I/O |
| `Rc` immutable values | Deterministic printing/equality/hashing | Inline caches until there is a call path |
| Generational host handles | Snapshot rejects live handles | Handle reacquisition and migration |
| Fuel-based suspension | Same-build, fresh-VM image | Cross-build snapshot compatibility |
| ID-based snapshot DTO + Serde | Buffered-host resume oracle | Distributed migration |

This retains the interesting part: the execution state really can stop at an
arbitrary bytecode boundary, become ordinary data, and resume elsewhere in the
same build. It also keeps the kernel explainable. Async and migration can later
reuse the shape without being allowed to dictate it now.

## Recommended button-up sequence

1. Write one ADR choosing the cell representation and lifetime policy.
2. Supersede ADR-005/017 with the narrow v1 `Vm`/`Execution`/`Image` boundary.
3. Supersede ADR-009 with the complete span-origin rule and corrected tests.
4. Resolve globals, one-namespace v1, self recursion, and module-level-only
   mutual recursion together.
5. Resolve proper tail calls together with handler/finalizer frame state.
6. Add the small v1 surface table and reorder questions by milestone.
7. Correct the factual/mechanical items and add a routing page.
8. Start milestone 1. Leave collection representation open until milestone 6 and
   keep async/migration beyond the first serialization proof.

This is approximately five architectural decisions plus cleanup. If the
button-up pass starts producing a grammar, package design, scheduler API, or
standard-library catalogue, stop: that would be the scaffolding beginning to tie
the implementation's hands.

## Primary references used for factual checks

- Rust [`Rc`](https://doc.rust-lang.org/stable/std/rc/struct.Rc.html) and
  [`Rc::ptr_eq`](https://doc.rust-lang.org/stable/std/rc/struct.Rc.html#method.ptr_eq)
- The Rust Book on
  [`Rc<RefCell<T>>` and interior mutability](https://doc.rust-lang.org/book/ch15-05-interior-mutability.html)
- Serde's [`rc` feature and identity limitation](https://serde.rs/feature-flags.html#-features-rc)
- The official [Clojure reader reference](https://clojure.org/reference/reader)
  for syntax-quote qualification and auto-gensym
- The WebAssembly GC proposal's
  [aggregate types](https://webassembly.github.io/gc/core/syntax/types.html#aggregate-types)
- Official Go documentation on the standard toolchain's
  [WebAssembly binary-size floor](https://go.dev/wiki/WebAssembly#reducing-the-size-of-wasm-files)
- TinyGo documentation on
  [WebAssembly goroutines/Asyncify](https://tinygo.org/docs/concepts/compiler-internals/datatypes/#goroutine)
  and [disabling the scheduler](https://tinygo.org/docs/guides/optimizing-binaries/)

The quantitative prior-art claims were also checked against the local sibling
repositories named in `PRIOR-ART.md`. They are credible as local measurements;
the recommendation is to label them as such rather than generalize them.

## Appendix A — Review prompt (verbatim)

```text
Please review the pre-project design scaffolding we've created so far. Check for internal consistency, correctness/factualness, overall shape and architecture, and mechanical layout, structure, and anything else you can think of. My concern is to get this buttoned up before we start. Enough context to guide our angets, but not so much to tie their hands, or burden them with unecessary context. Feel free to get pedantic, but clearly dilineate the different types of your feedback. Treat it somewhat as a PR review, even though this is pre-project documentation. Write your report as a markdown file with specific findings and recommendations.  What's most important to me is preserving the "nanochat-like grokkability" + "clean, fast-enough architecture with good choices and reasoned trade-offs" + (last) "maybe a novel feature like pause/resume serde" -- above all, I want agents to be able to run with the spec and design and create something nice that fits in my head, along the guidelines and principles I've discussed and encoded. Include this prompt in the appendix of your review.
```
