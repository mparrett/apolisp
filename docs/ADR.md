# Architecture Decision Record

**Append-only.** To change a decision, add a new entry that supersedes the old
one. Do not edit a past entry except to add a `Superseded by` line. There are no
version bumps and no amendment procedure. The point is not stability — it is
never having to re-derive last month's reasoning.

Each entry: **decision · why · cost · rejected**. Every entry below is Active
unless marked otherwise. The rationale for this format is ADR-022; unfamiliar
terms are in `GLOSSARY.md`.

ADR-001 through ADR-015 are ported from SPEC v0.1 Part III (commit `c494f2a`).
ADR-016 through ADR-020 are ported from SPEC v0.1 Part II, where they were
settled design that had never been written up as decisions. ADR-021 through
ADR-029 are new. Factual corrections to entries whose decision still stands are
in **Errata** at the end, not in the entries themselves.

---

### ADR-001 — Rust as the host language

*Decision stands; evidence rescoped by errata E-2, E-3, E-4.*

**Decision.** Rust, single-threaded core, no async runtime in the VM.

**Why.** Small wasm binaries (~200KB vs ~2MB for Go), and distribution is how the
thing gets used at all. Concrete enums and `match` are both the fastest and the
most legible construct available. No GC to inherit means no GC to explain.

**Cost.** ~30% more lines than Go. Borrow-checker friction in the compiler,
mitigated by ADR-020 (the VM owns everything; pass `&mut Vm`).

**Rejected.** *Go* — smaller line count and a free GC, but fat wasm binaries and
TinyGo drags in Asyncify. *OCaml* — excellent for compilers, outside the working
toolchain.

---

### ADR-002 — Immutable locals, flat closures

**Decision.** Locals are immutable. Closures copy their captures into an
`Rc<[Value]>` at creation. No open upvalues, no closing, no aliasing.

**Why.** Deletes the entire upvalue apparatus — open/closed lists, sorted
tracking, close-on-scope-exit — that a mutable-locals language requires. The
largest single simplification available, and it comes from a *language* decision
rather than an implementation trick.

**Cost.** Mutual recursion needs an explicit mechanism. Capture-heavy code
allocates slightly more.

**The recursion exception.** Mutually recursive `let` bindings cannot be captured
flat, because each closure must reference the other before either environment
exists. Therefore:

- self-recursion resolves through the function's own identity, not a capture;
- mutual recursion lowers to explicit identity cells, initialized once;
- v1 may restrict mutual recursion to module-level bindings.

This corner must not be allowed to push all environments into `RefCell`.

**Rejected.** Lua-style upvalues; boxed mutable environments.

---

### ADR-003 — Reference counting, no tracing GC

**Decision.** `Rc` throughout. No collector.

**Why.** Immutable locals mean the only cycle risk is explicit cells. A tracing
GC is 500–2,000 lines that touches every subsystem, and is the main reason VMs
become unreadable.

**Cost.** Cycles through cells leak. Refcount traffic on hot paths.

**Mitigation.** `Weak` for known-cyclic patterns, or accept the leak — a
single-user language may leak knowingly.

**Rejected.** Mark-sweep; WasmGC (ADR-019).

---

### ADR-004 — Explicit frame stack; the interpreter never recurses

**Decision.** Calls push frames into a VM-owned `Vec`. The dispatch loop is flat.
The Rust stack is empty at every instruction boundary.

**Why.** This single decision yields serialization, migration, coroutines,
generators, stack-depth limits, step limits, inspectable backtraces, and
independence from Asyncify — all from one piece of work.

**Cost.** Macroexpansion and any host callback that re-enters the VM must
trampoline rather than recurse. Bounded, and worth it. Note the consequence:
compilation is not a pure function of source — it requires a live VM, because
macros are language code.

**Rejected.** Recursive `eval`; host-stack-based calls with Asyncify or JSPI for
suspension.

---

### ADR-005 — Serializable machine state is a first-class property

*Superseded by ADR-029. The property stands; the state tuple below is incomplete
and the boundary is now Vm / Execution / Image.*

**Decision.** VM state is plain data with a canonical encoding. Suspension,
async, and migration are one mechanism, built once.

Machine state is exactly:

```
(constants, code, frame stack, value slots, cell heap, handle table, intern table)
```

Every element is serializable except the handle table, which serializes as
identity plus reacquisition intent.

**Why.** This is priority #2. Naming it as a property settles things that are
otherwise left open: whether handles may appear in serialized state (they may, as
identity + reacquisition intent), whether the intern table is part of a snapshot
(it is), whether `Value` has a canonical encoding (it must).

**Cost.** Host handles cannot be embedded as raw pointers. Every host adapter
must define its reacquisition semantics or explicitly refuse to migrate.

**Verification.** Round-trip is a property test, not a hope. See `BUILD.md`.

**Open.** Sharing/cycle preservation in the encoding, and symbol-id remapping into
a populated VM, are unresolved — Q5, Q9.

---

### ADR-006 — Slot-based bytecode, no register allocator

*Decision stands; one claim corrected by erratum E-5.*

