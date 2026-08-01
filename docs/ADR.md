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

*Decision stands. The grapheme third of "separate explicit operations for bytes,
scalar values, and graphemes" is withdrawn by ADR-054; the byte and scalar
operations exist and are unaffected. The "no promise of O(1) character indexing"
clause is a declined guarantee and not a prohibition — see ADR-052, which had to
say so after ADR-049 read it the other way.*

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

*Read-time resolution superseded by ADR-040: with one namespace it resolves to
itself. The decision that hygiene is a property of symbol identity, and that
metadata carries spans and nothing else, stands.*

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

### ADR-030 — The line budget counts every line, and is an order-of-magnitude target

*(New, 2026-07-26. Amends the budget in `BUILD.md`. Resolves Q21.)*

**Decision.** Three parts:

1. **Everything counts.** Comments and blank lines are in the budget alongside
   code. The unit is lines of file, not lines of code.
2. **The number is an order of magnitude, not a threshold.** The core is a
   single-digit-thousands artifact. The working total is **~7,000 lines**, and
   ±1,000 is noise. Only the total is asserted; the per-layer rows are guidance
   and are reported rather than enforced.
3. **Budgets are hard, and amendable by ADR.** Going over is not a test to
   silence or a number to nudge — it is a decision, made deliberately, in a new
   entry that says what grew and why.

**Why.** Constraint #1 is a context-window constraint, so the thing being
measured is how much has to be held at once. A comment occupies the window like
anything else, and counting code alone would make the number stop measuring the
constraint it exists for.

But the 3,000–5,000 figure this project started from assumed minimal comments.
Counting comments against it without raising it was a silent ~20% cut that
nobody chose — milestone 1 measured 19% comments and blanks, and denser
subsystems will run higher. The revised total restores the code headroom the
layer table always meant, and states the counting rule out loud.

The order-of-magnitude framing is the part that matters most. Whether the core
is 5,000 lines or 7,000 does not change whether one engineer and one model can
hold it; whether it is 7,000 or 70,000 decides it. Precision past the nearest
thousand is false, and a budget that reads as exact invites arguing about 40
lines instead of noticing a subsystem that doubled.

**Cost.** With only the total asserted, a bloated layer can hide inside a lean
one. Accepted: per-layer assertions at 300-line granularity would be false
precision, and would generate ADR churn for rebalancing that carries no meaning.
Per-layer numbers are printed on every run, so the shape stays visible even
though it is not enforced.

**Rejected.** *Counting code only* — stops measuring the constraint. *Per-layer
hard assertions* — see cost. *Keeping the total at 5,300 with comments counted*
— a fifth of the budget removed by accident rather than by decision, and it
taxes exactly the why-comments that make the core holdable.

---

### ADR-031 — The library/driver split; "until it hurts" arrived at milestone 1

*(New, 2026-07-26. Advances ADR-015 one step along its own progression; does not
supersede it.)*

**Decision.** `src/lib.rs` holds the language — `error`, `value`, `reader`,
`printer`, and every module after them, still as inline `mod` blocks in one
file. `src/main.rs` is the process driver: arguments, file I/O, stdout, exit
codes, and nothing else. If a change to `main.rs` changes what a program means,
it is in the wrong file.

Tests call the library for properties and the binary for golden snapshots. The
binary path comes from `CARGO_BIN_EXE_apolisp`, never from a hand-built
`target/debug/...`.

**Why.** ADR-015 named the trigger as "a large file strains tooling" and it
turned out to be the wrong trigger. What actually hurt was the *test boundary*:
with no library target, `read(print(read(s))) == read(s)` could only be checked
as *printed strings being equal*, which is a projection of the property rather
than the property. Under that comparison `1e400` round-tripped from `Float` to
`Sym` and the test passed (ADR-032, and demonstrated by adding the case to the
old suite). The hand-written `Value::PartialEq` — a deliberate, load-bearing
implementation with its own `TRAPS.md` entry — was unreachable from any test at
all.

Two smaller things came with it: every property spawned a process and wrote a
temp file, and a hand-built binary path meant a run under a custom
`CARGO_TARGET_DIR` compiled the tests there and executed the *old* binary in the
repository, so the suite could pass against a stale artifact.

This is the second step of ADR-015's stated progression — one file → one crate
with file modules → library + binaries — reached earlier than expected and for a
reason that entry did not anticipate. The single-reading-view benefit is
retained: the library is still one file with inline `mod` blocks.

**Cost.** Two files instead of one, and a public API surface that did not exist
before. `pub` on a module is now a decision rather than a formality, and the
temptation is to widen it for testing convenience — which is how a test suite
starts pinning internals. The layer report reads per-file totals as well as
inline `pub mod` markers.

**Rejected.** *Staying on one file and keeping string comparison* — the property
would remain a proxy, and the enum whose equality is hand-written would remain
untested. *`#[cfg(test)]` unit tests inside `main.rs`* — works, but puts the
oracle inside the artifact it checks and does not fix the stale-binary problem.
*A workspace or multiple crates* — no deployment boundary asks for one.

---

### ADR-032 — Non-finite float literals are written, not computed

*(New, 2026-07-26. Does not touch Q13, which owns float **equality**.)*

**Decision.** Three tokens read as the floats they name: `##Inf`, `##-Inf`, and
`##NaN`, which are Clojure's spellings and exactly what the printer already
emits. A finite-looking literal that overflows the range — `1e400` — is a read
error naming `##Inf` as the way to say it on purpose.

Underflow is deliberately not symmetric: `1e-400` reads as zero, as it does
everywhere else. Losing precision near zero is the ordinary condition of
floating point; losing the value entirely is not.

**Why.** The printer emitted three tokens its own reader could not read. `##Inf`
came back as a *symbol* named `##Inf`, which printed as `##Inf` again — so the
data changed type while the text stayed identical, and any string-level
round-trip check agreed with itself. Closing the hole in the reader is what
makes the round-trip property able to see the difference at all.

Rejecting `1e400` follows the rule the reader already applies to an oversized
integer: a literal whose digits are gone is a silent wrong answer, and the
source said something the value no longer records. Ergonomics is last
(`ETHOS.md`), and `##Inf` is one token.

**Cost.** A divergence from Clojure, which reads `1e400` as `##Inf`. Anyone
porting code that relies on overflow-to-infinity gets an error instead. The
divergence is cheap to reverse — accepting more inputs later is safe — and
narrowing after the fact is not, which is the direction this errs in.

`##NaN` also puts a value in the reader that is not equal to itself under
`Value::PartialEq`. That is IEEE and is Q13's to settle; until then the
round-trip property compares floats by bit pattern, which is the right
comparison for "did this survive the trip" regardless of how Q13 lands, and
distinguishes `0.0` from `-0.0` as a bonus.

**Rejected.** *Rejecting non-finite literals entirely* — the printer would still
have no readable output for a float the VM can produce. *Accepting `1e400` as
infinity* — Clojure-compatible and silent. *Deferring to Q13* — Q13 owns
equality; a printer emitting unreadable tokens is a defect now.

---

### ADR-033 — Evaluation order, arity, and variadic `fn`

*(New, 2026-07-26. Resolves the part of Q20 that milestone 2 forces. Q20 stays
open for the rest.)*

*Decision stands; one claim corrected and one reason strengthened by erratum
E-11.*

**Decision.** The compact table Q20 asked for, scoped to the three edges a
compiler cannot avoid answering:

| Edge | v1 | Clojure |
|---|---|---|
| Argument evaluation | strict left to right, operator first | same |
| `let` binding order | sequential, left to right | same |
| Collection literal order | left to right; in maps, key before value | same |
| Under-supply | throws — no implicit `nil` filling | same |
| Over-supply | throws — no silent discard | same |
| Where arity is checked | callee prologue, at call time | runtime |
| Rest marker | `&` | same |
| Rest with no extra args | **empty list** | **`nil`** — deliberate deviation |
| Multiple arity bodies per `fn` | **not in v1** | supported |

Stated as rules:

1. **Evaluation is strict left to right everywhere**, and the operator position
   is evaluated before the arguments.
2. **Arity is checked once, at runtime, in the callee's prologue** — never at
   the call site, never at compile time. Under- and over-supply both throw.
3. **`fn` takes one parameter list**, optionally ending in `& rest`. `rest`
   binds to a list, empty when nothing extra was supplied — never `nil`.

**Why.**

*Left to right* is what a slot compiler emits anyway: argument *i* evaluates
into slot base+i under ADR-006's monotonic allocation, so the decision ratifies
the obvious implementation rather than constraining it. Writing it down is what
makes a later reordering "optimization" visible as the semantic change it is —
`throw` is a core form (ADR-007) and cells are mutable (ADR-020), so which of
two side-effecting arguments runs first is observable, not a compiler liberty.

*Arity in the callee* falls out of ADR-027: globals are rebindable `CellId`
entries, so a global's arity at compile time is not its arity at call time. A
call-site check would need invalidation machinery on every rebind, or an
assumption that is quietly false. One check, in one place, on entry — and the
prologue is already the path tail calls pass through (ADR-028).

*Empty list rather than `nil`* is the one deviation here, taken on the
correctness clause of constraint #4. Clojure's nil rest argument forces every
variadic body to handle two types for one parameter, and an empty list is truthy
while `nil` is not — so the two spellings of "no extra arguments" take different
branches. That is a `TRAPS.md` entry we would be creating deliberately. There is
no code to port and no users to break, and widening to accept `nil` later is
safe.

*One parameter list* keeps the prologue to a check rather than a switch.
Multi-arity dispatch is a second mechanism for a convenience a macro can supply
later over a single variadic `fn`; ergonomics is last (`ETHOS.md`).

**Cost.** Fixed evaluation order forecloses argument reordering — which ADR-021
already makes a semantic change requiring an entry, so the cost is naming it.
The arity check is one comparison per call that a compile-time check could
sometimes elide; measure before caring. No multi-arity means Clojure code using
it needs a rewrite or the deferred macro, and the rest-argument deviation means
ported code testing `(nil? more)` reads differently. Both are the kind of break
the freedom clause exists for.

**Rejected.** *Unspecified evaluation order* — standard for C-family compilers
and wrong here: with `throw` in the core the order is observable, and
"unspecified" in practice means the first implementation decides it silently,
which is the failure Q20 exists to prevent. *Compile-time arity checking* —
unsound against ADR-027's rebindable globals without invalidation machinery.
*Implicit `nil` for missing arguments* — turns a caught error into a wrong
answer. *Clojure's `nil` rest argument* — see why. *Multi-arity bodies* —
deferred, not refused.

**Open.** Maximum arity is a bytecode-encoding question, not a semantic one, and
belongs with the instruction format in milestone 2 rather than here.

---

### ADR-034 — The instruction format: a typed enum, and the spellings the core was missing