**Decision.** Numbered operand slots per function, allocated monotonically. No
liveness analysis, no reuse in v1.

```
CONST   r0, 10
CONST   r1, 20
ADD     r2, r0, r1
RETURN  r2
```

**Why.** Captures most of a register VM's dispatch advantage over a stack VM
without the lowering complexity, the wider encoding, or the class of compiler
bugs that liveness and moves introduce.

**Cost.** Larger frames than necessary. Fixable later by an optional last-use
reuse pass that changes no semantics.

**Rejected.** Stack VM (simpler, more dispatches); full register allocation
(premature).

---

### ADR-007 — Closed core, open reader, open macros

**Decision.** Three layers, with exactly one closed:

| Layer | Openness | Rationale |
|---|---|---|
| Reader | **Open**, scoped (ADR-008) | All surface-syntax weirdness lives here |
| Macros | **Open** | Primary semantic extension seam |
| Core forms | **Closed**, ~12 | Read the compiler, know the language |

```
literal   local   global   if    do    let   fn
call      quote   set-cell!   throw   try
```

Macros may introduce arbitrary syntax and may change *when* code runs (a `go`
block is a macro). Macros may **not** add new primitive operations to the VM.
That is the actual line, and it is what keeps the kernel inspectable.

**Why.** The kernel is the thing that must be readable to understand the
language. Every experiment worth running fits in the reader or a macro; `go`
blocks are a CPS-transforming macro, which is proof of how much headroom that is.

**Cost.** Some experiments will feel like they *want* a new special form. Treat
that feeling as a signal to look harder at the macro.

**Rejected.** An extensible special-form registry — it blocks inline caching and
makes the compiler no longer authoritative about the language.

**Open.** `loop`/`recur` (Q5).

*Amended by ADR-027: the closed core gains one top-level create-or-rebind
operation, making it 13 forms. ADR-028 fixes `finally` as part of `try`'s shape
rather than a separate form.*

---

### ADR-008 — Reader configuration is file-scoped and declared

**Decision.** Reader macros are registered per file, declared near the top, and
frozen for the duration of that file's parse.

**Why.** A globally mutable reader table makes a file unparseable without
replaying the reader history that preceded it. That contradicts constraint #1 —
you can no longer hold the system at once, because the system now includes its own
history. It also makes `.forms` golden snapshots order-dependent, and flapping
snapshots kill the oracle. This is the Racket `#lang` shape.

**Cost.** Cannot mutate the reader mid-file.

**Rejected.** Runtime-global mutable reader table.

**Open.** Whether the REPL session is its own parse unit, with a freely mutable
table — Q1.

---

### ADR-009 — Metadata on forms from day one

*Superseded by ADR-026 (spans) and ADR-024 (capture avoidance). "Every form
carries metadata" is not the decision that survived — origins belong to positions
in a tree, not to values.*

**Decision.** Every form carries metadata. Source spans are populated by the
reader and preserved through macroexpansion.

**Why.** Two independent requirements ride on one mechanism: usable error
messages (the development feedback loop, not a nicety) and namespace-qualified
symbols for macro hygiene. Cheap to build in, miserable to retrofit.

**Cost.** Memory per form; a little plumbing in every reader path.

**Open.** Whether forms *are* `Value`s, and where metadata lives if so — Q2. This
ADR is unimplementable until that is settled.

---

### ADR-010 — `Value` is a concrete enum; its size is asserted

*Superseded by ADR-025. The asserted size survives; the enum below is missing
`Keyword`, `Bytes`, and `Buffer`, and its `Cell(Rc<Cell>)` is unimplementable
alongside ADR-020. Size reasoning corrected by erratum E-8.*

**Decision.** Plain Rust enum, `Rc` payloads, no NaN-boxing. A test asserts
`size_of::<Value>() <= 24`. Construction goes through constructor functions so
the representation can change without touching the compiler.

```rust
enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<StrObj>),
    Sym(SymId),          // interned, pointer/index equality
    List(Rc<ListObj>),
    Vec(Rc<VecObj>),
    Map(Rc<MapObj>),
    Fn(Rc<Closure>),
    Cell(Rc<Cell>),      // the only mutable language object
    Handle(HandleId),    // opaque host resource
}
```

**Why.** Legible, no allocation for primitives, and the compiler can see the whole
state space. NaN-boxing is a legibility disaster for a modest win. Size is
asserted, not assumed — `Rc<str>` and `Rc<[T]>` are fat pointers.

**Cost.** 24 bytes rather than 16. Accepted.

**Rejected.** NaN-boxing; thin-pointer wrappers with an extra indirection.

**Open.** No `Keyword` variant exists, and the host examples already use keywords
— Q3.

---

### ADR-011 — One representation per collection type in v1

**Decision.** One implementation each for list, vector, map. Construction behind
constructor functions.

**Why.** Tiered small/large representations are a real win eventually and pure
speculation now. The constructors preserve the seam at zero cost.

**Rejected.** HAMT + RRB from the start (~1,200 lines, the single largest chunk
of a Clojure-alike, unjustified before workloads exist).

**Open.** Which representation, and whether transients exist — Q6. Interacts
badly with ADR-012.

---

### ADR-012 — Eager sequences; no laziness

*Decision stands; one claim corrected by erratum E-6.*

**Decision.** No lazy seqs. Transducers for composition.

**Why.** Laziness is Clojure's most expensive elegance: it infects every
collection operation, forces chunking hacks, wrecks locality, and moves errors far
from their cause. Transducers deliver the composition benefit and are faster.

**Cost.** Infinite sequences require explicit generators. Under ADR-004 those are
nearly free. Reduce-into-a-collection becomes the default idiom, which makes Q6
sharper than it looks.

---

### ADR-013 — Language features are unconditional; only host adapters are gated

**Decision.** Cargo features gate host capability — terminal, HTTP, JSON,
filesystem, RNG. They do **not** gate macros, metadata, or exceptions. Authority
reduction happens at runtime via the host registry.

**Why.** Feature-gating core semantics turns one system into 2ⁿ systems, a direct
assault on constraint #1. The VM knows exactly one generic `HostCall` opcode;
which host functions exist is a registry question, not a compilation question.

The features are not there for users — there are none. They are the **subtraction
test harness**: building with a host adapter cut out is how "seams exist for
subtraction" stays a fact rather than a feeling.

**Cost.** Minimum binary carries the full language even when an app uses little of
it. Acceptable — the language is the small part.

**Rejected.** `#[cfg(feature = "macros")]`; per-profile language dialects.

---

### ADR-014 — Delegate standardized machinery, own everything semantic

**Decision.** Initial dependencies: `slotmap` (generational handle table),
`unicode-segmentation`, `crossterm`, `serde_json`. Later, as subsystems appear:
`bytes`, `miette`, `rand`.

**Why.** The test is *"would implementing this teach us something about our
language, or merely reproduce a protocol or standard algorithm?"* Unicode
segmentation, terminal portability, and JSON edge cases are the least readable and
most failure-prone lines we would otherwise write.

**Owned outright, never delegated.** Form representation and metadata,
reader-macro dispatch, macro expansion, core AST, closure capture, bytecode and
compiler, instruction dispatch, frame and task representation, error semantics,
collection semantics, `Value`, and every host-boundary conversion.

**Rejected.** `logos` — a Lisp tokenizer is ~150 lines, and a generated lexer
fights character-level reader-macro dispatch. `hashbrown` — no measured need at
this scale.

---

### ADR-015 — One file until it hurts; module paths chosen up front

**Decision.** Start as one file with inline `mod` blocks. The module paths are
chosen now so extraction into files later is a move, not a redesign:

```
value  form  reader  expand  compile  bytecode  vm  host  error  main
```

Progression, stopping at the first level that suffices: one file → one crate with
file modules → library + binaries → workspace *only* where a deployment boundary
justifies it.

**Why.** The seams are for subtraction, not organization (see `ETHOS.md`). One
file keeps the whole thing readable in one pass, which is constraint #1 stated
concretely; the `mod` blocks mark where a subsystem could be cut or lifted.
Fifty fragments and a bare monolith both fail, for opposite reasons.

**Cost.** A large file eventually strains tooling. That is the "until it hurts"
trigger, and it is a real signal rather than a schedule.

**Rejected.** Leading with a ten-file layout (SPEC v0.1's framing; the module list
survives, the priority does not). A generated amalgamated reading view is
unnecessary while the source is already one file.

---

### ADR-016 — All host interaction goes through opaque generational handles

*(Ported from SPEC v0.1 Part II.)*

**Decision.** Language code never manipulates a Rust object directly. The VM owns
the handle table, lifetimes, foreign calls, and cleanup.

```clojure
(io/open path :read)   ; → Handle
(io/read h buf)
(io/close h)
```

Handles are generational, so a stale handle is an error rather than a silent
alias. Resource lifetime is **explicit**: `with-open` lowering to `try`/`finally`,
plus idempotent `close`.

**Why.** It is the only representation of a host resource that survives ADR-005 —
a raw pointer cannot be serialized, an index into a generational table can.

**Cost.** Refcount destruction is not available as a resource-management contract;
lifetime must be written out.

---

### ADR-017 — Async and migration are the same mechanism

*Superseded by ADR-029. One representation still serves all three, but "cost:
none beyond ADR-004" was false.*

*(Ported from SPEC v0.1 Part II.)*

**Decision.** One mechanism serves suspension, async, and migration.

```
host call returns:  Ready(v) | Pending(handle) | Error(v)
task states:        Running → Waiting(h) → Runnable → Completed | Failed
```

On `Pending`, the current task's frames are already plain data, so suspension is a
move, not a capture. The host event loop marks readiness and reschedules.
Serialize the same structure to disk or a socket and that is migration.

v1 ships blocking I/O only. The frame representation must nonetheless be
suspension-ready from the first commit.

**Why.** Building suspension and migration twice is the failure mode to avoid, and
under ADR-004 they are already the same data.

**Cost.** None beyond ADR-004, which is what pays for it.

**Rejected.** Asyncify; the stack-switching proposal; any platform dependency.

**Open.** With blocking-only I/O there is no `Pending` in v1, so what the
milestone-8 property test actually suspends on is unresolved — Q7.