*(New, 2026-07-26. Answers ADR-033's Open clause. Reads ADR-006 through E-5.
Refines ADR-007's core-form list with two spellings it never fixed.)*

**Decision.** Five parts.

**1. An instruction is a Rust enum with named, typed fields.** No bit packing, no
operand-width question.

```rust
pub enum Instr {
    Const      { dst: Slot, k: ConstIdx },
    Move       { dst: Slot, src: Slot },
    GetGlobal  { dst: Slot, name: SymId },
    SetGlobal  { name: SymId, src: Slot },
    GetCapture { dst: Slot, idx: CaptureIdx },
    GetSelf    { dst: Slot },
    SetCell    { cell: Slot, src: Slot },
    Closure    { dst: Slot, proto: ProtoIdx },
    Call       { dst: Slot, base: Slot, argc: u32 },
    TailCall   { base: Slot, argc: u32 },
    Return     { src: Slot },
    Jump       { target: Pc },
    JumpUnless { cond: Slot, target: Pc },
    Throw      { src: Slot },
    PushHandler { catch: Pc, err: Slot },
    PushFinally { target: Pc },
    PopHandler,
    EndFinally,
}
```

`Slot`, `ConstIdx`, `Pc`, `ProtoIdx`, and `CaptureIdx` are all `u32`.
`size_of::<Instr>() <= 16` is asserted the way `size_of::<Value>()` is
(ADR-025) — the number is measured, not assumed.

A call's callee sits at `base` and its arguments at `base+1 ..= base+argc`, which
is where left-to-right evaluation into monotonically allocated slots puts them
anyway (ADR-033). The callee's frame receives the arguments in its own slots
`0..argc`, so parameters are the first slots of a frame by construction.

**2. There is no maximum arity.** Arity is bounded by the slot space and, long
before that, by memory. This is the answer to ADR-033's Open clause, not a
deferral of it: the encoding imposes no limit, so there is no number to state.

**3. A global is referenced by its interned `SymId`.** The name→cell resolution
is the VM's, at run time. ADR-027's rule that globals are VM-owned `CellId`
entries is unchanged; what this fixes is that the *operand* is the name.

**4. The unit of compilation is a `Chunk`: a flat `Vec<Proto>`, with `protos[0]`
the top level.** A `Proto` carries `code`, a `lines` array parallel to it
(ADR-023 point 2), a constant pool, capture descriptors, the parameter count and
variadic flag, and the slot count. Nested `fn`s are separate protos referenced by
index, reserved in source order so the disassembly is stable. A file's top-level
proto takes no parameters, evaluates each top-level form in order, and returns
the last — `nil` for an empty file.

**5. The two core forms that had no spelling get one, and `try` gets its shape.**

- ADR-027's top-level create-or-rebind operation is **`set-global!`**:
  `(set-global! name expr)`, `name` unevaluated. `def` and `defmacro` are the
  library macros over it, as that entry says.
- **`fn` takes an optional name** — `(fn name? [params] body*)` — bound only
  inside the body, to the running closure's own identity, compiled as `GetSelf`.
  This is ADR-002's self-recursion exception made real: the name resolves through
  identity, never through a capture.
- **`try` is `(try body* (catch sym body*)? (finally body*)?)`.** The catch
  clause binds the thrown value to one symbol. There is no class filter and no
  predicate, because Q23 has not decided what a thrown value *is*.

`let`, `fn`, and the `try` clauses take implicit `do` bodies, as in Clojure.

**Why.**

*The enum, because of E-5.* ADR-006 claims monotonic slots avoid a wider
encoding; E-5 corrects it — no reuse raises the maximum live slot index, so a
packed format's operand fields are exactly what comes under pressure. The two
standard answers to that are 8-bit fields with an `EXTRAARG` escape, or varint
operands, and both buy density with encode/decode complexity in three places at
once: the compiler, the VM loop, and the disassembler. Not packing dissolves the
problem instead of managing it. With `u32` fields there is no width to exhaust,
E-5's correction stops having a consequence, and ADR-006's optional last-use
reuse pass goes back to being a pure optimization rather than a latent
requirement.

It also ranks correctly under `ETHOS.md`'s own order. *Line count:*
`Call { dst, base, argc }` reads the same in the compiler, the VM, and a
debugger, and the disassembler is a `match`, not a decoder. *Serializable state:*
a typed enum derives its encoder; a packed word needs the layout documented as
a wire format. *Speed:* dispatch is a match on a discriminant, which lowers to
the jump table a byte opcode would get. What is actually given up is code
density and therefore instruction-cache behaviour — a constant, unmeasured, on a
project whose own prior art says architecture is the lever and everything else
is a rounding error (`PRIOR-ART.md`, wallisp). If it ever matters, it is a
pre-registered experiment, not a design assumption.

*No maximum arity* is what the unpacked encoding makes available, and it is
worth taking. Every fixed limit here would be arbitrary — 20 is Clojure's and it
is a JVM artifact, 255 is a byte and this format has no bytes — and an arbitrary
limit is discovered by the generated call that trips it, at the worst moment.
Macro-expanded code is exactly where a 300-argument call comes from.

*Globals by name.* Three reasons, in order. Forward references are free: a call
to a global defined later in the file compiles with no fixup and no patch list,
which matters because the top level is a sequence. The disassembly prints the
name rather than a table index whose value depends on definition order — that is
a determinism property for the goldens, not a nicety. And the compiler does not
need a live VM to resolve a name, which keeps milestone 2 from having to build
half of milestone 3; compilation acquires a VM dependency at milestone 5, when
macros make it unavoidable (ADR-004), and not before.

*Duplicated `finally`.* `(try B (catch e H) (finally F))` lowers to two nested
handler regions, and `F` is emitted twice — once inline on the normal path, once
as the unwinding path the VM enters. Nesting the catch region inside the finally
region is what makes a throw *from the catch body* still run the cleanup, and
composition falls out: a `try` with only one clause emits only that region. The
duplication buys the absence of a runtime mechanism — no return-address slot, no
jsr/ret — and "exactly once" (ADR-028 invariant 1) is readable off the emitted
code rather than argued about a state machine. It is also cheap here because the
core has no `return`: the only nonlocal exit is `throw`, so there are two exit
paths, not three.

**Cost.** The code array is 16 bytes an instruction rather than 4, and an `Image`
is correspondingly larger. `finally` duplicates 2^depth under nesting;
de-duplicating it is a compiler-local change under ADR-021 if a real program ever
nests deeply enough to care. Reading a global costs a name→cell lookup per
access that a `CellId` operand would have paid once at compile time — that is
where an inline cache goes, and ADR-007's rejection of an extensible special-form
registry was partly to keep that option open. `set-global!` is a name nobody will
type, which is the intent.

**Rejected.** *Packed `u32` words, Lua-style* — 8-bit slot fields against
monotonic allocation with no reuse is E-5 as a compile error; the escape hatch
is the complexity ADR-006 declined. *A varint byte stream* — densest and
unbounded, but `lines[i]` stops being an array index into the code, which is the
one thing ADR-023 point 2 asks for. *A `CellId` operand for globals* — needs a
live VM at compile time and a fixup list for forward references, and makes the
disassembly depend on definition order. *jsr/ret for `finally`* — puts a machine
address in a slot, therefore in a language value, therefore in the snapshot and
the debugger, to avoid duplicating code we can duplicate. *A stated maximum
arity* — see why.

**Open.** Multiple catch clauses, and any dispatch on the thrown value, wait on
Q23. Whether `lines` stays one `SpanOrigin` per instruction or compresses to a
range table is a size question for milestone 8 and changes nothing semantic.

---

### ADR-035 — A collection literal in code position is a call

*(New, 2026-07-26. Answers the part of Q20 milestone 2 forces. Q20 stays open
for the rest.)*

**Decision.** `[a b]` in code position compiles as `(vector a b)`, and `{k v}`
as `(hash-map k v)`. `()` evaluates to itself, as in Clojure; `[]` and `{}` are
zero-argument calls like any other empty literal.

The constructor is resolved as a **global, bypassing lexical scope**: `[x]` is a
vector literal even where a local named `vector` is in scope.

**Why.** ADR-033 already decided that collection literals evaluate their elements,
left to right, key before value. ADR-007 already decided the core is closed at 13
forms. Those two together leave exactly one option that does not break either.

A `Vector(Vec<Expr>)` node in the core AST would be a fourteenth core form — the
compiler would no longer be authoritative about a closed list, which is the whole
of ADR-007's "read the compiler, know the language." Lowering to a call keeps the
list at 13 and gets ADR-033's evaluation order for free, because it is then
ordinary argument evaluation and there is no second rule to keep in agreement
with the first.

Clojure emits direct construction rather than a call here. It can afford to:
its special-form set is not closed by decision, and its compiler already knows
about persistent collection types. Ours does not, and milestone 6 is where
collections get a representation at all (Q6).

**Cost.** Rebinding the global `vector` or `hash-map` changes what a literal
means, everywhere, silently. That is a real wart and it is the price of the
closed core. It is also self-inflicted by construction — one user, no
compatibility contract — and the alternative is a permanent fourteenth form.
Second cost: a literal of constants is built at run time rather than folded into
the constant pool. Folding it later is a compiler-local optimization that changes
no semantics (ADR-021), and doing it now would mean a literal's meaning depended
on whether its elements happened to be constant.

**Rejected.** *A `MakeVec`/`MakeMap` instruction pair* — the instructions are
free, but the core AST node they need is not; see why. *Constant-folding literals
whose elements are all constants* — a special case that makes the same syntax
mean two different things, for a saving nothing has asked for. *Resolving the
constructor through lexical scope* — makes `[x]` mean something different inside
a `let` that happens to bind `vector`, which is worse than the global wart in
every way.

**Open.** Which collection literals exist at all — sets, and whether map literals
survive contact with Q6's representation — is still Q20's.

---

### ADR-036 — Nesting depth is bounded, and the reader is the only place that checks

*(New, 2026-07-26. From the milestone-2 review. Related to ADR-004, which bounds
the *interpreter*; this bounds the phases in front of it.)*

**Decision.** A form may nest at most **256** levels deep. The reader enforces
it, once, and returns an ordinary `LispErr` with a position. No other phase
checks.

**Why.** ADR-004 keeps the Rust stack empty at every VM instruction boundary.
Nothing had said anything about the phases *before* the VM, and all of them
recurse on the host stack: the reader, the origin walkers, `Rc`'s drop glue over
a deeply nested list, the resolver, and the lowering. Milestone 2's compiler
aborted with SIGABRT at about 1,400 levels of nesting on input the reader
accepted at 3,000 — a stack overflow presenting as a killed process rather than
as a diagnostic, which is the failure `TRAPS.md` describes for the VM and which
had quietly arrived in the compiler.

The reader is the only place that needs the check because every later phase
recurses at most once per level of form nesting, so a bound on form depth bounds
all of them. Checking again downstream would be the duplicated enforcement that
milestone 2's mutation pass already found once
(`notes/milestone-2-mutants.md`) — two mechanisms for one rule, and no way for a
test to say which is working.

256 is chosen to be safe on the smallest stack this code runs on, which is a
2 MB cargo test thread rather than the 8 MB main thread — the limit that
matters is the tightest one, not the typical one. It is also far past anything
hand-written; `../reg-lisp` capped nesting for the same reason after its
conversion had to become iterative on both axes (`PRIOR-ART.md`).

**Cost.** A generated program that nests past 256 is rejected rather than
compiled. Macro expansion is where that could plausibly happen, and macro output
does not pass through the reader — so when milestone 5 lands, the expander owns
this check for the forms it produces, and that is the one place the rule is
allowed to be enforced twice, because it is genuinely two entry points rather
than two mechanisms on one path. Raising the number later is a new entry, not an
edit.

**Rejected.** *Making the resolver and lowering iterative* — an explicit worklist
in both phases costs far more legibility than the bound does expressiveness, and
constraint #1 is the binding constraint. *A larger stack for the process* —
moves the number without removing the failure mode, and does nothing for the test
threads. *Catching the overflow* — a Rust stack overflow is an abort, not a
recoverable condition, and ADR-028 rule 5 already says host-level failures never
become language unwinding.

---

### ADR-037 — Integer overflow throws

*(New, 2026-07-26. Resolves Q10.)*

**Decision.** Integer arithmetic is checked. Overflow raises a language error
rather than wrapping or saturating. No automatic promotion to a wider type and no
bignums.

**Why.** Constraint #4 orders correctness above efficiency, and wrapping turns a
caught error into a wrong answer — the same objection ADR-033 used to reject
implicit `nil` for missing arguments. Simulators are one of the three workloads
this substrate exists for (`ETHOS.md`), and a counter that silently wraps is the
failure mode they cannot detect.

It is also the only option that makes the two build profiles agree by
construction. Rust panics on overflow in debug and wraps in release, so an
unchecked implementation is two different languages depending on how it was
built; `just test-release` exists in the gate because of exactly this, and
`TRAPS.md` has carried the entry since before there was code. Checked arithmetic
removes the divergence rather than testing for it.

Clojure agrees on the spelling that matters: `+` throws on long overflow and
`+'` is the promoting variant. Ported code that overflows will fail loudly in
both languages.

**Cost.** One predictable, well-branch-predicted comparison per arithmetic
operation. `../wallisp` measured architecture as the lever and everything else as
a rounding error (`PRIOR-ART.md`), so this is not where the time goes; if it ever
shows up, it is a pre-registered experiment. No bignums means a program that
needs them has to say so, and nothing in v1 offers a promoting variant — adding
`+'` later is a library question, not a language one.

**Rejected.** *Wrapping* — a silent wrong answer, and the failure mode with no
diagnostic. *Saturating* — invents a value the program never computed and breaks
the algebra quietly, since `(+ x 1)` can equal `x`. *Automatic promotion to
bignum* — a second numeric representation, a second equality rule, and Q13 is not
even settled for the two that exist.

---

### ADR-038 — The call protocol: native functions, primitives as globals, and how a frame is sized

*(New, 2026-07-26. Resolves Q24 and Q25. Completes ADR-034's calling convention
on the run-time side.)*

**Decision.** Four parts.

**1. A native function is a kind of closure, reached through the ordinary
`Call`.** `Closure` stops being a placeholder and becomes:

```rust
pub enum Closure {
    Fn { proto: ProtoIdx, captures: Rc<[Value]> },
    Native(NativeId),
}
```

`Value` is untouched — still `Fn(Rc<Closure>)`, still 16 bytes — so ADR-025
stands unamended. No new instruction: `HostCall` remains unbuilt and arrives with
the handle table at milestone 7, where ADR-013 puts it.

**2. Primitives are ordinary entries in the global table.** `+` is a `Value` you
can pass to `map`, and ADR-027's rebind operation works on it like any other
global.

**3. The VM sizes a frame `max(proto.slots, argc)`, and a variadic prologue packs
in place.** The prologue collects slots `params-1 ..< argc` into a list and
writes it back to slot `params-1`. Packing in place is safe because those slots
are dead the moment they are collected: the prologue runs before the function's
first instruction, so nothing else can observe them.

**4. Until Q23, a VM-raised failure is not a language value.** Arity mismatch,
overflow, an unbound global, and calling a non-function produce a host-side fault
carrying a message and an origin, which ends the run with a diagnostic. `throw`
carries a language `Value`, as ADR-007 requires. With no handler stack until
milestone 4, both simply end the run, so nothing yet depends on the two being the
same shape — which is the point.

**Why.**

*Native functions inside `Closure`* is the option that changes the least. A
`Value::Native` variant would supersede ADR-025 over a distinction the call path
does not need, and would widen every match in the printer, in equality, and
eventually in the serializer. Dedicated `ADD`/`SUB`/`LT` instructions would be
faster and would stop `+` from being a value — `(map + xs)` would have nothing to
pass — which makes arithmetic a special case rather than a library. That option
stays reachable later as a pure optimization over this one, which is the right
order under ADR-021.

*Primitives as globals* follows from ADR-027 having exactly one namespace and one
rebind operation. The wart is real and already familiar: `(set-global! + ...)`
breaks arithmetic, exactly as ADR-035's `(set-global! vector ...)` breaks vector
literals. `../let-rs` made the same choice and `PRIOR-ART.md` flags it as the
thing an inline cache has to invalidate against — worth knowing now, not worth
avoiding now.

*`max(slots, argc)`* is forced. ADR-034 removed any maximum arity, so the
compiler cannot record how large a variadic frame might need to be; recording
"unbounded" would be the VM sizing the frame anyway, with a field in `Proto` that
never says anything.

*Faults are not values yet* keeps Q23 genuinely open. The alternative — inventing
a shape for VM errors now, most likely a string — would answer Q23 by accident in
the exact way rule 3 exists to prevent, and milestone 4 would inherit it as a
fait accompli rather than a decision.

**Cost.** Two call paths in the dispatch loop instead of one, though they share
the argument window. A native function has no `Proto`, so it has no `lines` and
contributes no position to a backtrace beyond its name. Rebinding a primitive
global breaks the language, silently, with no diagnostic. And VM faults are not
catchable at all until Q23 is answered — `try` cannot handle an overflow yet,
only an explicit `throw`.

**Rejected.** *`Value::Native`* — supersedes ADR-025 for no gain at the call
site. *Dedicated arithmetic instructions* — see why; available later. *A
`HostCall` opcode now* — ADR-013 scopes it to host capability, and arithmetic is
not on that entry's own list. *A max-arity field in `Proto`* — see why. *Making
VM faults language values now* — answers Q23 sideways.

**Open.** Whether host faults and language `throw` produce the same shape is
still Q23, and it is what milestone 4 has to settle before a `.out` transcript
can pin a failure.

---

### ADR-039 — What an error is: a fault is a throw, and a thrown value is any value

*(New, 2026-07-27. Resolves Q23. Supersedes ADR-038 clause 4, which held the
question open on purpose. Answers ADR-034's Open clause on catch dispatch.)*

**Decision.** Five parts.

**1. A thrown value is any `Value`.** `throw` imposes no shape and validates
nothing: `(throw 42)` and `(throw {:type :app-error})` are both legal. The
language half of Q23 is that there is no language half — an error is a value,
and which values are errors is a convention programs adopt.

**2. A VM-raised fault is a throw.** An arity mismatch, an overflow, an unbound
global, a call to a non-function: each raises the same unwinding as `throw`, so
each runs pending `finally` blocks and can be bound by a `catch`. At run time
there is exactly one failure path, and `vm::run` loses its `Err` channel to say
so.

**3. A fault's value is a map of three keys.**

```clojure
{:type :vm-error :kind :unbound :message "`ready?` is not bound"}
```

`:kind` is a closed vocabulary *within* a `:type`. For `:vm-error` in v1 it is
`:arity`, `:unbound`, `:not-callable`, `:type`, `:overflow`, `:undecided`, and
`:internal`. The vocabulary grows in the entry that adds the subsystem raising
the kinds, not by whoever writes the message: milestone 7's host errors arrive
as `:type :io-error` with ADR-013's capability list supplying the kinds and
extra keys (`:operation`, `:path`) beside them, which is the shape the original
design conversation proposed.

`:message` is prose for a human. `:kind` is the contract — a program that
matches on the message text is matching on something this project reserves the
right to reword.

**4. Position and suppression travel beside the value, not inside it.** The
in-flight unwind carries three things: the value, the `SpanOrigin` of the
instruction that raised it, and a `suppressed` list. A `catch` binds the value
alone. This is ADR-026's rule for origins applied to errors — the same reason,
that a position is about *where a value came from* rather than about the value.

That makes ADR-028 invariant 3 expressible for any thrown value, `(throw 42)`
included: when a cleanup throws while unwinding, the cleanup's error wins and
the parked original moves onto its `suppressed` list. A `.out` transcript prints
both, under `--- at` and `--- suppressed`.

**5. One `catch` clause, no filter, still.** ADR-034 left multiple clauses and
any dispatch on the thrown value waiting on this entry. The answer is that
dispatch is a library concern, not a language feature: once maps are readable
(milestone 6) a handler body does its own `:kind` test, and `catch` stays the
one-symbol binding ADR-034 specified.

**Why.**

*Faults converge with `throw` because the alternative is two unwinding paths
that differ only in the last step.* Both have to drop frames, both have to run
every pending `finally` exactly once, and both end the run when nothing catches
them; keeping them apart buys one thing — `catch` cannot see a fault — at the
price of writing that machinery twice and keeping the two copies in agreement.
The concrete case that decides it is milestone 7's `with-open`, which ADR-028
promised as `try`/`finally` lowering: if an arity error inside the body skips
cleanup, a handle leaks on exactly the failures nobody predicted, and the
promise is worth less than the words in it.

*The taxonomy is closed per `:type` because a vocabulary nobody can enumerate is
not a vocabulary.* A program that wants to retry on `:not-found` and give up on
`:permission-denied` needs the set to be finite and stable; an open set of
keywords invented at the raise site is a formatted string wearing a colon.
Closing it per `:type` rather than globally is what lets milestone 7 add its
kinds without superseding this entry.

*Suppression lives on the unwind record because a thrown value can be an
integer.* Attaching the original to the winner works only when the winner is a
map, and `(throw 42)` from inside a cleanup is precisely the case invariant 3
exists for. The alternative — a metadata channel on `Value` — is a second
decision, taken to serve one field, on a `Value` that ADR-025 froze at 16 bytes.

**Cost.** A bare `(catch e ...)` now swallows a typo: an unbound global inside a
protected body becomes a caught value instead of a diagnostic. That is the
Clojure behaviour for `(catch Exception e ...)` and it is a `TRAPS.md` entry
rather than a mechanism, because the alternative is a filter language nothing
else in v1 needs.

A program cannot read the suppressed original in v1 — it reaches the transcript
and nothing else. Every fault allocates a three-entry map and interns nothing
(the keywords are interned once, at VM construction). And the fault messages are
now language-visible strings, which is a surface this project has to be
deliberate about rewording even though `:kind` is the part that is promised.

**Rejected.** *Faults stay uncatchable, unwinding but never caught* — the two
paths, and no program can recover from an overflow it can predict. *Faults stay
terminal, skipping cleanup* — breaks ADR-028 invariant 1 the first time a fault
happens inside a `try`. *A fault is a string* — the smallest change, and it
declines the half of Q23 worth answering: no program can dispatch on what went
wrong. *A flat `{:error :unbound :message ...}`* — one keyword space for every
subsystem, so milestone 7 either overloads it or supersedes this. *A
`Value::Error` variant or an exception type* — supersedes ADR-025 for something
a map already prints, compares, and serializes. *Suppressed assoc'd onto the
winning value* — see why.

**Open.** Whether a program can ever read the suppressed chain (it needs either
value metadata or a primitive that reaches into the unwind) waits for a real
use. Whether `:message` should be a structure formatted at print time rather
than a string is the same question one level down, and waits for the same
evidence.

---

### ADR-040 — Macros: one form the expander knows, a prelude, and hygiene by gensym

*(New, 2026-07-27. Supersedes ADR-024's read-time resolution clause; its
unbundling of hygiene from metadata stands. Completes ADR-027's promise that
`def` and `defmacro` are library macros. Second entry point for ADR-036.)*

**Decision.** Five parts.

**1. The expander knows exactly one new form: `(set-macro! name expr)`.** It
expands `expr`, compiles it, runs it, and keeps the resulting closure as a macro
for the rest of the compilation unit. The form itself expands to `(quote name)`
— it has already had its whole effect, and the top level stays a sequence of
expressions. `def` and `defmacro` are then written *in the language*, in
`src/prelude.xs`, which is compiled into the binary and expanded ahead of every
unit.

**2. Macros live in the expander, not the `Vm`.** A macro is a property of a
compilation unit. Nothing about it reaches an `Image` (ADR-029), and two units
compiled by one VM cannot see each other's macros. A macro is a closure *plus
the chunk it was compiled in*, because a closure names its proto by index
(ADR-034) and means nothing without it.

**3. Quasiquote is lowered by the expander, into ordinary calls.** The reader
desugars the punctuation only — `` `x `` reads as `(quasiquote x)`, exactly as
`'x` already read as `(quote x)` — and the expander turns a template into
`list`, `concat`, `vector`, `vec`, and `hash-map` calls, all ordinary globals
(ADR-038). Only symbols are quoted; everything else evaluates to itself. Three
new primitives fall out: `concat`, `vec`, and `gensym`.

**4. Hygiene is auto-gensym plus `gensym`, and syntax-quote does not qualify.**
`x#` inside a template becomes one fresh symbol per template, so two occurrences
in a template are the same name and two templates never collide. `(gensym)`
covers the case where a macro computes a name. Counters are per compilation
unit.

**5. Macro output carries the call site; what it passed through keeps its own
position.** A node the expander can still identify — by object identity, at any
depth, against the arguments it handed the macro — keeps the `Source` origin it
was read at. Everything else is `Generated(call site)`. Nothing becomes
`Unknown` here, which refines ADR-026: a macro call always *has* a call site,
and reporting it beats reporting nothing.

**Why.**

*One form rather than a built-in `defmacro`.* ADR-027 said `def` and `defmacro`
are library macros, and without metadata to mark macro-ness (ADR-024 gave that
up deliberately) something has to install one. Making the primitive
`set-macro!` rather than `defmacro` keeps the built-in at its smallest — bind a
name to a function — and puts the part with syntax, destructuring, and a
template in the language, where it can be read and changed without touching
Rust. That the prelude is *itself* a macro definition is the exit condition
being met rather than described.

*The lowering is in the expander because the reader has no business knowing
about gensym.* ADR-024 put resolution at read time for hygiene reasons that part
4 removes; with those gone, what is left is a template-to-calls rewrite, and
doing it in the expander keeps `.forms` showing what was written and
`.expanded` showing what it became. Each golden then shows one phase's job.

*Qualification buys two things, and one of them is vacuous here.* Clojure's
syntax-quote qualifies symbols so that a macro's references resolve where the
macro was *defined* and so that a template cannot bind a name the caller chose —
a qualified symbol is not a legal binding form. With ADR-027's single namespace
the first is meaningless: there is nowhere else for a name to resolve to.
The second is real, and the price of it is three couplings — the reader would
have to know the thirteen core form names to leave them unqualified, the
resolver would have to reject qualified binding names, and global lookup would
have to canonicalize `ns/foo` to `foo` for as long as there is one namespace.
That is a large mechanism whose only surviving job is to make one class of
mistake impossible, and `x#` makes the same mistake easy to avoid. Qualification
is what Q12 buys when a second namespace exists, and this entry is what it would
supersede.

Auto-gensym is per *template*, not per expansion, which is also Clojure's
behaviour and for the same reason: the template is lowered once, when the macro
is defined. Two expansions of one macro therefore share a generated name. That
is safe — the name cannot collide with anything the caller wrote, and a macro
nested inside itself shadows in the ordinary way.

**Cost.** A template can still capture: `` `(let [x 1] ~body) `` binds the
caller's `x`, silently, where Clojure would refuse to compile it. That is a
`TRAPS.md` entry rather than a mechanism, and it is the one thing this entry
gives up against ADR-024 as written.

A macro body sees primitives and macros, and nothing else the unit defines: the
expander does not evaluate top-level forms as it goes, so a function defined
with `def` earlier in the file does not exist when a macro runs. Clojure's
model, where the top level is compiled and evaluated form by form, is what
removes that limit; whether to adopt it is **Q28**.

Nested quasiquote is refused rather than half-supported, and so is `~@` inside a
map template — splicing pairs needs `apply`, which v1 does not have. Both are
diagnostics with a position, not silent wrong answers.

The prelude is a new kind of file: language code inside the compiler's binary.
It counts against the ADR-030 budget, and it is the easiest place in a project
like this for a standard library to start growing by accident.

**Rejected.** *A built-in `defmacro`* — smallest, and it makes the milestone's
exit condition something the host does rather than the language. *Macro-ness as
a `Closure::Macro` variant, macros in the global table* — one table instead of
two, at the price of superseding ADR-038 and widening every match over
`Closure`; and it would put macros in an `Image`, which is where they least
belong. *Lowering quasiquote in the reader (Clojure's placement)* — makes the
reader namespace- and gensym-aware, which ADR-024's own cost paragraph called a
real coupling, for no benefit once qualification is gone. *Read-time
qualification* — see why.

**Open.** Q28 (does the expander evaluate the top level as it goes) and Q12 (a
second namespace, which is when qualification stops being vacuous). Whether
`loop`/`recur` is expressible as a macro over the core is still Q5, now
answerable: the machinery it was waiting on exists.

---

### ADR-041 — Collections, equality, and the numeric tower

*Decision stands; the performance claim in part 1's rationale is corrected by
erratum E-13 — `Rc::make_mut` does not yet mutate in place, and measurement is
how that was found.*

*(New, 2026-07-27. Resolves Q6, Q13, and Q26, and the collection half of Q20.
Names the representation ADR-011 left open. Narrows ADR-032. Adds one `:kind`
to ADR-039's vocabulary.)*

**Decision.** Six parts.

**1. Every collection is a flat `Vec`, and mutation happens in place when the
value is not shared.** A list and a vector are both `Vec<Value>`; a map is a
`Vec<(Value, Value)>` of pairs in insertion order. `conj` and `assoc` take the
buffer by `Rc::make_mut`, which mutates when the refcount is one and copies
otherwise. There are **no transients**, because the case they exist for —
building a fresh collection in a loop — is the case where the refcount is
already one.

There are **no sets** in v1, and still no character type. Both would be
`Value` variants, and ADR-025 froze that enum.

**2. `=` is structural, crosses list and vector, and does not cross `Int` and
`Float`.** `(= '(1 2) [1 2])` is true; `(= 1 1.0)` is false. Maps compare
without regard to insertion order. `##NaN` is equal to nothing including
itself, and `-0.0` equals `0.0` — both are IEEE's answers, reached by comparing
the numbers rather than their bit patterns. Functions, cells, handles, and
buffers compare by identity. `==` is numeric equality and *does* cross
`Int`/`Float`.

**3. Arithmetic coerces; integers and floats fail differently.** Any float
among the operands makes the result a float. Integer overflow throws (ADR-037,
unchanged); float overflow is `##Inf`, as IEEE says. `/` always produces a
float, because there are no ratios and truncating integer division under a `/`
spelling is the kind of silent wrong answer ADR-037 rejected. `quot` and `rem`
are integer division, and dividing by integer zero throws `:divide-by-zero` —
a new `:kind` in ADR-039's `:vm-error` vocabulary.

**4. `nil` punning, on the operations that read.** `(count nil)` is 0,
`(first nil)` is `nil`, `(rest nil)` is `()`, `(get nil k)` is `nil`,
`(empty? nil)` is true. The operations that *build* treat `nil` as the empty
thing of their own kind: `(conj nil x)` is `(x)` and `(assoc nil k v)` is a
map. Duplicate keys resolve last-wins at construction, so a map never holds
two equal keys.

**5. Strings are not sequences, and this is the surface that makes ADR-018
real:** `str`, `str-len` (bytes), `str-slice` (byte indices, and an error
rather than a panic on a non-boundary), `str-scalars` and `scalars-str` (Unicode
scalar values as integers, since there is no character type), `str-bytes` and
`bytes-str`, `bytes-len`, `bytes-slice`. `count` refuses a string, and says to
name the unit.

**6. Higher-order functions are written in the language, never as
primitives.** A native that called a language closure would re-enter the
dispatch loop on the Rust stack, and ADR-004 requires that stack to be empty at
every instruction boundary — a suspended computation inside a native's callback
is not representable in an `Image`. So `map`, `filter`, and `reduce` are
ordinary in-language definitions written where they are used.

**Why.**

*Flat `Vec` with copy-on-write* is the answer Q6 was actually asking for. The
question named the failure mode precisely: with reduce-into-a-collection as the
default idiom (ADR-012), copy-on-`assoc` makes the commonest operation in the
language O(n²). `Rc::make_mut` removes exactly that case — a fresh accumulator
has one reference, so the loop mutates a buffer it already owns — without a
transient API, a second representation, or a second set of rules about when a
value can be aliased. What remains O(n) is `get`/`assoc` against a *large* map
and `conj` onto a *shared* vector, and both are measurable rather than
arguable: ADR-021 removes the gate on optimizing them, and a pre-registered
benchmark is what should decide HAMT and RRB, not the fact that Clojure has
them.

*`=` follows Clojure's split because the alternative decides hashing too.* If
`1` equals `1.0` then equal values must hash equal, and every later choice about
a hashed map inherits a constraint made now, for a convenience. Type-strict `=`
plus `==` costs one extra function and keeps the assoc-vec map — which hashes
nothing — genuinely free of the question. Crossing list and vector is the same
call in the other direction: those are one abstraction with two representations,
and Clojure agrees.

*Arithmetic coerces because the alternative is a language whose literals do not
compose.* Q26 kept `(+ 1 2.5)` a fault so the tower would not be settled in a
match arm; this entry settles it deliberately. Integers throwing while floats
saturate to infinity looks inconsistent and is not: ADR-037's argument was that
a wrapped integer is a *wrong answer with no diagnostic*, and `##Inf` is neither
wrong nor silent — it is IEEE's own out-of-range value, it propagates, and it
prints.

**Narrowing ADR-032.** That entry made `##Inf` a value the reader accepts and
the printer emits, on the grounds that it is written rather than computed. Part
3 makes it computable, so the rule narrows to what it was really about: the
*reader and printer* must agree on a spelling for every float a program can
hold, including the ones arithmetic produces. The round-trip property is
unaffected and gets a wider input set.

**Cost.** A large map is a linear scan, and nothing warns you when it gets
large. `Rc::make_mut` makes performance depend on aliasing, so the same code is
fast or slow depending on whether something else is holding the collection —
the honest version of the transient story, but a real cliff with no diagnostic.
`(= 1 1.0)` being false will surprise, and it is the surprise Clojure ships. No
sets means `contains?` over a vector is the workaround, and it is O(n).

Part 6 has a sharper cost than it looks: with no way to call a closure from a
primitive, and no way to share compiled code between chunks (**Q29**), `map` and
`reduce` cannot live in the prelude either — a prelude function's closure names
a proto in the prelude's own chunk, which the unit being compiled does not have.
They are written per file until that is fixed.

**Rejected.** *HAMT and RRB now* — ADR-011 rejected it on line count and nothing
has changed except that we now know which operation to measure. *Transients* —
an API for the case `Rc::make_mut` already covers. *Numeric `=` across types* —
decides hashing as a side effect. *Truncating `/` on integers* — a silent wrong
answer under the spelling everybody types. *Float overflow throwing, for
symmetry with ADR-037* — symmetry with a rule whose reason does not apply.
*`count` on a string* — ADR-018 exists because that is where Unicode
correctness goes to die.

**Open.** Q29 (sharing compiled code between chunks, which milestone 9's REPL
forces anyway), and the representation question reopens the day a benchmark
says so.

---

### ADR-042 — The host boundary: io errors, no `HostCall`, and the handle table

*(New, 2026-07-27. Resolves Q27. Narrows ADR-013's opcode clause. Adds the
`:io-error` half of ADR-039 clause 3's vocabulary.)*

**Decision.** Five parts.

**1. An io failure is a throw of a map with four keys, plus `:path` where one
exists.**

```clojure
{:type :io-error :operation :open :path "data.txt"
 :kind :not-found :message "no such file"}
```

`:kind` for `:io-error` in v1 is `:not-found`, `:permission-denied`, `:closed`,
`:invalid-data`, `:interrupted`, and `:other`. The three network kinds the
original design conversation proposed — `:timeout`, `:would-block`,
`:connection-reset` — are **not** in this entry, because nothing milestone 7
builds can raise one; they arrive with the adapter that does, which is what
ADR-039 clause 3 means by a vocabulary growing in the entry that adds the
subsystem raising it.

`:operation` is the primitive that failed, as a keyword. `:path` is present only
when the operation names one, so a `stdin` failure has four keys and an `open`
has five.

**2. The raw host error code is not preserved, and `:message` is ours.** There
is no `:code` key. The prose in `:message` is constructed from our own kind
table, not forwarded from `std::io::Error`'s `Display`.

**3. There is no `HostCall` opcode.** An io primitive is a native like `+` is:
an ordinary entry in the global table, found by name, called by `Call`, handed
`&mut Vm` (ADR-038 parts 1 and 2). ADR-013's opcode clause is narrowed to
nothing; its *gating* clause is untouched, and Cargo features remain the
subtraction harness.

The rule that makes this sufficient: **a host call is atomic.** It completes or
it throws, and the VM never suspends inside one. There is no `Pending` outcome
and no way to re-enter a native.

**4. The handle table is VM-owned, generational, and hand-rolled.** A `u32`
index and a `u32` generation, a free list of reclaimed indices, and an occupancy
count. `close` is **idempotent on a live-or-already-closed handle and an error
on a stale one** — the two are different questions and only the second is a bug.
`Vm::open_handles()` answers ADR-029's "is anything open" as a subtraction, not
a scan.

`Value::Buffer` stays unused. A read allocates and returns fresh `Bytes`;
nothing writes into a caller-supplied buffer.

**5. `with-open` is a prelude macro, and no corpus program does io.** It lowers
to `try`/`finally` in `src/prelude.xs` (ADR-016, ADR-041 part 6). Io tests live
in `tests/lang/io.xs`, and the lang runner prepends a generated
`(def tmp-dir "…")` naming a per-run directory under `CARGO_TARGET_TMPDIR`.

**Why.**

*The kinds ship short because a kind nobody can raise is a guess with a colon in
front of it.* Pinning `:connection-reset` now means milestone 10 either fits a
real socket error into a name chosen before any socket existed, or supersedes
this entry — and the second is the honest outcome, so the first is the trap.
ADR-039 already built the mechanism for adding kinds later; using it is cheaper
than predicting.

*`:other` earns its place because the alternative is a lie about the host.*
`std::io::ErrorKind` is `#[non_exhaustive]`, and the set of things an operating
system can refuse is not ours to close. A vocabulary with no escape hatch would
route an unmapped disk error into `:vm-error`/`:internal`, which claims the VM
is broken about something that is merely unusual. `:other` is documented as *do
not dispatch on this, read the message* — it weakens the vocabulary exactly as
much as reality does, and no more.

*The raw code is dropped for determinism, not for tidiness.* An errno differs
across platforms and `std::io::Error`'s `Display` differs with it — "No such
file or directory (os error 2)" on one host, other words on another. ADR-039
makes a thrown value printable into a `.out` transcript, so either one would put
a machine-specific string in a golden file, and BUILD.md's rule is that a
flapping golden gets disabled and then there is no oracle. Owning the prose also
keeps `:message` what ADR-039 said it was: text this project reserves the right
to reword, which is only true if this project writes it.

Dropping it also keeps the key set closed, which a `:code` key would have
opened — a different promise from the one ADR-039 makes about `:kind`, made for
one field. Adding the key later is purely additive, so this is the cheap
direction to be wrong in.

*The opcode is not built because ADR-038 already delivered everything it was
for.* A native is reached by `Call` and gets `&mut Vm`, which is the entire
requirement of a blocking `read`. Building `HostCall` on top would add a third
arm to the dispatch loop and a second registry beside the native table, to reach
the same function by a different name.

The one thing an opcode genuinely buys is a **resume point**: a `HostCall`
instruction leaves the PC on itself, so "retry the instruction" is the natural
way back in after a host call that could not complete. Under `Call` the resume
point is inside a native with no `Proto` and no frame, so there is nothing to
resume to. That matters the day a host cannot block — wasm — and it is deferred
rather than ignored, because part 3's atomicity rule forbids the suspension it
would serve. Milestone 8 has to refuse the case anyway: ADR-029 already refuses
an `Image` while a handle is live, and an `Image` taken with a native on the
Rust stack is impossible under ADR-004 in either design. An opcode would be
buying a resume point for a suspension both designs forbid.

*The table is hand-rolled because `Value` already fixed the key type and ADR-016
needs a distinction the library merges.* `Value::Handle(HandleId)` is frozen at
`(u32, u32)` inside a 16-byte `Value` (ADR-025), so `slotmap`'s `KeyData` could
not *be* the identity — it would sit behind a conversion at every boundary and
milestone 8's serializer would have to reach inside a foreign type to write an
`Image`. And `SlotMap::remove` returns `None` for both an already-removed key
and a stale one, which is precisely the pair part 4 separates. Measured against
a written draft, the arena is ~40 lines, ~30 of them code, against a 700-line
host row. ADR-014 pre-approves the dependency and it is still declined: what it
would save is smaller than what stays ours either way.

The generational pattern is not being invented here — `slotmap`, `slab`, and
every ECS do the same thing, and `Vm::new_cell` is already a degenerate version
of it. What is new is the reclamation half: ADR-025 keeps a cell for the VM's
lifetime, so its generation is written once and never bumped, and a handle table
is the first thing in this system that frees a slot.

*`close` splits idempotence from staleness because they are opposite signals.*
Closing an already-closed handle is what a correct `with-open` does when the
body closed explicitly, and erroring there would make the safe idiom the
dangerous one. A stale handle is a live resource being addressed through a dead
name — the aliasing bug ADR-016 put a generation in the id to catch — and
swallowing it converts a use-after-close into silence.

*Reads return fresh `Bytes` because there is no mutable byte value.* ADR-016's
sketch was `(io/read h buf)`, written before ADR-041 made collections immutable
and copy-on-write. Filling a caller's buffer needs a mutable one, which is what
`Value::Buffer` would become — a second mutability story in a language that has
exactly one, for an allocation this scale cannot measure. The variant stays in
the enum unused rather than being given a job to justify it.

**Cost.** `:other` is a bucket, and every error that lands in it is one no
program can dispatch on; the kind table is the thing to extend when a real
program needs to.

A varying key set — four keys or five — means a handler that reads `:path`
unconditionally gets `nil` on a stdio failure. The alternative was a `:path` of
`nil` on every stdio error, which is a key that says nothing, present so the
count stays even.

Dropping the raw code means a failure we cannot classify arrives as `:other`
with prose, and the number that would have identified it is gone. The `.out`
transcript is the only place it could have gone, and that is the place it must
not be.

No `HostCall` means the wasm and async question is answered by refusal rather
than by design: when a host cannot block, this entry is what has to be
superseded, and the resume point is the specific thing that will need
inventing.

Handles are not reclaimed by scope, only by `close` or by the VM's end. A
program that opens in a loop and never closes exhausts file descriptors, and the
generation counter on a heavily reused index wraps at 2³² with no check.

**Rejected.** *All nine proposed kinds* — see why. *A closed vocabulary with no
`:other`* — misreports the host as an internal fault. *A `:code` key* — opens
the key set and puts a platform-specific integer where a golden can print it.
*A `HostCall` opcode* — narrows to ADR-038's existing `Call` with a second
registry attached. *`slotmap`* — the key type is already frozen and the hard
half stays ours. *`with-open` as a function* — a primitive calling a language
closure re-enters the dispatch loop on the Rust stack, which ADR-004 forbids
(ADR-041 part 6). *An `io/temp-dir` primitive* — a line in the host row that
exists only so a test can find a directory the runner already knows. *A corpus
program that opens a file* — puts a machine-specific path or error in a golden,
which is the failure BUILD.md's determinism rule exists to prevent.

**Open.** Whether `:interrupted` should be retried by the primitive rather than
thrown — `EINTR` is arguably the host's problem and not the program's, and the
answer wants a real program that hits it. The wasm resume point, per the cost
clause.

---

### ADR-043 — Fuel, the `Image` encoding, and the determinism the oracle rests on

*(New, 2026-07-27. Resolves Q22 and Q8. Completes ADR-029 on the encoding side.
Amends the `BUILD.md` budget table under ADR-030 part 3. Corrects a conflict
ADR-042 created.)*

**Decision.** Seven parts.

**1. v1 has no nondeterministic source, and that is a decision rather than an
omission.** No clock, no RNG, no environment read, no directory listing that
reaches a value. The round-trip property is therefore sound *by construction*:
there is nothing that could make two runs of the same program differ.

When a clock or an RNG does arrive, it arrives as **VM-owned seeded state
captured by the `Image`** — a counter and a seed that a snapshot carries and a
resume restores — and never as a primitive that reads the real world. A host
adapter that returns the wall clock is the one shape this entry forbids.

**2. The encoding preserves sharing. Every heap value gets an object id.** A
`Value` in the DTO is a 32-bit id into one object table, and two `Rc`s with the
same address encode to the same id. This covers strings, collections, closures,
and byte vectors; cells and handles were already ids (ADR-025, ADR-042).

**3. The DTO is flat, and v1 does not serialize it.** `Image` is `Vec`s and
`u32`s with no `Rc`, no `Value`, and no borrowed lifetime anywhere inside it.
The round-trip is `Vm` + `Execution` → `Image` → *fresh* `Vm` + `Execution`, in
memory. No `serde`, no bytes, no dependency.

**4. Fuel is a counter on `Execution`, decremented once per instruction.**
Reaching zero suspends at the instruction boundary before the next dispatch and
produces a third `Outcome`:

```rust
pub enum Outcome { Returned(Value), Threw(Unwind), Suspended }
```

`Suspended` carries nothing: the state is the `Execution`, which the caller
already holds.

**5. The standard streams are declared reconstructible, and only they are.**
`SnapshotHasLiveHandles` counts handles *beyond* `io/stdin` and `io/stdout`.
Those two are recreated by `host::install` at the same ids in any VM of the
same build, so a resume that rebuilds them is restoring them rather than
inventing them.

**6. The `Image` carries code identity, not code.** A chunk fingerprint, and
`resume` takes the chunk from its caller and refuses a mismatch. ADR-029 says
same-build only; a fingerprint is what makes that checkable rather than assumed.

The intern table travels **whole**, as its `names` vector — the `index` map is
derived and is rebuilt on resume. Symbol ids are positional, and a snapshot
that omits the table resumes with wrong identities and appears to work, which
`TRAPS.md` already lists as the dangerous one.

**7. The budget gains a row.**

| Layer | Target |
|---|---:|
| Fuel, `Image`, resume | 500 |

with the total moving from ~7,000 to **~7,500**.

**Why.**

*Answering Q22 with "neither" is a real answer and not a deferral, because the
half that is load-bearing is the half about the oracle.* `BUILD.md` makes the
round-trip property the oracle for constraint #2: run to fuel exhaustion,
resume in a fresh VM, compare the whole transcript. A program that reads a wall
clock produces two transcripts for reasons that have nothing to do with the
snapshot, the property flaps, and — by this project's own rule about flapping
goldens — gets disabled, at which point constraint #2 has no oracle at all. The
property is being written *now*, so the guarantee it needs has to exist now.

Building the seeded clock and RNG instead would prove the `Image` carries
counter state, which is otherwise an untested claim. It is the better version
of this entry and it is not v1's: it spends from a 500-line row alongside fuel
and resume, and a simulator that needs reproducible time can pass time in as an
argument, which is a thing the language can already do.

*Sharing is preserved because the alternative is exponential and reachable in
ten lines.* `(def b [a a])` twice over multiplies by four; four levels is
sixteen copies of one vector. Nothing in v1 can *observe* the difference —
`=` is structural, there is no `identical?`, and cells are already ids — so
this is not a semantics question, which is exactly what makes it easy to get
wrong and expensive to fix later. ADR-029 already said the DTO is object-id
based; treating that as a statement about cells only would have been reading it
narrowly to save an id table.

*Not taking serde keeps this milestone about the graph model.* The hard part of
a snapshot is the encoding — identity, cycles, ordering — and a flat id-based
`Image` is exactly the thing that has to be right. Once no `Rc` appears inside
it, a derive is plumbing, which is what ADR-029 claimed and what this entry
gets to leave as a claim honestly rather than by assertion. It also keeps
`Cargo.toml` at zero dependencies for one more milestone.

*Fuel decrements per instruction rather than per call or per backward branch,*
because the suspension point has to be somewhere a snapshot is legal, and
ADR-029 legalises exactly one place: an instruction boundary in compiled code.
Counting calls would suspend inside a native, which ADR-042 part 3 forbids;
counting backward branches would never suspend a straight-line program.

*Declaring the standard streams reconstructible is a correction, and it is the
reason this part exists.* ADR-042 made `io/stdin` and `io/stdout` permanent
entries in the handle table, so `open_handles()` is never zero and ADR-029's
refusal would reject **every** snapshot — milestone 7 made milestone 8's exit
condition unreachable, silently, and nothing failed because nothing yet asked.
ADR-029 anticipated the shape of the fix in its own words ("caches and host
registry entries are declared either reconstructible or excluded"); what it did
not anticipate is that a *handle* could be reconstructible. It can, for exactly
these two, because they name nothing the host has to reopen: `Host::Stdout` is
the buffered output and `Host::Stdin` is a stream a fresh VM addresses the same
way. A file is not, and never becomes one by this argument.

*The budget row is written before the code because rule 4 is about that
ordering.* The table in `BUILD.md` sums to 7,000 across seven layers and has no
row for serialization: ADR-029 created the `Vm`/`Execution`/`Image` split after
the table existed, and ADR-030 amended the total without adding a line. So this
layer has been unbudgeted since it was invented, and the difference between
deciding 500 now and recording 500 afterwards is the difference between a
budget and a measurement. 500 against ~1,200 uncommitted lines leaves the REPL
its 600 and keeps a margin ADR-030 already calls noise.

**Cost.** A fuel check on the dispatch loop, which is the hot path, on every
instruction. ADR-029 named this cost; it is now real.

The object table is built with a pointer-keyed map during encode, so encoding
is not a pure function of the value graph — it depends on `Rc` addresses, which
are stable within a run and meaningless across one. Nothing may ever compare
two `Image`s for equality; the round-trip property compares transcripts, and
that is the only comparison this design supports.

Preserving sharing means the decoder must handle a forward reference, because
an object can name an id it has not reached yet. A cell cycle is what ADR-029
built ids for and it is still the only cycle, but the decode is now two passes
rather than one for every object kind.

A fingerprint that matches does not prove the chunks are the same, only that
they hash the same. Same-build is the promise and a fingerprint is the cheap
check on it, not a proof.

And "no nondeterministic source" is a rule with no enforcement. Nothing stops
the next primitive from calling `SystemTime::now()`; what exists is this entry
and a reviewer.

**Rejected.** *A seeded RNG and virtual clock in v1* — see why; the better
version of this entry, not v1's. *Leaving Q22 open* — the property being
written this milestone is the thing that needs the answer. *Expanding shared
structure into copies* — exponential, and reachable in ten lines. *`serde` now*
— the graph model is the milestone; the derive is not. *Fuel per call or per
backward branch* — suspends where a snapshot is illegal, or does not suspend.
*Refusing a snapshot while stdio is open* — makes every snapshot illegal, which
is what ADR-042 accidentally did. *Carrying the chunk in the `Image`* — ADR-029
says code identity, and same-build means the caller already has the code.
*Fitting serialization inside the VM row* — hides an unbudgeted layer inside a
budgeted one, which is the thing per-layer reporting exists to make visible.

**Open.** Whether a resumed `Execution` can be resumed *again* — nothing here
forbids it and nothing tests it. Q29 still blocks a REPL, and milestone 9 will
want an `Image` per input, which is a harder shape than one `Image` per run.

---

### ADR-044 — A REPL session is one unit, and one chunk

*(New, 2026-07-27. Resolves Q1. Resolves Q29's REPL half and names why the
other half stays open. Says what ADR-040's "compilation unit" and ADR-008's
"parse unit" mean when there is no file.)*

**Decision.** Five parts.

**1. A REPL session is one compilation unit and one parse unit.** Not one per
input. Macros defined in one input are in scope in the next, the gensym counter
runs monotonically across the whole session and never restarts, and the reader
configuration — when there is any — is the session's to change freely. That
last clause is Q1, answered the way Q1 proposed: mutate freely at the REPL,
declare at the top of a file.

**2. A session has one `Chunk`, and each input is compiled into it.** Protos are
appended; existing indices never move. Evaluating input *n* means running the
top-level proto that input *n* just added, not proto 0.

**3. Therefore Q29 does not arise here.** A `Closure` still names its proto by a
bare index into one chunk (ADR-034 unchanged), `Value` and `Closure` are
untouched, the dispatch loop gains nothing, and `Image` still carries one
fingerprint for one caller-supplied chunk. There is no chunk registry and no
chunk id on a closure.

**4. The prelude half of Q29 stays open, and this is why.** The same mechanism
would fix it — compile the prelude into the unit's chunk and a prelude
*function* becomes possible. Not taken: the prelude's protos would then appear
in every chunk, so every `.disasm` golden in the corpus would carry them, and
they would grow with the prelude forever. `map` and `reduce` still do not live
in the prelude, and the reason is now a golden-file cost rather than an
impossibility.

**5. A failed input does not end the session, and input is plain stdin.** A
throw prints its transcript and the loop continues with the VM it already had.
Lines are read and buffered until a form is complete; there is no history, no
cursor movement, and no dependency. The session's semantics live in a `session`
module in `src/lib.rs`; prompts, stdin, and exit codes stay in `src/main.rs`
(ADR-031).

**Why.**

*One session, one unit is one sentence that settles three questions,* which is
the argument for it. Macros persisting, the gensym counter not restarting, and
the reader table's scope are the same question asked about three subsystems,
and answering them separately would have produced three rules to keep in
agreement. It also makes part 2 the natural shape rather than a trick: if the
session is the unit, the session having one chunk is what "a unit is compiled
into a chunk" already said.

The gensym half is the load-bearing one. A counter that restarted per input
would let input 2 mint a name input 1 had already used, and a fresh symbol that
is not fresh is the one thing gensym may not produce. Nothing about *file*
compilation changes — a file is still a unit, the counter still resets between
files, and every `.expanded` golden is untouched.

*Q29 is answered by removing the condition rather than by satisfying it.* The
question asks how compiled code can be shared *between* chunks; the answer is
that a REPL does not need two. The registry `QUESTIONS.md` proposed is the more
general fix and it is real, but its cost lands where there is least room: a
closure carrying a chunk id must resolve that id after an `Image` is restored,
so `restore` would have to serialize every `Chunk` — protos, instructions,
constants, spans — or take a registry from its caller, and the image row is at
400 of 500. Appending protos costs a function that compiles into an existing
chunk and an entry point that starts at a given proto. Nothing else moves.

The registry stays available. What would force it is migration — an `Image`
resumed against code assembled differently — and ADR-029 already promises
same-build, fresh-VM only.

*Errors leaving the session alive is not a new promise,* it is ADR-039's
existing one being used: there is one failure path, unwinding runs every
pending cleanup, and `tests/vm.rs` already pins that the machine is usable
afterwards. A REPL that died on a typo would be a REPL nobody develops in,
which is the stated exit condition.

*Plain stdin, because the terminal is milestone 10's and outside the budget.*
`BUILD.md` puts host adapters outside the line budget precisely so that
terminal handling does not compete with language work; taking `crossterm` now
would spend the REPL's 600 lines on the one part of milestone 9 that a later
milestone was going to do for free.

**Cost.** The chunk grows for the life of a session and nothing reclaims it:
every input's top-level proto is retained forever, whether or not anything
still refers to it. Bounded by typing speed, which is why it is acceptable and
not why it is right.

An `Image` taken at input 5 will not restore against the chunk as it stands at
input 7, because the fingerprint covers the whole growing chunk. Snapshotting a
REPL session therefore works only if nothing is evaluated in between, which is
close to useless. ADR-043 left "an `Image` per input" as milestone 9's harder
shape and this entry does not deliver it.

No line editing at all: no history, no arrow keys, no completion, and a typo on
a long line is retyped. That is a worse interactive experience than the exit
condition implies, and it is deliberate.

A prelude function is still impossible, now for a reason about golden files
rather than about closures.

**Rejected.** *A VM-owned chunk registry* — see why; more general, and its cost
falls on the `Image`. *One unit per input* — a gensym counter that restarts can
mint a name it already handed out. *Compiling the prelude into every unit's
chunk* — puts the prelude's protos in every `.disasm` golden and grows them
with the prelude. *`crossterm` now* — spends the language budget on the one
part of this milestone that is outside it. *A REPL that exits on a thrown
value* — ADR-039 already leaves the machine usable, so ending the session would
be discarding a guarantee that is already paid for.

**Open.** Q12 — one namespace — is untouched and a REPL makes it more visible,
because every input shares one global table and shadowing is how it will be
noticed. Whether a session can be *saved* — an `Image` plus its chunk — is the
shape ADR-043's open clause wanted and this entry does not reach.

---

### ADR-045 — Host adapters: where they live, what they cost, and the kinds they finally raise

*(New, 2026-07-27. Spends ADR-014's dependency budget. Adds the network half of
ADR-042's `:io-error` vocabulary. Takes ADR-015's second step for one directory
only.)*

**Decision.** Six parts.

**1. Adapters live in `src/adapters/`, and the budget excludes them by path.**
`term.rs`, `tcp.rs`, `json.rs`, behind `pub mod adapters` in `src/lib.rs`. This
is ADR-015's second step — one crate with file modules — taken for this
directory and nowhere else; `src/lib.rs` is still one file with inline `mod`
blocks.

The budget test excludes the directory and **prints what it excluded, with line
counts**. An exclusion nobody can see is how a budget stops measuring anything.

**2. The `:io-error` vocabulary gains its network kinds:** `:timeout`,
`:would-block`, and `:connection-reset`. ADR-042 shipped six and named these
three as belonging to "the entry that adds the subsystem raising them". This is
that entry, and they now have raisers.

**3. Two dependencies: `crossterm` and `serde_json`.** Both pre-approved by
ADR-014, both outside the line budget. TCP takes none — `std::net` is the
protocol.

The zero-dependency property ends here, and it ends *asymmetrically*: the
language has no dependencies and the adapters have two. That is the boundary
`BUILD.md` already draws, made visible in `Cargo.toml` rather than only in
prose.

**4. One Cargo feature per capability**, `default = ["fs", "tcp", "term",
"json"]`. `just subtract` builds and tests three points rather than the whole
2⁴ lattice: everything off, everything on, and `fs` alone. The middle point is
the one that catches a `#[cfg]` written as if two features always travel
together.

**5. A socket goes through the handle table like a file, and refuses a snapshot
like one.** `Host::Tcp` and `Host::Listener` join `Host::File`. ADR-043 part 5
declares only `io/stdin` and `io/stdout` reconstructible, so a live socket
makes `capture` return `SnapshotHasLiveHandles` with no new code and no new
decision — which is the handle table doing the job ADR-016 built it for.

**6. JSON maps to values losslessly, and object keys become strings.**
`null` ↔ `nil`, `true`/`false` ↔ booleans, arrays ↔ vectors, objects ↔ maps
with **string** keys. A JSON number is an `Int` when it is integral and fits
`i64`, otherwise a `Float`. Encoding `##NaN` or `##Inf` throws
`:type :io-error :kind :invalid-data` rather than emitting something no parser
accepts.

**Why.**

*File modules for one directory is the first level of ADR-015's progression
that suffices, which is the rule ADR-015 states.* A workspace would be the
harder boundary — adapters could then only reach the VM through its public API
— and ADR-015 reserves that for a deployment boundary. There is no deployment
boundary here: it is one binary, and the subtraction that matters is already
done by Cargo features, which work within one crate.

The exclusion printing is the part worth arguing for. `BUILD.md` has said
adapters are outside the budget since before any existed, and until now that
cost nothing because the sentence described an empty set. The moment it
describes real files it becomes a way to move lines out of a budget by moving
them into a directory, and the only defence is that the move is visible on
every run.

*The network kinds arrive now because ADR-042's argument was that a kind
nobody can raise is a guess with a colon in front of it.* That argument only
pays if the deferral is actually honoured — an entry that defers three kinds
and then never adds them has not been careful, it has been lucky. `:timeout`
and `:would-block` come straight from `std::io::ErrorKind`;
`:connection-reset` is the one a program most wants to retry on and the one a
file can never produce.

*The dependencies are ADR-014's own test applied, not re-litigated.* "Would
implementing this teach us something about our language, or merely reproduce a
protocol or standard algorithm?" JSON string escaping, number formats, and
terminal capability detection are all the second thing. Writing them here would
add several hundred lines of protocol nobody reviews carefully, outside the
budget so nothing would even flag the growth.

What is lost is a fact two write-ups assert. It is worth being precise rather
than quietly dropping it: **the language still has zero dependencies**, and it
is checkable — `cargo tree --no-default-features` shows nothing. The bylines
need a clause, not a deletion.

*Object keys are strings because keywords do not round-trip.* Our keywords are
interned and can hold any text, so `{"a b": 1}` could become `{:a b 1}` — which
prints as something the reader would read back as two forms. Type-strict `=`
(ADR-041) makes the choice visible rather than subtle: a program that got
`:a` where it expected `"a"` fails immediately instead of silently missing a
lookup. Keywordising is a convenience a caller can apply; un-keywordising after
a lossy conversion is not available to anyone.

*`##NaN` throws on encode because JSON has no spelling for it.* ADR-032 made
the reader and printer agree on a spelling for every float a program can hold,
and JSON is a third party that agrees with neither. Emitting `NaN` produces a
document no conforming parser reads; emitting `null` silently turns a number
into an absence. A throw is the only option that does not lie.

**Cost.** Two dependencies, their transitive trees, and their release cadence,
in a project that had none. `crossterm` in particular is a large surface for
what will initially be a handful of primitives.

Three features means eight subtraction builds exist and three are tested. A
`#[cfg(all(feature = "tcp", feature = "json"))]` written by mistake would be
caught; one written as `#[cfg(any(...))]` in the two untested combinations
would not.

The JSON mapping is not a bijection in the other direction: a map with
non-string keys cannot be encoded, and that is a throw at encode time rather
than something the type system prevents. Same for a map that is not a valid
JSON object at any depth — the failure is found by walking, and the position it
reports is the value, not the source that built it.

A terminal adapter exists that the REPL does not use. ADR-044 chose plain stdin
and this entry does not revisit it: wiring line editing into the prompt would
make the REPL depend on a *feature*, so `--no-default-features` would produce a
REPL with different behaviour rather than a smaller one. That is a decision, not
an oversight, and it is the one thing here most likely to look like unfinished
work.

**Rejected.** *A workspace crate* — ADR-015 reserves it for a deployment
boundary and there is none; Cargo features already subtract within one crate.
*Hand-rolling JSON and terminal handling* — ADR-014 exists to decline exactly
this, and outside the budget the reimplementation would grow unobserved.
*Keywordised JSON object keys* — not reversible, and type-strict `=` turns the
mismatch into a silent failed lookup. *Emitting `null` for `##NaN`* — turns a
number into an absence. *Testing the whole feature lattice* — eight builds per
gate run for two combinations nothing else distinguishes. *Rewiring the REPL
onto `crossterm`* — makes a feature change the prompt's behaviour rather than
its size.

**Open.** Whether the REPL should eventually use the terminal adapter, and what
it would mean for `--no-default-features` if it did. Whether an adapter can ever
checkpoint a live handle, which ADR-029 called "a later opt-in" and nothing has
needed yet.

---

### ADR-046 — `parse-number`, and one number grammar

*(New, 2026-07-27. Repairs an ADR-013 violation. Narrows nothing; adds a core
primitive. Prompted by Q31's probe programs.)*

**Decision.** Four parts.

**1. `parse-number` is a core primitive.** `(parse-number s)` takes a string and
returns an `Int` or a `Float`.

It exists because the language had **no string-to-number conversion at all**.
Not an awkward one — none. The prim table converts value→string via `str` and
never back, so a length header, a config value, a command-line argument and a
line of user input were all unreachable. Ten milestones did not notice, because
nothing outside the test suite had been written and a suite that exercises the
VM builds its numbers as literals.

**2. This repairs ADR-013 rather than extending anything.** The one path that
worked was `(json/decode "27")`, and `json` is an *optional host adapter*. So
`--no-default-features` removed the language's only way to read a number out of
text. ADR-013 says features gate host capability and never language semantics;
that had quietly stopped being true — not because a feature gated a semantic,
but because a semantic had no home and was squatting in a feature.

`just subtract` could not catch it and still cannot catch its like. The build
without `json` was green because nothing in the suite parses a number from a
string either. **The subtraction harness proves a capability can be removed; it
cannot notice that a semantic went missing with it.** That is a real limit on
ADR-013's test, and it took writing a program to see.

**3. It *is* `reader::parse_number`, not a second implementation.** One grammar,
so a literal and a parsed string cannot drift. Two consequences that make this
more than tidiness:

- The non-finite spellings **move into** `parse_number`. They were three arms of
  the reader's token match, ahead of the call, which is exactly why
  `(parse-number "##Inf")` first answered `nil` while the reader read the same
  three characters as a float. A shared grammar that the caller can add cases in
  front of is not shared.
- `print` and `parse-number` are therefore inverses over every number, non-finite
  ones included — pinned in `tests/lang/numbers.xs`.

**4. `nil` for a string that is not numeric-looking; a fault for one that is and
still is not a number.** `"abc"` is `nil`; `"1abc"`, `"1.2.3"`,
`"99999999999999999999"` and `"1e400"` all raise `:type`, carrying the reader's
own message.

Two failure modes rather than one is a real cost, taken because error quality
sits outside the priority ranking in `ETHOS.md`. A `nil` for `"1e400"` tells the
caller nothing; "overflows to infinity; write `##Inf` to mean it" tells them
what happened and what to write instead.

**The seam is whitespace, and it is inherent.** `" 27"` is `nil` and `"27 "`
faults. The asymmetry looks arbitrary and is not: `parse_number` was written to
take a *token*, and the reader splits on whitespace before ever calling it, so
the reader cannot reach either case. The prim can, and the rule it applies is
the function's own — does this start like a number? `" 27"` does not, so it is a
plain "no". `"27 "` does, so it is a number that turned out not to be one.

**Also corrected: a typo is not an overflow.** `1abc` reported "number `1abc`
does not fit in a 64-bit integer", which sent a reader looking for a range
problem in a token containing letters. Both cases reach the same branch —
numeric enough to commit to, not a number in the end — and are now told apart by
whether the remaining characters are digits.

**Rejected.** *Returning `nil` for everything*, which is Clojure's
`parse-long`. Simpler, one failure mode, and it throws away the diagnostics the
reader already has. *Parsing in the primitive*, which would give the language two
answers to "what is a number" and no test that they agree. *Trimming
whitespace*, which would make `parse-number` accept text the reader rejects and
put the two grammars back out of step in a new place.

**Open.** Whether the inverse property should be a test over generated numbers
rather than the six cases pinned by hand — the reader fuzzer has the machinery
and this has not needed it yet.

---

### ADR-047 — `loop` and `recur` are core forms

*(New, 2026-07-27. Resolves Q5. Applies ADR-028 rule 2 rather than amending it.
Diverges from Clojure once, on purpose.)*

**Decision.** `loop` and `recur` are core forms. Q5 budgeted for "a fourteenth
special form", singular; the answer needs two, because `recur` without `loop` has
no target and `loop` without `recur` is `let`.

**1. A `loop` is a `let` around an immediate call to an anonymous function whose
parameters are the loop's names. A `recur` is a tail call to that function.**

This is the entire implementation, and it is chosen for one property: **there is
no second definition of tail position.** "Tail position for `recur`" is tail
position for the loop's function, which is the flag the compiler already threads
for every other tail call. The lowering for `Core::Recur` reads that decision and
never makes its own.

That property is the whole reason this is a core form. A prelude macro with
identical semantics is eight lines and was written first
(`notes/loop-recur-attempt.md`); what it could not do was diagnose, and every fix
for its diagnostics required reimplementing tail-position analysis in the
prelude — two definitions, in two languages, with no test that they agree, whose
drift is a `recur` the macro accepts and the compiler does not make a tail call.
A silent stack leak in the one construct people reach for to avoid one.

The outer `let` is not ceremony. It keeps bindings sequential, as `let`'s are, so
`(loop [a 1 b a] …)` sees `a`. Passing the initialisers straight in as arguments
would evaluate them in the outer scope and change that without saying so.

**2. What it refuses, and the rule each refusal comes from.** None is new
policy:

| Refused | Because |
|---|---|
| `recur` outside a `loop` | there is no function for it to re-enter |
| `recur` across a `fn` | the inner function's frame is not the loop's frame |
| `recur` with the wrong arity | it rebinds the loop's names, so it takes that many |
| `recur` not in tail position | it re-enters rather than returning a value |
| `recur` across a `try` | ADR-028 rule 2 — the frame is still needed |

Arity is checked against the *loop* during resolution rather than at the call,
because the loop's arity is known there and "rebinds the 1 name(s) its `loop`
binds, given 2" beats an arity error naming a function the user never wrote.

**3. Lowering becomes fallible.** Whether a `recur` is in tail position is known
only while lowering, so `Lower` carries the first refusal and `compile_into`
returns it. The alternative — threading tail position back into resolution —
would have created the second definition this entry exists to avoid.

A refused compile leaves the chunk exactly as it was. Under ADR-044 a REPL
session's chunk is shared across inputs, so a half-appended proto would renumber
every closure compiled after it.

**4. `recur` targets a `loop` and never the enclosing `fn`.** Clojure allows
both. Here the second is unnecessary: ADR-028 already makes a self-call in tail
position run in constant space, so `recur` in a function body would buy a
spelling and nothing else.

**5. A `recur` from a `catch` is allowed. This differs from Clojure and the
reason is a property of this VM.** The VM pops a handler record when it
dispatches to it, so by the time a catch body runs there is no region left to
leave — `regions` is 0 and the ordinary tail-call rule permits the jump. 50,000
iterations through a firing `catch` run in constant space and leave the handler
stack clean. With a `finally` there *is* an open region and the same rule refuses
it, with no case analysis anywhere: the difference falls out of the counter
ADR-028 rule 2 already maintains.

**Cost.** `compile` goes from 942 lines to 1,138, and core from 6,088 to 6,303
of 7,500. `compile` was already the largest layer and still is.

**Rejected.** *The prelude macro* — complete on semantics, three unfixable
diagnostics, and the fixes cost more than this did (`notes/loop-recur-attempt.md`
has the measurements). *A dedicated jump-based loop with its own header and
backward branch* — faster by a frame per loop entry, and it would have needed its
own answer to what tail position is, which is the thing being avoided. *Leaving
it out* — Life shrinks from 17 top-level definitions to 12 with this, and the
five that go were never about Life.

**Open.** Whether `recur` should eventually accept the enclosing `fn` as a
target after all, if a `fn` body ever wants rebinding that a self-call spells
worse. Nothing has wanted it.

---

### ADR-048 — Prelude functions, appended after the unit

*(New, 2026-07-28. Resolves Q29. Uses ADR-044's mechanism for the case ADR-044
deferred. Adds the sequence library.)*

**Decision.** The prelude may define runtime functions, not only macros. They are
compiled into every unit's chunk **after the unit's own protos**, and the
prelude's top level is its own proto, run before the unit's.

**1. Appending after the unit is the entire decision.** A unit's proto indices
then depend on the unit alone, so a `.disasm` golden is pinned to the program it
names and does not move when the prelude gains a function.

The cost of getting this wrong was measured before choosing: four sequence
functions compile to 9 protos and 160 lines of disassembly. Compiled in front of
each unit that is **+1,440 lines across the nine corpus goldens**, taking them
from 672 to 2,112 — a 3.1× blow-up for four functions, growing with every
function ever added. That is the cost Q29 declined for four milestones, and it
was right to. As built, six functions were added and **not one golden changed.**

**2. The prelude's top level is a separate proto.** The driver runs it, then the
unit's `protos[0]`. The alternative — the definitions as a prologue inside the
unit's `<top>` — would put prelude instructions in every program's `<top>` and
in every `.expanded` golden, which is the same leak by another route.

**3. A session orders it first; a file orders it last.** Both keep the same
promise, and they differ because a REPL session's inputs keep appending, so
"last" is not a place. `Chunk::prelude` is therefore a `PreludeSpan { top, len }`
rather than a boundary index — a span can say either. A session runs the prelude
once at construction rather than per input: a session is one unit (ADR-044), and
re-running the definitions at each prompt would rebind names the user may have
deliberately shadowed.

**4. Expanding the prelude now has two products.** Its `set-macro!` forms are
consumed as before and leave nothing behind; its `set-global!` forms are code
that still has to run and are carried to the compiler. The filter is a
**whitelist** — only `set-global!` is kept — because a prelude form that is
neither is a form whose meaning nobody decided.

`prelude_definitions` runs *before* the unit is expanded. Expanding the prelude
advances the gensym counter and `expand_all` resets it, so doing this first
leaves no trace on the unit. Done afterwards it is still safe for the unit, but
the prelude's own gensyms would be numbered from wherever the program's expansion
stopped — and the prelude would compile differently for different programs, which
is a determinism failure with a golden attached.

**5. The prelude gets its own golden.** `apolisp prelude` disassembles it
standalone, pinned by `tests/prelude.disasm`. Without it the prelude's functions
would be the only code in the language pinned nowhere: compiled into every unit
and printed in none of them. `just bless` regenerates it with the rest.

**6. The library, and its shape.** `map`, `filter`, `reduce`, `range`, `repeat`,
`join`.

They take and return **vectors, not lazy seqs**. There is no laziness here, and
`conj` extends a vector where it is cheap, so a seq abstraction would buy nothing
but a second collection to explain. `reduce` takes an explicit seed **always** —
Clojure's one-argument form takes the first element as the seed and errors on
empty, which is two behaviours behind one name. `join` puts the separator
between and never after, which is the off-by-one every hand-written version gets
wrong once.

**Cost.** Core goes from 6,303 to 6,561 of 7,500; `prelude.xs` from 66 lines to
118. Life, the probe from `notes/first-programs.md`, goes from 48 lines to 26 —
37 of those with `loop`/`recur` alone (ADR-047) and the rest from these six.

**Rejected.** *Prepending the prelude*, measured above at 3.1× on the goldens and
unbounded. *A separate prelude chunk with tagged proto indices* — cleanest
separation, and it wanted the high bit of every proto index plus surgery on frame
handling and ADR-029's fingerprint, which is the machinery constraint #2 depends
on; not worth it when ordering alone buys the same golden stability. *A
concatenated `lib/seq.xs`*, which is what `tests/lang/harness.xs` already does
and what Q12 documents — zero risk and zero cost, and `map` is unavailable until
you paste it.

**Open.** Whether the prelude should ever hold something that is neither a macro
nor a `set-global!`. The whitelist will refuse it, loudly, which is the intended
way to find out.

---

### ADR-049 — The string surface names its units

*Decision stands; its rejection of scalar-indexed slicing is superseded by
ADR-052, which explains why that clause misread ADR-018.*

*(New, 2026-07-28. Refines ADR-018 and ADR-041 part 5. Removes `str-len`.
Prompted by `notes/the-report-program.md`.)*

**Decision.** Five parts.

**1. `str-len` is removed. `str-byte-len` and `str-scalar-len` replace it.**

ADR-041 part 5 already made the rule, and made it as an error message: `count`
refuses a string and *says to name the unit*. The surface then pointed at
`str-len` as the byte answer — a name that does not name its unit. The language
was refusing the ambiguous spelling and shipping one.

It answered 5 for `"josé"`, so the column-padding idiom every report writes
misaligned by one space per non-ASCII character, with no error and nothing to
notice. `str-slice` raises when a cut lands inside a character and `count`
refuses outright; this was the hole in the same argument.

**2. Why `str-index-of` returns a byte index and does *not* say so.** The
distinction is worth stating, because it is the rule for the next function
somebody adds:

> An ambiguous unit is dangerous when it flows into **arithmetic** and safe when
> it flows into an operation that **validates**.

An index from `str-index-of` can only be spent on `str-slice`, which raises
rather than guessing when a bound lands inside a character — so a unit mistake
announces itself on the first non-ASCII input. `str-len`'s result was spent on
subtraction, and arithmetic checks nothing. This is why the fix is naming rather
than a blanket rule about byte offsets.

**3. `str-index-of` is added:** `(str-index-of s needle from)` → byte index or
`nil`. The `from` offset exists so a scan does not re-read what it has passed,
which is what keeps `split` linear rather than quadratic in slices. An empty
needle is refused: it matches at every position, so every caller that advances
past a match would loop forever, and that is better refused once here than
guarded in each of them.

**4. `split`, `pad-right` and `pad-left` join the prelude.** They are the three
functions every text program defines before it can start.

The padders use `str-scalar-len`. That is the point of the entry: **a column
lines up because the library counts characters, not because each caller
remembered which unit a length was in.** They return the string unchanged when
it is already too wide, because losing data to make a column line up is the
wrong trade to make silently.

`split` keeps empty fields — a trailing separator yields a trailing empty field.
Dropping it is the caller's decision and not the split's.

**5. The surface stays byte-indexed.** ADR-018 promises no O(1) character
indexing and that is unchanged; `str-slice` still takes byte indices.

**Rejected.** *Keeping `str-len` as an alias for the byte version* — the
ambiguous name is the defect, so keeping it keeps the defect and adds a synonym.
*Redefining `str-len` to mean scalars*, which reads like the friendly choice and
is worse: its result would no longer be a legal `str-slice` bound, so
`(str-slice s 0 (str-len s))` would be correct on ASCII and wrong on everything
else — converting a loud misalignment into a silent truncation. *Scalar-indexed
slicing*, which is ADR-018's O(1)-character-indexing promise being broken to
avoid naming a unit.

**Cost.** Core goes from 6,561 to 6,639 of 7,500; `prim` 702 → 755 and
`prelude.xs` 118 → 143. The report program that prompted this goes from 54 lines
to 41, and the standard library it has to define first from 30 lines to 19 — all
19 of which are `sort`, which this entry does not address.

**Open.** `sort`, `take`/`drop`, and `apply`, all named by the same program and
none of them a string question.

---

### ADR-050 — `compare`, and ordering that stops where it runs out

*(New, 2026-07-28. Adds `take`, `drop`, `sort`, `sort-by`, `sort-with`,
`compare`. Prompted by `notes/the-report-program.md`. Defers `apply` to Q32.)*

**Decision.** Four parts.

**1. `compare` orders within numbers and within strings, and refuses
everything else by name.** It answers `-1`, `0`, `1`.

It exists because `<` and friends are **numbers only**, so before this there was
no way to order two strings at all — and a `sort` that cannot order a list of
names is not a sort anybody wanted. That is the gap, and it was invisible until
a program tried to print a report.

**It is deliberately not a total order over every value.** Ordering a keyword
against a vector is a decision with no obvious answer and nothing has needed
one, so the pair is refused rather than given a ruling. The refusal has two
messages, because they are two different mistakes: a keyword against a keyword
is "this type has no order", and a number against a string is "these two do not
share one". One message for both reads like a bug in the checker.

Strings order by their UTF-8 bytes, which for UTF-8 *is* code-point order — so
this is not a byte-order quirk, it is the answer scalar-by-scalar comparison
would give. **It is not a collation.** `"Z"` sorts before `"a"` and no locale is
consulted, which is a limit to know rather than a bug to file.

Numbers reuse the comparison `<` already makes, across `Int` and `Float`, so
`compare` and `<` cannot disagree and `##NaN` is refused here exactly as it is
there.

**2. `take` and `drop` clamp rather than raising.** Asking for more than there
is, or for a negative count, is how a caller says "all of it" and "none of it" —
and every use site would otherwise write the same two guards.

**3. `sort`, `sort-by`, `sort-with` — three names, because there is one arity
per name.** Clojure spells these as overloads of `sort` and `sort-by`; without
multi-arity the honest translation is three names rather than a variadic
signature that inspects its arguments.

**It is a merge sort, and that is the decision, not the detail.** The version
everyone writes first — take the smallest, remove it with `filter`, repeat —
removes *every* element equal to the smallest rather than one of them, so it
silently drops ties. That is in `TRAPS.md` because it is wrong on the first
input with a duplicate and looks right on every input without one.

The merge takes from the left when neither element is less, which makes it
**stable**: equal elements keep their input order, so sorting by one key and
then another composes the way people expect.

`sort-by` calls its key function once per *comparison*, not once per element. An
expensive key wants pairing up first — a thing to know rather than a thing to
hide.

**4. `apply` is not here.** It is the one item on the list no program asked
for — it came from a capability probe rather than from writing something — and
it is much deeper than it looks. Filed as **Q32** with the measurement.

**Cost.** Core goes from 6,639 to 6,741 of 7,500. The report program that
prompted this arc goes 54 lines → 41 → **21, with no hand-written library at
all**; it is now only the program.

**Rejected.** *A total order over all values*, which would need a ruling on
keyword-versus-vector that nothing needs and everything would then be stuck
with. *Making `<` work on strings* — Clojure keeps `<` numeric and puts general
ordering in `compare` for the same reason, and a mixed-type `<` would need the
total order above anyway. *A native `sort`*, which would have to call a language
comparator, and ADR-041 part 6 forbids exactly that.

**Open.** `apply` (Q32). Collation, if a program ever wants human-order names
rather than code-point order.

---

### ADR-051 — How a program paints a terminal

*(New, 2026-07-28. Adds `term/open`. Prompted by
`notes/the-pager-program.md`. Narrows Q33 to the general case and leaves it
open. Widens ADR-045 part 4's lattice by one point.)*

**Decision.** Three parts.

**1. A program paints a terminal by opening it as a handle.** `(term/open)`
returns a read/write handle on `/dev/tty`, and painting is `io/write` to that
handle — the same primitive a file and a socket use.

`io/stdout` is not that path and does not become one. It is the buffered host: a
write goes to `Vm::emit`, into `Vm::out`, and nothing reaches the process until
the program ends.

**2. A program that paints a terminal cannot be snapshotted, and that is the
point rather than a limitation.** The handle is not reconstructible, so ADR-043
part 5 makes `capture` refuse while it is open, with no new code and no new
decision.

This is the part worth being explicit about, because it is what makes the choice
between the shapes. ADR-029 requires emitted effects to be **part of the
serialization comparison rather than escaping it**, and the `Image` serializes
`Vm::out` — buffered output is resumable machine state, not a test fixture. Any
path that writes to a real descriptor produces bytes the `Image` cannot carry,
so the invariant that has to hold is *if output escaped the buffer, refuse the
snapshot*. Routing painting through a handle makes the handle table enforce that
invariant, using machinery ADR-016 already built. Every other shape has to
re-derive the refusal by hand.

**3. Painting is `term`'s capability, not `fs`'s.** `Host::File` is now
`any(feature = "fs", feature = "term")`, and `just subtract` gains `term` alone
as a fourth lattice point.

Before this, the only spelling was `(io/open "/dev/tty" :write)`, which is `fs` —
so a build with `term` and without `fs` could read keys and could not paint. A
half-present capability that no build compiles and no test runs is the shape
ADR-046 already recorded once, and one instance was an accident.

A tty is `Host::File` and not a parallel variant, because it reads and writes
like a file and refuses a snapshot like one. A `Host::Term` would be three new
arms restating all three behaviours.

**Why `/dev/tty` and not stdout.** A program whose stdout is a pipe still has a
controlling terminal, and the pipe is what the `.out` transcript wants. Opening
the terminal by name keeps those two things separate, which is what lets a
painting program still have a golden.

**Cost.** Adapters go from 414 to 439 lines outside the budget. Core goes from
6,749 to 6,756 of 7,500 — the terminal is an adapter, but the `Host::File` gate
and the comment explaining why two capabilities construct one variant are in
`host`, and they count. `just subtract` runs six cargo invocations instead of
four. The
`#[cfg]` on `Host::File` now names two features, which is one more place to be
wrong — mitigated by the new lattice point, which fails to compile if the gate
narrows again (verified by reverting it: six errors in the `term`-only build).

The real cost is that `/dev/tty` is Unix. This has no Windows story and does not
pretend to; ADR-045 already delegates terminal portability to `crossterm` for
input, and the output half now has a boundary `crossterm` is not behind.

**Rejected.** *A `term/write` native writing straight to the descriptor.* It is
the symmetric-looking answer and it silently breaks the oracle: it does not go
through the handle table, so `capture` would accept a program that had already
painted, the painted bytes would be absent from the `Image`, and a resumed run
would diverge from an uninterrupted one. The round-trip property would not
notice, because its corpus does not paint terminals — which is milestone 8's
documented blind spot, re-created on purpose.

*An unbuffered stdout mode selected by the driver.* It makes `Vm::out`
sometimes-machine-state and sometimes-not with `capture` unable to tell which,
and it makes `.out` depend on flush timing — which is `main.rs`'s own argument
against giving the CLI a fuel flag.

*Making `term` depend on `fs` in `Cargo.toml`.* Cheapest, and it contradicts
ADR-045 part 4: one Cargo feature per capability, so that cutting one cuts one.

**Open.** The general case, still **Q33**: `io/stdout` remains all-or-nothing,
and a program that wants incremental output to a *pipe* has no answer at all.
The shape for that is a real stream with an explicit flush point, which reaches
ADR-016 and has to supersede `io/close`'s reasoning that dropping the descriptor
is what flushes it. Deferred rather than decided — nothing has asked, and the
terminal case that had asked is answered here.

Also open, and independent of all of this: **`term/raw-mode` mutates
process-global state that the `Image` does not carry and that `capture` does not
refuse on.** A terminal program holding no other handle can be captured
mid-session and resumed in a fresh process with the mode wrong. The transcript
would match, because output is buffered, so the round-trip property cannot see
it. ADR-045 already notes raw mode outlives a failed program; this is the same
fact reaching serialization.

---

### ADR-052 — `str-scalar-slice`, and the rejection that misread ADR-018

*(New, 2026-07-29. Adds `str-scalar-slice`. **Supersedes ADR-049's rejection of
scalar-indexed slicing.** Resolves Q34. Prompted by
`notes/the-editor-program.md`; predicted in
`notes/str-scalar-slice-prediction.md`.)*

**Decision.** `(str-scalar-slice s from to)` returns the substring between the
**scalar** indices `from` and `to`. It raises when `from > to` or when `to`
exceeds the string's scalar length. It is **O(n) in the string** — one
`char_indices` pass, with no index built and nothing cached.

**1. Why ADR-049's rejection does not hold.** That entry rejected this primitive
in one clause: *"Scalar-indexed slicing, which is ADR-018's O(1)-character-indexing
promise being broken to avoid naming a unit."* Every part of that is wrong, and
the parts are worth separating because the same mistake is easy to repeat.

ADR-018 does not promise O(1) character indexing. It says **"No promise of O(1)
character indexing"** — a declined guarantee, not a prohibited operation. A
linear scalar slice keeps that non-promise exactly: it is O(n), and this entry
says so in the decision line rather than in a footnote. The rejection read a
performance disclaimer as a ban.

ADR-018's decision text also says **"Separate explicit operations for bytes,
scalar values, and graphemes."** A scalar-indexed slice is the second of those
three. The rejection cited the entry that asks for the operation as the reason
not to add it.

And it does not "avoid naming a unit" — the name contains the unit, which is
ADR-049's own rule (part 2) being followed rather than dodged. `str-slice` speaks
bytes and says so; `str-scalar-slice` speaks scalars and says so.

**2. Why linear is enough, and why O(1) was never the question.** The defect Q34
records is not that slicing costs more than O(1). It is that the
character-correct path had no native slice at all, so it went `str-scalars` →
`take`/`drop` → `scalars-str`, and `take`/`drop` are prelude `conj` loops —
quadratic in the column (Q6/E-11). The fix is replacing a quadratic
language-level loop with a linear native scan. Dropping an exponent is the win;
the constant was never the point.

**3. The byte surface is unchanged.** `str-slice` still takes byte indices and
still raises inside a character. Two slices now exist, in two units, both named
— which is ADR-049's rule applied at a second site, not weakened.

**4. What this does not fix**, stated because a large speedup invites the
assumption that the program is fast:

- **Graphemes.** A scalar is not a character. Backspace still splits `e` + U+0301
  and still dismantles a ZWJ sequence, and this primitive makes that *faster*
  rather than correct. See `TRAPS.md`.
- **`RET`.** `split-line` rebuilds the line vector with `concat`, which is
  O(buffer) and is not a string question at all.
- **Q6/E-11.** `take`, `drop`, `map`, `filter` and the rest stay quadratic
  everywhere else. This entry removes the one call site that hurt, and removes no
  exponent from the collection surface.

**Rejected.** *`str-scalar-offset`* — scalar index to byte offset, composed with
the existing `str-slice`. Smaller, and safe by ADR-049 part 2's test, since its
result flows into an operation that validates. Rejected on ergonomics rather than
safety: it hands the caller a way to compute byte indices so that the byte API
stays the one being called, so the correct operation remains two steps and
remains byte-flavoured. The recurring shape in `TRAPS.md` is that the surface
built to keep Unicode honest is the one nobody can afford to use, and an offset
routes around that shape where a slice removes it.

*Making `take`/`drop` native* — fixes this call site and not the general case, and
ADR-041 part 6 has opinions about natives that call back into the language.
*Fixing Q6/E-11 first* — the right answer eventually and much larger; it stays
open, and this entry does not depend on it.

*Caching a scalar→byte index on the string object* — would make repeated slices
of one line O(1) and is the obvious next step if a measurement ever asks for it.
Rejected now because it adds mutable state to an immutable value for a cost
nothing has yet shown, and ADR-018's non-promise is what makes declining it
legal.

**Cost.** Core goes from 6,561 to 6,616 of 7,500 — **55 lines, against the 25–35
predicted**, and the overrun is comment rather than code. ADR-030 counts comments
on purpose, so this is a real cost and not an accounting artifact. The editor's
`scalars-take` and `scalars-drop` stop existing, and its `.disasm` loses two
protos.

Measured, release, length-preserving edit at the middle of the line:

| line length | old idiom | `str-scalar-slice` | speedup |
|---:|---:|---:|---:|
| 100 | 93.6 µs | 0.92 µs | 102× |
| 1,600 | 9,683.8 µs | 4.54 µs | 2,134× |
| 25,000 | 2,213,853.5 µs | 43.58 µs | 50,802× |
| 100,000 | — | 165.68 µs | — |

The last two rows are the point: 4× the characters costs 4.01× and 3.80×, so the
primitive is **linear and measurably so**, exactly as the decision line claims. It
is not flat, and nothing here needed it to be.

**Open.** Graphemes, as a decision about a dependency, a table, or a stated
refusal — **Q35**. `split-line`'s O(buffer) rebuild, now the largest remaining
cost in the editor and conspicuous only because this entry fixed the other one —
**Q36**. Q6/E-11, which is under both of them.

---

### ADR-053 — `vec-slice`, and why `take`/`drop` stop being loops

*(New, 2026-07-30. Adds `vec-slice`. Rewrites `take` and `drop` onto it.
Resolves Q36. Predicted in `notes/vec-slice-prediction.md`.)*

**Decision.** `(vec-slice v from to)` returns the elements between two indices,
half-open, as a vector. It accepts a **list or a vector** and always returns a
vector. It raises when `from > to` or `to` exceeds the count. One copy, O(n).

`take` and `drop` become clamping wrappers over it and stop being `conj` loops.

**1. The clamping stays in the prelude, and the primitive raises.** ADR-050 gave
`take`/`drop` a clamping contract — asking for more than there is means "all of
it". `vec-slice` refuses a bound it cannot honour, the way both string slices do,
and the two functions that promise clamping do the clamping. A primitive that
guessed would make the guess unavailable to callers who want the error.

**2. It is lenient about its input because `take`/`drop` always were.** `concat`
returns a *list*, and `(take 2 (concat [1 2] [3 4]))` has always worked and
always returned a vector. A strict primitive would have broken that on the way
past, silently for anyone not testing the list case. There is now an assertion
pinning it.

**3. Why not `subvec`.** Clojure's `subvec` is vector-only and an O(1) view over
shared structure. This one takes a list and copies. Two divergences, one of them
about complexity and one about types — and `TRAPS.md` opens by saying regressions
cluster "where syntax matches Clojure but semantics don't". ETHOS says to inherit
Clojure's surface rather than invent one, and that argument holds where the
operation is the same operation. Here it is not, so the name joins this project's
own family instead: `str-slice`, `str-scalar-slice`, `vec-slice`, all half-open,
all raising, all naming what they cut.

**4. Why not `splice`, which Q36 named first.** A `(splice v from to items)`
shaped for `split-line` and `join-prev` would allocate once where the `vec-slice`
composition allocates four times. It would also leave `take`/`drop` quadratic for
every other caller, which is most of the prelude — `sort` partitions with them.
The bet was that four linear copies beat one linear copy by a constant nobody
would notice, and the measurement below is that bet settled: `RET` went 252×
faster on a decision whose predicted win was 70×.

**Cost.** Core 6,616 → 6,637 of 7,500; `prelude.xs` 195 → 201, which is *longer*
rather than shorter because the comment explaining where clamping lives is bigger
than the loops it replaced.

Measured, release, 200 operations per sample with a per-size zero-iteration
baseline:

| buffer lines | `RET` before | `RET` after | |
|---:|---:|---:|---|
| 250 | 0.709 ms | 0.0106 ms | |
| 1,000 | 7.244 ms | 0.0288 ms | **252×** |
| 4,000 | did not finish | 0.0999 ms | |
| 16,000 | — | 0.3796 ms | |

Four times the buffer now costs 3.80× where it cost 10.2×. And `take` of half a
vector, which is what the general case buys:

| elements | `conj` loop | `vec-slice` | speedup |
|---:|---:|---:|---:|
| 250 | 151.8 µs | 1.56 µs | 97× |
| 4,000 | 28,781.2 µs | 16.87 µs | 1,706× |
| 16,000 | 436,816.9 µs | 64.32 µs | 6,792× |

3.81× per 4× elements against the loop's 15.2×.

**Open.** **Typing is now linear in the buffer**, which it was not when last
measured — `insert-str` calls `assoc` on the lines vector and that is a native
O(n) copy, invisible while the string path was quadratic and dominant. At 16,000
lines a keystroke is 0.127 ms, which is fine, and the shape is E-11's
copy-on-write not yet paying rather than anything this entry introduced. It is the
third time in three entries that removing the largest cost has exposed the next
one, and the pattern is worth more than any of the individual findings.

Q6/E-11 itself stays open: `map`, `filter`, `repeat` and `join` are still `conj`
loops, and `repeat` is quadratic enough that it cannot build the fixtures for
these benchmarks.

---

### ADR-054 — The language does not segment graphemes, and that is a decision

*(New, 2026-07-30. **Withdraws the grapheme clause of ADR-018.** Resolves Q35.
Prompted by an external review of the editor that assumed this was already
handled.)*

**Decision.** There are no grapheme operations and there will not be. The
**Unicode scalar value is the smallest addressable unit** in this language. A
cluster is however many scalars it happens to be, `str-scalar-slice` will hand a
caller half of one, and a program that needs clusters builds them itself out of
`str-scalars`.

**1. What this withdraws.** ADR-018 promised "separate explicit operations for
bytes, scalar values, and graphemes." Two of the three exist. The third is
withdrawn rather than left pending, because "not yet" and "no" are different
claims and only one of them is honest after fifty-three entries in which nothing
needed it.

**2. Why, and the cost is real.** Correctness is first in the ETHOS ordering, so
this needs a better reason than convenience, and the reason is scope. That same
document says the project is *"not for generality, ecosystem compatibility, or
stability"*. Grapheme segmentation is UAX #29 — a standard, a table, and a table
that changes with each Unicode revision.

The price is exact and worth naming rather than softening: **backspace over `café`
spelled `e` + U+0301 removes the accent and leaves `cafe`.** The glyph count does
not change, so it reads as though nothing happened. The editor in the corpus does
this today, and now does it faster than before.

**3. The dependency is the part that decides it.** `unicode-segmentation` is
pre-approved by name in ADR-014, so this is not a question of whether the crate
is allowed. It is that ADR-013 makes features gate *host capability and never
language semantics*, so a segmentation crate cannot be optional — string
operations are language semantics. It would be the first dependency the
**language** carries.

ADR-045 defended that emptiness while adding two host dependencies, and called it
"a fact two write-ups assert… checkable — `cargo tree --no-default-features`
shows nothing." Nothing checked it. It does now: `the_language_carries_no_dependencies`
asserts every manifest dependency is optional, because an argument resting on an
unasserted fact is precisely what `notes/the-corpus-as-an-oracle.md` is about.
The invariant this entry spends is one this entry also makes real.

**4. What replaces the operation.** The trap is in `TRAPS.md`, and the behaviour
is now pinned by assertions in `tests/lang/strings.xs` that exist to fail if
somebody adds segmentation without an entry. A refusal nothing tests is
indistinguishable from an omission, which is the state Q35 was filed to end.

**Rejected.** *Adding `unicode-segmentation`*, whose case is genuinely good: the
crate is pre-approved, ADR-018 promised the operation, and a user typing an
accented character into the editor sees text quietly corrupted. It loses to the
zero-dependency language on scope, and the entry would rather record that trade
than pretend it was one-sided.

*A hand-rolled partial.* ZWJ sequences, variation selectors and regional
indicators are table-free ranges and could be clustered in a few lines. Combining
marks cannot: Rust's standard library exposes no general-category data. The result
would fix emoji and leave accents broken while looking like a grapheme surface —
fuzzy Unicode, which is the exact shape `TRAPS.md` exists to catch. Worse than
nothing, because nothing is honest.

**Cost.** No lines. One promise withdrawn, one invariant asserted for the first
time, and a known-wrong backspace that is now known-wrong on purpose.

**Open.** Reversible, and cheaply: ADR-014's pre-approval stands, the surface
would be three functions mirroring the scalar family, and the trigger is a
program whose correctness a user would actually notice. The editor is close and
is not that program.

---

### ADR-055 — Mutation as rung 5, hand-listed and opt-in

*(New, 2026-07-31. Resolves Q18, which had been open since milestone 1 with the
rung itself never in doubt — only its shape. Adds `mutate.sh` and `just mutate`.)*

**Decision.** A fifth rung: `just mutate`, a hand-listed set of mutations, each
breaking one load-bearing line and asserting the named test flips from pass to
fail. Not a framework, not part of `verify`.

**1. Why a list and not a tool.** Every general mutation tool answers "what
fraction of mutants die", which is a number nobody acts on. What has actually
paid here, eight milestones running, is a handful of mutations chosen because
someone believed a specific line was load-bearing. The value is in the choosing;
automating the choosing removes it.

**2. Three assertions, separately, because every way this rots is silent.** That
the edit changed the file; that the mutant still builds; that the test failed —
the last from the exit status, never from grepping output for a word.

`../reg-lisp` learned all three the hard way and had **twenty of its eighty-two
checks go quiet without it showing**, because the verdict came from a grep plus
an unconditional "a FAIL above is expected". It also found one dead check hiding
behind a live one under a shared description, which is why descriptions are
checked for duplicates here.

This entry earned its own first assertion immediately: the first run reported all
eight mutants as "does not build", because the build check matched `^error:` and
cargo prints `error: test failed` for exactly the outcome the script wants. A
harness that only said "no flip" would have sent someone looking at the mutants.

**3. Survivors may be declared, with a reason.** Two kinds are legitimate and
neither is a hole: a claim no test can separate — a *performance* claim is the
recorded example (Q18, milestone 6, erratum E-13) — and a guard against a failure
severe enough to enforce twice on purpose. The one declared survivor here is
`str-scalar-slice`'s `f <= t`, unreachable while the scan's early `break` stands,
and kept because reaching `s[f..t]` backwards is a **panic** where ADR-039
requires a throw.

Declaring is not excusing. **A declared survivor that starts dying is reported**,
because that means the reason stopped being true. And it must be written in
advance: milestones 4, 7 and 8 each predicted every survivor, and Q18 already
records why that matters — filled in afterwards they would have read as
discoveries.

**4. Opt-in, not in `verify`.** Every mutation is a rebuild. A gate measured in
minutes is a gate people learn to skip, and a skipped gate is worse than an
absent one because it still reads as coverage.

**5. What the rung finds, which nothing else does.** Q18 recorded four kinds over
eight milestones: a dead test; **duplicated enforcement**, where the fix is to
delete the redundancy rather than add a test; a defect that is real and
*unobservable*, where the honest response is to remove the way to write it; and a
hole in the corpus rather than a defect in the code, which scales worst because
no amount of strengthening a property fixes it.

The session that produced this entry adds a fifth: **a correct check that stops
covering its subject when a later change moves the ground under it.** Three
instances in one day — a two-row pin that stopped exercising the clamp it was
written for once the hint gave up its row, and two pins that silently began
testing soft wrap instead of clipping when wrap became the default. Each was
still passing, still correct, and no longer about anything. The mitigation is
narrow: **a pin states the mode it assumes**, because a pin that does not is one
that stops pinning the moment the default moves.

**6. And the timing, which is the actionable part.** Of the six checks this
session caught testing the wrong thing, three were caught at the moment they were
written and three had been believed for days. The habit costs least when the
assertion is fresh and most after it has been green a while, which is exactly
backwards from when it feels necessary. `AGENTS.md` carries the rule; this rung
carries the ones worth re-running.

**Rejected.** *A general mutation-testing framework* — see part 1; also
Q18's own long-standing position. *Running it in `verify`* — part 4. *Recording
mutants as a coverage percentage* — a number that goes up is a number people
optimise, and the finding here has never once been the ratio.

**Cost.** `mutate.sh`, outside the line budget as tooling. Eight seeded
mutations: seven flip, one survives as declared.

**Open.** The seeded set covers `str-scalar-slice`, `vec-slice`, `take`, and four
of the editor's geometry rules. Everything ADR-046 through ADR-051 decided is
unmutated, and the four earlier milestone passes live only in their notes rather
than in a runnable list.

---

### ADR-056 — A prose budget, as a tripwire

*(New, 2026-08-01. Adds `the_documents_stay_within_the_prose_budget`. Does not
amend ADR-030, which stays the budget for `src/`.)*

**Decision.** The documents get a cap of **20,000 lines**, asserted by a test.
Everything under `docs/` counts — `.md` recursively, so `notes/` and `archive/`
are in scope, and `.html` at the top level. One exclusion: the `<style>` block of
each write-up, which **prints**, with its line count, on every run.

**The number is the assertion, and this is the part that differs from ADR-030.**
That entry sets a working target of 7,500 and puts the tripwire a thousand lines
past it, so that failing can never start an argument about forty lines. There is
no working target here and no noise band beneath the number. This is a cap
against getting carried away, not a size the documents are meant to approach. The
current total is the test's to report and not this entry's to state — E-17 —
and if it ever approaches the cap that will be because something went wrong
rather than because a budget was being spent down.

**Why.** Constraint #1 is a context-window constraint, and for fifty-five entries
it has only ever been enforced against `src/`. Measured on the day of this entry:
6,845 lines of language against 10,751 lines of markdown, of which `ADR.md` is
3,524 — **half the size of the language it describes**. `notes/budget.html` had
just finished establishing that the line budget has never once refused anything,
and the reason is now plain: the code was never the thing growing. A project that
rigorously measures the artifact that was never at risk is not measuring.

*The exclusion prints, for ADR-045's reason.* Thirteen copies of one stylesheet
are 4,596 lines that nobody holds in their head, and counting them would put the
total at 18,567 — a cap meant as a safety margin binding on the day it was
written. But an exclusion nobody can see is how a budget stops measuring
anything, so it is itemised per file beside the content count.

*No directory is excluded.* `archive/` is 2,575 frozen lines that will never
grow, and excluding it would be free today. It is not excluded, because the
moment a budget can be satisfied by moving a file it measures where files are
rather than how much there is.

**Cost.** `ADR.md` is the largest single file inside this budget, so entries like
this one spend it. That is intended rather than ironic: an entry that costs
nothing to write is one nobody weighs before writing.

The failure modes will be the ones `budget.html` already catalogues for the line
budget — a counting rule that changes meaning, a layer that arrives unbudgeted, an
exclusion that grows. The mitigations are the same three and they are already
here: state the rule in the entry, print the breakdown, print what was skipped.

**Rejected.** *Per-file or per-directory caps* — false precision, and the
rebalancing churn ADR-030 already refused. *Counting only the normative docs* —
`notes/` is where the write-ups come from and is growing faster than `docs/*.md`.
*A `just prose` target* — the test prints the report and `just verify` runs it,
which is exactly the arrangement the line budget already has.

---

### ADR-057 — The write-ups are frozen

*(New, 2026-08-01. Closes the series added between 2026-07-27 and 2026-08-01.
Does not amend ADR-056; the write-ups stay inside the prose budget.)*

**Decision.** The thirteen pages in `docs/*.html` are **frozen as of
2026-08-01**. They describe the initial development of this language —
milestones 1 to 10 and the sessions that followed — and they are not
maintained. Every count in them is true as of the freeze date, which each page
now states in its footer.

**A later phase is a new series, not an edit to this one.** Nothing here says
never write another; it says this set is finished, and that bolting a
fourteenth chapter onto it would be a different act from writing one.

**Why.** A write-up is a record of a period, not a view onto a repository. The
distinction is invisible while the period is the present and becomes the whole
question afterwards, because the failure mode is specific and cheap to reach: a
later session finds thirteen pages asserting "55 entries", notices the log now
holds more, and helpfully updates them — turning an accurate historical
document into an inaccurate current one, one edit at a time.

That failure was already in progress. Nine of the thirteen stated a decision
count in the present tense with no date anywhere on the page, and **ADR-056
falsified all nine on the day it landed**. A freeze date is what converts those
from wrong into right: undated, "55 entries" is false; dated, it is permanently
true. This is E-17's rule reaching the artifact E-17 could not see, and the
answer is the same one — the number is not the problem, the missing *as of* is.

*Why a date rather than removing the counts.* The counts are load-bearing: the
argument on most of these pages is quantitative, and a page that says "several
entries carry corrections" is worth less than one that says seventeen. Stamping
is cheaper than rewriting and keeps what the pages are for.

**Cost.** The pages will drift further from the repository every week, and
somebody arriving in a year will read numbers that no longer describe anything
current. Accepted, and it is the point rather than a side effect: they are dated
and they say so. The alternative — thirteen documents to keep synchronised with
a moving codebase, forever, by hand — is the maintenance burden this entry
exists to refuse.

`AGENTS.md` carries the instruction not to refresh them, because that is the
file a session reads before it starts being helpful.

**Rejected.** *Leaving them live* — nothing was updating them, so "live" meant
"undated and quietly wrong". *Removing the counts* — see above. *Excluding them
from the prose budget now that they are frozen* — they are 3,220 lines that will
not grow, so excluding them buys nothing, and ADR-056's argument against
directory exclusions applies to a set that has stopped moving exactly as it does
to one that has not.

---

## Errata

Factual corrections to entries whose **decision still stands**. A wrong reason is
worth fixing even when it reaches the right conclusion, because the reason is
what gets reused. Superseding an entry over a mistaken sentence would be
ceremony; errata are the cheaper mechanism, and the affected entry carries a
pointer line. Where a correction changes a decision, it gets an ADR instead.

**E-17 — E-16, a number that should not have been written down.** That erratum
corrects ADR-055's "eight mutations" to "eighteen", and was stale within the day:
the set is larger again, and will be larger still. Both entries state an
inventory in prose, which guarantees a correction per sitting and teaches nobody
anything. **`mutate.sh` is the count**; neither number below should be read as
current, and no further erratum will be filed to move one.

What is worth keeping from the seeding is the *shape* of the results rather than
the total. Milestone 8's ten found one hole and confirmed nine — including all
four that survived its original pass, which had been closed by adding programs,
so the rung is now guarding those programs against removal. Milestone 5's five
found **none**, which is the first pass where nothing was wrong and is worth
recording as such: the fixes from that pass have held, including the two that
were corpus additions rather than code changes.

**E-16 — ADR-055, the seeded set and what seeding it found.** The entry's Cost
clause says eight mutations, seven flipping, and its Open clause says
"everything ADR-046 through ADR-051 decided is unmutated". Both were true when
written and neither is now: the set is **eighteen**, seventeen flipping and one
surviving as declared, and it reaches falsiness, float equality, `conj` order,
`str-scalar-len`, `str-index-of`'s `from` offset, merge stability, `take`/`drop`,
`vec-slice`'s return type, the gensym reset, and `emit`'s origin. The decision
stands unchanged; only the inventory moved.

Worth recording rather than quietly updating, because seeding it paid
immediately and in the way ADR-055 argues for. A mutation making `vec-slice`
return a **list** instead of a vector survived the entire suite — including three
assertions written the day before whose comment claimed "a list in, a vector out
— the conversion is the point". ADR-041 makes `=` cross representations, so
`(is= [1 2] ...)` is equally true of a list and could not see the thing it said
it was checking. The same hole covered `take` and `drop`, whose contract is that
they hand back a vector.

Fixed by asserting through `str`, which prints `[1 2]` for a vector and `(1 2)`
for a list. This is the fifth kind ADR-055 names — a correct assertion about the
wrong subject — found by the rung the entry created, one day after the tests were
written and by nothing else in the loop.

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

**E-10 — ADR-031, which step this is.** The entry calls the library/driver split
"the second step of ADR-015's stated progression." It is the third. ADR-015's
progression is *one file → one crate with file modules → library + binaries →
workspace*, and the split went from the first straight to the third: `lib.rs`
still holds inline `mod` blocks, so the file-modules step was skipped, not
taken. The decision stands and the reasoning is unaffected — the trigger was the
test boundary, not file size, which is exactly why the intermediate step was not
the one that helped.

The original design conversation (`archive/lispy-language-vm-convo-2026-07-25.md`)
lists a five-stage version with *one crate with inline modules* as its own step;
ADR-015 compressed that to four. Under either count, file modules were skipped.

**E-11 — ADR-033, the rest-argument deviation.** Two corrections to an entry
whose decision stands.

*The reversibility claim is wrong.* The entry says "widening to accept `nil`
later is safe." It is not a widening — a rest parameter is either always a list
or `nil`-when-empty, and swapping between them breaks code in both directions.
Under the empty-list rule a body may call `(count more)` unguarded; switch to
`nil` and that call needs a `nil`-tolerant `count`. Under the `nil` rule a body
may test `(if more ...)`; switch to the empty list and the test is always true.
Neither direction is free.

*What is actually safe* is a house idiom plus one library property: make the
core functions `nil`-tolerant — `(count nil)` → 0, `(empty? nil)` → true, as
Clojure has them — and write `(empty? more)` rather than `(if more ...)` or
`(nil? more)`. Code written that way behaves identically under either rule, so
the choice stops being a semantic fork. Milestone 6 owns delivering that
tolerance; the idiom applies from the first variadic function written.

*The load-bearing reason is stronger than the one given.* The entry argues from
one parameter having one type. The better argument is that Clojure's `nil` rest
argument is one instance of **nil-punning**, where empty and absent deliberately
collapse — `(seq [])` → `nil`, `(next [1])` → `nil`. That is coherent inside a
language built on lazy seqs. ADR-012 already declined laziness, so there is no
`seq` and no seq abstraction for nil-punning to inhabit; adopting `nil` here
would import a single artifact of a system this language does not have. Taking
it would mean adopting nil-punning as a policy, not as one calling convention.

*Also considered and not taken:* an empty **vector** rather than an empty list.
Nearly free today, since `ListObj` and `VecObj` are both `Vec<Value>` and
arguments already occupy contiguous slots. Declined because it would quietly
constrain Q20's still-open cross-type sequential equality, which currently
answers `false` for list-versus-vector.

**E-15 — E-12, the duplication no longer reaches a snapshot.** That erratum
closes by saying the `finally` proto duplication "matters because `Chunk.protos`
is what an `Image` serializes (ADR-029), so the duplication reaches the snapshot
and not only the compiler's output." True when written and false since ADR-043
part 6: an `Image` carries a *chunk fingerprint*, and `restore` takes the chunk
from its caller. No `Chunk` is serialized at all, so the duplication reaches the
compiler's output and stops there.

E-12's substance is unaffected — the proto table does double under nested
`finally`, and that is still a size statement rather than a correctness one.
What is wrong is the consequence, and the reason it went stale is worth more
than the correction: ADR-043 changed what an `Image` contains, and nothing
walked the errata to see what had been asserted about it.

**E-14 — ADR-043, the two-pass decode that is not needed.** The cost clause
says "the decode is now two passes rather than one for every object kind,"
because preserving sharing means an object can name an id it has not reached
yet. Written before the encoder was: it is not true, and the reason is worth
keeping.

The encoder builds an object's *children before it pushes the object itself*,
so every id an object names is strictly lower than its own. The object table
comes out topologically ordered by construction, and the decoder is a single
forward loop over it with no placeholder, no fixup, and no forward reference to
resolve. Preserving sharing cost a pointer-keyed map on the encode side and
nothing at all on the decode side.

The forward-reference worry was real for the *other* shape — reserving an id
before building, which is what a cycle would force. There is no cycle here: the
only cycle this system can construct runs through a cell, and a cell is an
arena id in a `Ref` rather than an edge in the object graph, which is the thing
ADR-029 made ids for. Ordering children first is available precisely because
the graph is acyclic, and the entry's own argument for ids is what guarantees
that.

The decision stands unchanged. What is wrong is a cost that was predicted and
never paid.

**E-13 — ADR-041, the transient win that is not there yet.** The *Why* clause
says `Rc::make_mut` "removes exactly that case — a fresh accumulator has one
reference, so the loop mutates a buffer it already owns". Measured, it never
does. At a native call the collection is live in the caller's argument window
*and* in the clone the primitive takes, so the strong count is at least two and
`make_mut` copies every time; in a `conj` loop the accumulator is additionally
still bound to a live local. Instrumenting `conj` over a five-element build
printed `copied=true` on every iteration, and replacing `make_mut` with an
unconditional clone survived the entire suite — the two are indistinguishable
today.

The decision stands: flat `Vec`, copy-on-write, no transients is still the right
shape, and `make_mut` is still the correct way to write it. What is wrong is the
claim that it already pays. The O(n²) Q6 named is still there, and removing it
needs the compiler to kill a slot on its last use (ADR-006's optional reuse
pass, extended) or a call protocol where a primitive consumes its arguments.
Both are pre-registered experiments under ADR-021, and neither is a collections
question. The one win that *is* real: a multi-pair `assoc` or multi-item `conj`
copies once rather than once per pair, because the `&mut` is taken outside the
loop.

**E-12 — ADR-034, what `finally` duplicates.** The cost clause says "`finally`
duplicates 2^depth under nesting" and means the code array. The proto table
doubles with it: a `fn` literal inside a cleanup is lowered twice and occupies
two entries in `Chunk.protos`, identical apart from their capture slots. The
per-copy capture slots are correct — each copy reads the cleanup's own bindings
from its own scratch slots — so this is a size statement, not a correctness one.
It matters because `Chunk.protos` is what an `Image` serializes (ADR-029), so
the duplication reaches the snapshot and not only the compiler's output.
Measured at milestone 2 on
`(try (a) (finally (let [z (q)] (fn [] z))))`, which yields protos 1 and 2.