---

### ADR-018 — Strings are not sequences; text/bytes conversion is explicit

*(Ported from SPEC v0.1 Part II.)*

**Decision.**

```
String    immutable UTF-8, Rc-backed, byte-indexed
Bytes     immutable byte sequence
Buffer    mutable byte buffer, VM- or host-owned
```

No promise of O(1) character indexing. Separate explicit operations for bytes,
scalar values, and graphemes. A builder for repeated concatenation. File and
network APIs speak bytes; `read-text` exists but has defined UTF-8 error
behavior.

**Why.** Treating strings as generic sequences is where a Unicode-correct runtime
either gets slow or gets wrong, and the conversion points are exactly where the
bugs live.

**Cost.** More surface area than `(count s)` implying one thing.

---

### ADR-019 — Wasm via linear memory, not WasmGC

*Decision stands; rationale corrected by erratum E-1.*

*(Ported from SPEC v0.1 Part II.)*

**Decision.** Ship the VM as a wasm module with values in linear memory. Offer a
"never reclaim" mode that skips refcount traffic entirely, for short-lived
invocations.

**Why.** WasmGC requires compiling the language directly to wasm, which puts
frames back on the wasm stack and forfeits constraint #2. Refcounting in linear
memory suffices, because immutable locals mean the only cycle risk is explicit
cells (ADR-003).

**Cost.** The never-reclaim mode is not "a null collector" — there is no
collector. Getting the win means a different `Drop` story for `Value`, not merely
declining to free.

**Rejected.** WasmGC.

---

### ADR-020 — Three layers of mutation; the VM owns all mutable state

*(Ported from SPEC v0.1 Part II.)*

**Decision.** Mutation exists in exactly three places, named so that "immutable
locals" is not mistaken for "immutable language":

1. **Lexical values** — immutable slots, flat captured environments. No cells, no
   interior mutability, no `RefCell`.
2. **Language identity cells** — explicit heap objects: atoms, globals, recursive
   bindings, hot redefinition. Mutation here is visible in the source.
3. **VM-owned state** — modules, intern tables, host handles, caches, scheduler.
   Reached only through `&mut Vm`.

Nothing in the core uses `Rc<RefCell<_>>`.

**Why.** It is the mitigation for ADR-001's borrow-checker cost: a single owner
means the compiler and VM pass `&mut Vm` and stop arguing. It is also what makes
ADR-005 possible — you cannot snapshot state you do not own.

**Cost.** Anything wanting shared mutability must go through a cell, visibly.

---

### ADR-021 — Optimization gates are blast radius, not profiling

*(New, 2026-07-25, from the ethos review.)*

**Decision.** An optimization is gated by how far it reaches and whether it is
reversible:

| Class | Examples | Gate |
|---|---|---|
| Local, reversible, semantics-preserving | inline caches, superinstructions, slot reuse, narrow fast paths | **None.** Do it whenever. |
| Large but self-contained | tiered collections (~1,200 lines) | The line budget in `BUILD.md`. |
| Reaches every subsystem or costs legibility | NaN-boxing, alternative `Value` layouts, numeric specialization | Constraint #1. Needs an argument, not a benchmark. |

**Why.** "Measure before optimizing" is calibrated for people who owe someone
stability. Here,
performance-per-line is constraint #3 *and* a reason the project exists; deferring
it behind a measurement gate reclassifies the motivation as a risk. The golden
corpus is what makes this safe: with four snapshots per program, a reckless
optimization that changes semantics is caught in seconds.

**Cost.** Some optimizations will turn out not to have mattered. That is an
acceptable price for not having to ask permission.

**Rejected.** SPEC v0.1's flat "deferred until profiling names a specific
pressure" list, which collapsed three different gates into one.

---

### ADR-022 — Decisions live in one append-only file

*(New, 2026-07-25.)*

**Decision.** This file. One document, entries appended in number order, never
reordered. To change a decision, add a new entry that supersedes the old one and
add a `Superseded by ADR-NNN` line to the old entry — that line is the only edit
a past entry ever receives. No version bumps, no amendment procedure, no approval
step.

A decision earns an entry when reversing it would touch more than one subsystem,
or when the reasoning is something we would otherwise re-derive badly in three
months. Everything else is code, and code is cheap to change.

**Why.** One file is one read, with no directory to walk — the same reason the
source is one file (ADR-015), and constraint #1 applied to prose. Append-only is
what makes the freedom clause safe to exercise: breaking the language on a Tuesday
costs nothing, but silently losing *why* Monday's version existed costs a
re-derivation every time the question resurfaces. Supersession keeps that trail
without acquiring a governance process, which is what SPEC v0.1's "amendments are
deliberate acts with a version bump" was — governance language written for a
language with users, of which there are none.

**Cost.** The file grows monotonically and superseded entries stay in it, so
reading front-to-back eventually means skimming past dead decisions. The status
line at the top of a superseded entry is what keeps that cheap. If it stops being
cheap, that is a real signal and the answer is an index at the top, not a purge.

**Rejected.** *A `decisions/NNN-*.md` directory* — more files than decisions, and
a directory walk to answer any question that spans two of them. *Amending entries
in place* — loses the trail, which is the only thing the log is for. *Version
bumps* — see above.

---

### ADR-023 — Spans live on the parent, indexed by child; and on code, indexed by instruction

*Forms-are-values stands and everything later builds on it. The span mechanism is
superseded by ADR-026, which found it is not closed under macro construction; the
serialization argument below is corrected in ADR-029.*

*(New, 2026-07-25. Resolves Q2. Refines ADR-009.)*

**Decision.** Forms **are** `Value`s. There is no separate `Form` type and no
conversion at the macro boundary.

Source position has two halves, and both are positional rather than attached:

1. **Forms.** A list or vector node carries `child_spans: Rc<[Span]>`, one entry
   per child. A node's span comes from its *parent*, indexed by its position; the
   root carries its own. Immediates and heap objects are covered by one
   mechanism, with no `Option` and no metadata field on `SymObj` or `StrObj`.
2. **Code.** The compiler emits a `lines` array parallel to the bytecode:
   `lines[i]` is the span of the instruction at `i`. Without this, runtime errors
   and backtraces have no position to report and the disassembler has none to
   print.

Metadata is reader-owned and compiler-consumed. Language code cannot read a
span or attach its own; there is no `with-meta` in v1.

**Why.** Forms-are-values is what "inherit Clojure's surface" means for macros —
quasiquote is list construction, `map` over a form works, and no accessor API has
to be invented. It also serializes for free: macroexpansion runs in the VM
(ADR-004), so a snapshot can contain forms mid-expansion, and a separate `Form`
type would need a second encoder for constraint #2.

The parent-indexed trick is what makes that affordable. The naive version of
forms-are-values (Clojure's `IObj`, metadata on heap objects) cannot attach a
span to `Sym(SymId)` or `Int(i64)`, because they are immediates — and a symbol
occurrence is exactly what an unresolved-var or arity error wants to point at.
The alternative, `Sym(Rc<SymObj>)`, buys that back at the price of an allocation
per symbol occurrence and a deref on the hottest comparison in the compiler.
Storing child spans on the parent gets symbol precision while `Value` stays 24
bytes (ADR-010) and symbol equality stays an integer compare.

The second half is not optional. reg-lisp carries `Line`/`Col` on its AST *and*
a `lines` array parallel to bytecode, and needed both; we had specified only the
first (`PRIOR-ART.md`).

**Cost.** A form detached from its parent loses its span. This is not a new
failure mode — anything a macro rebuilds already loses spans — but it makes the
rule uniform rather than incidental, and it means "span" is a property of a
position in a tree, not of a value.

The compiler's input type is not closed: a macro can return a closure or a handle
into code position, so the compiler needs a runtime check and a decent error for
"this isn't code." Roughly a dozen match arms and one error message.

**Rejected.** *A separate `Form` type with `Value::Form`* — every node gets a
`Meta` slot with no `Option`, which is the best error story available, but it
costs a second enum, a second printer, a second serializer, a quasiquote written
against `Form`, and it abandons Clojure's macro surface. It also blows the
600-line reader/forms budget. *Conversion at each macro boundary* — spans die on
the return trip, plus a full tree conversion per macro call; reg-lisp's
`valueToNode` had to become iterative on both axes and cap nesting depth
(ADR-013 there). *`Sym(Rc<SymObj>)`* — see above. *Metadata visible to language
code* — Clojure says yes and pays for it everywhere; nothing in v1 needs it.

---

### ADR-024 — Hygiene rides on symbol naming, not on metadata

*Read as "Clojure-style capture avoidance" — see erratum E-7.*

*(New, 2026-07-25. Supersedes the hygiene half of ADR-009.)*

**Decision.** Macro hygiene is a property of symbol *identity*, not of form
metadata. Syntax-quote resolves symbols to fully qualified names at read time,
and gensym supplies the rest. Form metadata carries spans and nothing else.

**Why.** ADR-009 justified metadata with two requirements at once — error
messages and namespace-qualified symbols for hygiene — and claimed they ride on
the same mechanism. They do not. Clojure resolves syntax-quoted symbols at read
time; hygiene is in the interned name before any metadata is consulted.

Unbundling them matters because the two are held to different standards.
Capture avoidance is a correctness property — a macro that captures a binding is
broken. Spans are best-effort and degrade gracefully. Fusing them made the
metadata mechanism look like it had to be total and precise, which is what made
the expensive options in Q2 look necessary.

**Cost.** Read-time resolution means syntax-quote needs the current namespace,
so the reader is namespace-aware — a real coupling, and it lands on Q12.

**Rejected.** Full hygienic macro expansion in the `syntax-rules` sense. Clojure's
resolve-plus-gensym is weaker, well-understood, and the surface we said we would
inherit.

---

### ADR-025 — Cells are VM-owned ids; the v1 `Value` enum

*(New, 2026-07-26. Supersedes ADR-010. Refines ADR-003, ADR-020. From the
pre-project review, P0.1 and P1.3.)*

**Decision.** A cell is an index into a VM-owned generational arena, not a shared
pointer. All cell reads and writes go through `&mut Vm`.

```rust
enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<StrObj>),
    Bytes(Rc<BytesObj>),
    Sym(SymId),          // interned
    Keyword(KwId),       // interned in the same table, distinct variant
    List(Rc<ListObj>),
    Vec(Rc<VecObj>),
    Map(Rc<MapObj>),
    Fn(Rc<Closure>),
    Cell(CellId),        // index into Vm.cells
    Handle(HandleId),    // opaque host resource
    Buffer(BufferId),    // mutable, VM- or host-owned
}
```

Immutable values — strings, bytes, collections, closures — stay `Rc`-backed.
`size_of::<Value>() <= 24` remains asserted (ADR-010's one surviving clause). The
listing is the v1 enum, not an illustration; adding a variant is an ADR.

For v1, arena cells are retained for the lifetime of the VM. Instrument the live
count rather than reclaiming.

**Why.** The previous text specified three incompatible ownership models at once:
`Value::Cell(Rc<Cell>)` (ADR-010), no `RefCell` anywhere with all mutable state
behind `&mut Vm` (ADR-020), and a VM-owned cell heap in the state tuple
(ADR-005). `Rc<T>` grants shared *immutable* access; mutating through it requires
interior mutability, which ADR-020 forbids. Only the arena satisfies all three
constraints as written.

Two consequences fall out of the same change. Logical cycles become ordinary id
edges rather than `Rc` cycles, so ADR-003's accepted leak narrows from "reference
graphs leak silently" to "cells are retained, and the count is on screen." And an
id-keyed arena is already the shape a snapshot wants (ADR-029), so constraint #2
stops fighting the value representation.

Keywords get their own variant rather than a flag bit inside `SymId`: the flag
saves one word and makes type predicates, printing, and host conversion all
slightly worse. `Bytes` and `Buffer` were settled by ADR-018 and missing from the
enum.

**Cost.** A cell read is an arena index through the VM rather than a pointer
deref — fine on a slot VM where the VM is already in hand, awkward anywhere that
only has a `Value`. Retention until VM teardown is a real leak for a long-running
simulator; if that bites, the answer is a small tracing pass over the cell arena
alone, not `RefCell` through every environment.

**Rejected.** `Rc<RefCell<Value>>` — the standard Rust shape, and what `../let-rs`
adopted, but it requires superseding ADR-020's no-`RefCell` rule and budgeting an
identity-aware graph serializer, and it reproduces let-rs's unfixable `letrec`
cycle (`PRIOR-ART.md`). *`Rc<Cell>` with `UnsafeCell` inside* — same aliasing
question, less legibility, no serialization benefit.

**Resolves** Q3. **Narrows** Q17 and Q19: with id edges there is no `Rc` cycle to
leak, so the case against refcounting loses most of its force.

---

### ADR-026 — Span origins: positional, total, and explicit about generated code

*(New, 2026-07-26. Supersedes ADR-009. Supersedes the span mechanism in ADR-023;
its forms-are-values decision stands. From the pre-project review, P0.3. Carrier
shape corrected by erratum E-9.)*

**Decision.** Forms remain `Value`s (ADR-023). Spans live *outside* the value
graph, in a carrier the reader and expander thread alongside any value they treat
as code:

```text
LocatedForm { root: Value, origin: SpanOrigin }
SpanOrigin  = Source(Span) | Generated(Span) | Unknown
```

- Every aggregate — list, vector, **and map** — holds one origin per syntactic
  child. The carrier holds the root's origin, so an immediate at the root is
  covered.
- Nodes a macro constructed receive `Generated(call_site)`. A node the expander
  can still identify as passed through unchanged keeps its `Source`. Anything
  else is `Unknown`, which prints as such rather than as a plausible lie.
- The compiler emits `lines` parallel to the bytecode, mapping instruction to
  origin (retained from ADR-023). Without it, runtime errors and backtraces have
  no position to report.
- Language code can neither read nor attach an origin in v1.

Verification splits into four, replacing the single round-trip claim:

1. print/read equality **ignores** origins and tests that data round-trips;
2. a span-invariants property: every `Source` span lies within its file, and
   child-origin arity matches child count;
3. `.forms` and `.expanded` snapshots render origins in a debug mode;
4. one macro diagnostic test pins call-site attribution.

**Why.** Parent-indexed storage was not closed under the operations macros use.
Macros build output with ordinary `list`, `cons`, and quasiquote,
and language code cannot attach metadata — so under ADR-023 as written, macro
output would have carried *no* spans at all, which is worse than Clojure, where
expansion at least inherits the call site. Four cases had no defined behavior: an
immediate at the root, maps, language-constructed aggregates, and a macro
returning one of its arguments unchanged.

The property test was also unsatisfiable as stated. `read(print(read(s))) ==
read(s)` either includes spans, in which case printing moves columns and it
always fails, or excludes them, in which case it cannot catch the metadata loss
it was introduced to catch.

Naming `Generated` and `Unknown` makes degradation explicit and testable instead
of silent — which is what `TRAPS.md` says the dangerous version looks like.

**Cost.** The expander threads a carrier rather than reading metadata off a
value; every path that treats a `Value` as code has to carry origins with it.
That is the price of keeping spans out of the value graph, and the value graph is
what serializes.

**Rejected.** Metadata fields on heap objects (Clojure's `IObj`) — cannot cover
immediates, and `Sym(Rc<SymObj>)` to fix that costs an allocation per symbol
occurrence. Parent-indexed `child_spans` stored in the graph (ADR-023 as written)
— see above. Language-visible metadata and `with-meta` — Clojure pays for these
everywhere and nothing in v1 needs them.

---

### ADR-027 — The v1 boot model: one namespace, VM-owned globals, one rebind operation

*(New, 2026-07-26. From the pre-project review, P0.4. Resolves Q11 and Q17.)*

**Decision.** The smallest thing that lets the language define anything:

- **One** current module/namespace in v1. Not a module system.
- Global names are interned fully qualified.
- The VM owns the global table; each entry is a `CellId` (ADR-025).
- **One** explicit top-level create-or-rebind operation in the core. `def` and
  `defmacro` are library macros over it.
- Self-recursion resolves through the current function's own identity, never
  through a captured cell.
- Mutual recursion is module-level only in v1, via the global table.
- The reader-configuration preamble is fixed built-in syntax. A declaration that
  changes the reader cannot itself require the changed reader to parse it.

**Why.** Milestone 3 requires a recursive function and milestone 5 requires
in-language `defmacro`, but the closed core could *read* a `global` and had no way
to create one — the language could not bootstrap itself. Separately, ADR-024 has
syntax-quote resolving symbols against a current namespace at read time, which
requires a namespace to exist before any module system does.

Module-level-only mutual recursion is a rule rather than a hedge, for the reason
`../let-rs` documents: the VM outlives every closure, so a global cell has an
unambiguous owner. `letrec`-style cells have none, which is why that shape's
cycle cannot be broken and this one's can.

**Cost.** No `require`, no aliasing, no multiple namespaces in v1. Q12 becomes a
question about when that stops being enough, not a blocker.

**Correcting Q17.** The earlier claim that "every `defn` leaks" was wrong: it
assumed a self-recursive closure captures its own cell, which this ADR and
ADR-002 both forbid. Under ADR-025 there is no `Rc` cycle in the first place.

---

### ADR-028 — Proper tail calls, with a handler stack that survives them

*(New, 2026-07-26. From the pre-project review, P0.5. Resolves Q4.)*

**Decision.** Tail calls reuse the caller's frame. `try` carries `finally`, and
active handlers and finalizers live in an explicit handler stack in VM-owned
memory, reachable from the execution image. The invariants, which bind before
frame layout is chosen:

1. Every nonlocal exit runs pending `finally` blocks exactly once.
2. A tail call first discharges cleanups for the scopes it exits. **A call in
   tail position inside a `try` with a `finally` is therefore not a tail call** —
   it becomes an ordinary call, because the frame is still needed.
3. Cleanup may itself throw. The cleanup error wins; the original is retained on
   it as suppressed.
4. Suspension is permitted only at instruction boundaries where handler state is
   inside the `Execution` image (ADR-029).
5. Rust panics are host bugs. They never become language unwinding and never
   cross the VM loop.

**Why.** These were three questions with one answer. `with-open` was promised as
`try`/`finally` lowering while the core-form list had only `try` and no document
said where handlers lived; tail calls were unspecified; and suspension needs to
know what state is live. All three turn on the same structure, and picking frame
layout before deciding it would have meant picking twice.

Tail calls themselves are close to table stakes for a Lisp — a loop that dies at a
fixed depth is a bad surprise, and with eager sequences (ADR-012) iteration *is*
recursion. `../wallisp` measured the throughput cost of TCO at +7% on one
toolchain and −5% on a later one, i.e. within noise and not stable in sign; it
kept TCO on correctness grounds. It also found that TCO fixes the stack and not
the heap — each iteration still allocated a frame that was never reclaimed. Under
ADR-025 frames drop on reuse, so we get that half.

**Cost.** Rule 2 makes tail-call elimination conditional on dynamic extent, which
the compiler must track and the disassembler should show. The alternative
silently skips cleanup.

**Open.** Whether `loop`/`recur` is expressible as a macro over this (Q5) is now
answerable — attempt it.

---

### ADR-029 — `Vm` / `Execution` / `Image`: the v1 snapshot boundary

*(New, 2026-07-26. Supersedes ADR-005 and ADR-017. From the pre-project review,
P0.2, P1.1, P1.2, P1.6. Resolves Q7 and Q9.)*

**Decision.** Three named things, replacing ADR-005's single tuple:

```text
Vm        = intern table + globals/modules + cell arena + handle table
            + host registry config
Execution = code identity + frames + slots + pc + handler stack + status + fuel
Image     = serializable DTO for one Vm plus one suspended Execution
```

v1 constraints, each of which is a promise *not* made:

- Snapshots are taken **only at VM instruction boundaries in already-compiled
  code**. No mid-read, mid-expand, or mid-compile snapshot.
- The suspension trigger is **fuel exhaustion** — a step limit checked at
  instruction boundaries.
- An `Image` is **same-build, fresh-VM only**. Snapshots are disposable.
- Live handles are **refused**: taking an image with a live handle is a typed
  `SnapshotHasLiveHandles` error. Adapter checkpointing is a later opt-in.
- The DTO is explicitly **object-id based**. Serde is format plumbing over it,
  never the graph model — serde's `rc` feature serializes the pointee at every
  reference and does not preserve identity.
- Caches and host registry entries are declared either reconstructible or
  excluded. Nothing is implicitly in the image.

The capability grows in four steps, not four promises: pause/resume on fuel →
serde checkpoint against a buffered in-memory host → multiple tasks once a
nonblocking adapter exists → migration once a real use case justifies handle and
effect policy.

**Why.** ADR-005 said machine state was "exactly" seven things, and the list
omitted task and scheduler state, handler state, globals and modules,
deterministic counters, and any record of effects. ADR-017 then claimed async and
migration cost "none beyond ADR-004," which contradicts ADR-005's own cost
paragraph. Explicit frames make the continuation *representable*; they do not
make graph encoding, external effects, scheduling, or reacquisition free.

The `Vm`/`Execution` split is what makes the promise checkable: an `Image` is one
of each, and anything not in either is out of scope by construction rather than
by oversight.

**Correcting ADR-023.** Its argument that forms-as-values gives free serialization
"because a snapshot can contain forms mid-expansion" was wrong — macroexpansion
is a Rust-side walk, and running a macro *in* the VM does not make the surrounding
walk serializable. The surviving true claim is narrower and still sufficient:
forms are values, so they serialize with the same encoder and need no second one.

**Cost.** Fuel checks on the dispatch loop. No mid-expansion snapshots. The
resume oracle runs against a buffered host, so the first serialization property
proves determinism of pure computation plus captured effects. It claims nothing
about a live socket surviving a move.

---

## Errata

Factual corrections to entries whose **decision still stands**. A wrong reason is
worth fixing even when it reaches the right conclusion, because the reason is
what gets reused. Superseding an entry over a mistaken sentence would be
ceremony; errata are the cheaper mechanism, and the affected entry carries a
pointer line. Where a correction changes a decision, it gets an ADR instead.

**E-1 — ADR-019, WasmGC.** The stated reason is false. WasmGC provides managed
struct and array references; it does not require lowering guest calls onto the
WebAssembly call stack, and an interpreter can hold explicit frames in
GC-managed arrays exactly as it can in linear memory. WasmGC therefore does not
inherently forfeit constraint #2. The decision stands on different grounds:
Rust toolchain support, control over layout, ease of producing a canonical
byte-oriented image, and not depending on host GC behavior.

**E-2 — ADR-001, binary size.** The ~2MB figure is the *standard* Go toolchain's
wasm floor. TinyGo commonly produces much smaller binaries, so the size gap is
against one of Go's two toolchains, not Go.

**E-3 — ADR-001, Asyncify.** "TinyGo drags in Asyncify" is too categorical.
TinyGo uses Asyncify for goroutines on wasm, and its scheduler can be disabled.
The decision stands — Rust is the working toolchain and the legibility argument
is independent — but this was never the load-bearing reason.

**E-4 — ADR-001, line count.** "~30% more lines than Go" is a projection, not a
measurement. No comparable pair has been built.

**E-5 — ADR-006, operand width.** The entry claims monotonic slots avoid a wider
encoding. They can do the opposite: no reuse raises the maximum live slot index,
which can push operand fields wider. The decision stands, and the optional
last-use reuse pass is the fix if it ever bites.

**E-6 — ADR-012, transducers.** "Transducers are faster" is inherited folklore
here, not something measured on this implementation.

**E-7 — ADR-024, terminology.** "Hygiene" overstates what read-time resolution
plus gensym provides. Clojure macros are not hygienic in the `syntax-rules`
sense — a macro author can still construct a capturing symbol deliberately. Read
the entry as *Clojure-style capture avoidance*, which is what its rejection
clause already says.

**E-8 — ADR-010/ADR-025, `Value` size.** The predicted 24 bytes assumed
`Rc<str>` and `Rc<[T]>`, which are fat pointers. The implementation uses
`Rc<StrObj>` and `Rc<ListObj>` — thin pointers to sized structs — so `Value` is
**16 bytes**. The `<= 24` assertion stands and passes with room; the reasoning
in ADR-010's cost paragraph does not. Measured at milestone 1, `apolisp sizes`.

**E-9 — ADR-026, the shape of the carrier.** The sketch reads
`LocatedForm { root: Value, origin: SpanOrigin }`, one origin, alongside "every
aggregate holds one origin per syntactic child." Those cannot both be one
struct. The implemented carrier is a tree mirroring the value tree:

```rust
struct Origins { origin: SpanOrigin, children: Vec<Origins> }
struct LocatedForm { root: Value, origins: Origins }
```

A map contributes two children per pair, key then value. The decision is
unchanged — origins live outside the value graph, positionally, covering
immediates — but the single-field sketch was not implementable as written.
