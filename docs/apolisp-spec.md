# lispylang — Specification & Architecture Decision Record

**Status:** Draft v0.1 · working name, not final
**Host language:** Rust
**Primary user:** one, with a collaborator; others may use it, but adoption is not a design input

---

## Part I — Charter

### Purpose

A small Lisp in the Clojure dialect, with its own VM, built as a substrate for
writing terminal applications, web services and APIs, and simulators. The
implementation is optimized for legibility, experimentation, and interactive
development — not for generality, ecosystem compatibility, or long-term
stability.

### The four governing constraints

1. **Legibility budget.** The entire core must be holdable at once — by one
   engineer and by one language model. This is a context-window constraint, not
   an aesthetic one, and it is the binding constraint on every other decision.
2. **Serializable machine state.** VM state is plain data. A running computation
   can be suspended, serialized, moved, and resumed. This is cheap if decided on
   day one and effectively impossible to retrofit.
3. **Performance punching above its weight.** Fast relative to implementation
   size, achieved through concrete representations and good constants — never
   through JIT, type inference, or speculative optimization.
4. **Correctness > simplicity > efficiency > scale.** When the four conflict,
   this is the order.

### Priority order

When constraints conflict:

1. Small, readable line count
2. Serializable / migratable state
3. Raw execution speed
4. Language ergonomics

**This ranking is directional, not a scoring function.** It exists to break ties
and to name what gets sacrificed under pressure. It is not a licence to ship
something unusable, and "ergonomics last" means *inherit Clojure's surface rather
than invent one* — it does not mean bad errors. Error quality is not ergonomics;
it is the feedback loop that everything else depends on.

### Non-goals

Third-party users · stable APIs · backward compatibility · package management ·
version negotiation · plugin architectures · generic compiler frameworks ·
"everything is extensible" · self-hosting · JIT compilation · multithreaded
execution within one VM.

These are costs justified only by multiple independent users. The language may
break on a Tuesday.

### Success criteria

- The core implementation fits in a modern LLM context window and stays there.
- Every compilation and execution stage is printable: forms, expansions, IR,
  bytecode, frames, values, handles.
- A suspended computation round-trips through serialization and resumes
  correctly.
- Any runtime value can be traced from source to VM slot without navigating an
  abstraction hierarchy.
- A REPL session is the primary development interface, not an afterthought.

### Line budget

| Layer | Target |
|---|---:|
| Reader, forms, metadata | 600 |
| Macro expansion | 600 |
| Core AST, lowering, bytecode compiler | 1,100 |
| VM: frames, calls, closures, errors | 1,100 |
| Values, collections, strings, bytes | 900 |
| Host handle table + blocking I/O | 500 |
| REPL, disassembler, diagnostics | 500 |
| **Total core** | **~5,200** |

Tests, host adapters (HTTP, terminal, JSON), and tooling live **outside** this
budget. The boundary is the point: *substantial host capability is a Rust
library behind the handle table, not a language subsystem.*

---

## Part II — Architecture

### Pipeline

```
source
  → reader                    (open, file-scoped configuration)
  → forms + source metadata
  → macro expansion           (open)
  → closed core AST           (~12 forms)
  → slot bytecode
  → explicit-frame VM         (serializable)
  → host handle table         (Rust on the far side)
```

Each arrow is a phase boundary and each phase boundary is printable. Abstraction
lives *at* these boundaries and nowhere else; inside a phase, the code is
concrete enums, `match`, and explicit control flow.

### Three layers of extensibility

| Layer | Openness | Rationale |
|---|---|---|
| Reader | **Open**, file-scoped | All surface-syntax weirdness lives here |
| Macros | **Open** | Primary semantic extension seam |
| Core forms | **Closed**, ~12 | Read the compiler, know the language |

Macros may introduce arbitrary syntax and may change *when* code runs (a `go`
block is a macro). Macros may **not** add new primitive operations to the VM.
That is the actual line, and it is the one that keeps the kernel inspectable.

### Core forms

```
literal   local   global   if    do    let   fn
call      quote   set-cell!   throw   try
```

`loop`/`recur` is admitted only if it cannot be expressed cleanly as a macro over
these — decide with a real attempt, not in advance.

### Where mutation lives

Three layers, named explicitly so that "immutable locals" is not mistaken for
"immutable language":

1. **Lexical values** — immutable slots, flat captured environments. No cells,
   no interior mutability, no `RefCell`.
2. **Language identity cells** — explicit heap objects: atoms, globals,
   recursive bindings, hot redefinition. Mutation here is visible in the source.
3. **VM-owned state** — modules, intern tables, host handles, caches, scheduler.
   Reached only through `&mut Vm`.

The VM owns all mutable runtime state. Nothing in the core uses
`Rc<RefCell<_>>`.

### Values

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

Construction goes through constructor functions so the representation can change
without touching the compiler. Size is **asserted, not assumed** — `Rc<str>` and
`Rc<[T]>` are fat pointers and will push `Value` to 24 bytes. 24 is acceptable;
representation tricks before profiling are not.

### Closures

Flat. Captures are copied into an `Rc<[Value]>` at closure creation — no open
upvalues, no closing, no aliasing. This is the single largest simplification the
language buys with immutable locals.

**The recursion exception.** Mutually recursive `let` bindings cannot be captured
flat, because each closure must reference the other before either environment
exists. Handle it with a dedicated mechanism:

- self-recursion resolves through the function's own identity, not a capture;
- mutual recursion lowers to explicit identity cells, initialized once;
- v1 may restrict mutual recursion to module-level bindings.

Do **not** let this corner push all environments into `RefCell`.

### VM

Single-threaded. Slot-based bytecode with numbered operands. Explicit frame
stack in VM-owned memory — the interpreter loop never recurses into itself, so
the Rust stack is empty at every instruction boundary.

```
CONST   r0, 10
CONST   r1, 20
ADD     r2, r0, r1
RETURN  r2
```

Slots are allocated monotonically per function with no reuse. Last-use reuse is a
later, optional pass. There is no register allocator in v1.

Machine state is exactly:

```
(constants, code, frame stack, value slots, cell heap, handle table, intern table)
```

Every element is serializable except the handle table, which serializes as
identity plus reacquisition intent.

### Host interface

All host interaction goes through opaque handles. Language code never
manipulates a Rust object directly.

```clojure
(io/open path :read)   ; → Handle
(io/read h buf)
(io/close h)
```

The VM owns the handle table, lifetimes, foreign calls, and cleanup. Handles are
generational, so a stale handle is an error rather than a silent alias.

Resource lifetime is **explicit**: `with-open` lowering to `try`/`finally`, plus
idempotent `close`. Refcount destruction is not a resource-management contract.

### Async is migration

The task-suspension mechanism and the migration mechanism are the same
mechanism, and building either one twice is the failure mode to avoid.

```
host call returns:  Ready(v) | Pending(handle) | Error(v)
task states:        Running → Waiting(h) → Runnable → Completed | Failed
```

On `Pending`, the current task's frames are already plain data, so suspension is
a move, not a capture. The host event loop marks readiness and reschedules.
Serialize the same structure to disk or a socket and you have migration. No
Asyncify, no stack-switching proposal, no platform dependency.

v1 ships blocking I/O only. The frame representation must nonetheless be
suspension-ready from the first commit.

### Strings and I/O

```
String    immutable UTF-8, Rc-backed, byte-indexed
Bytes     immutable byte sequence
Buffer    mutable byte buffer, VM- or host-owned
```

Strings are not generic sequences. No promise of O(1) character indexing.
Separate explicit operations for bytes, scalar values, and graphemes. A builder
for repeated concatenation.

Text/bytes conversion is always explicit at the I/O boundary. File and network
APIs speak bytes; `read-text` exists but has defined UTF-8 error behavior.

### WebAssembly

Ship the VM as a wasm module with values in linear memory. Do **not** target
WasmGC — it requires compiling the language directly to wasm, which puts frames
back on the wasm stack and forfeits constraint #2. Refcounting in linear memory
suffices, because immutable locals mean the only cycle risk is explicit cells.

Offer a "null collector" mode: never reclaim, for short-lived invocations.

---

## Part III — Architecture Decision Records

Each ADR is: the decision, why, what it costs, and what was rejected. Anything
here is settled law for implementers and must not be re-litigated mid-loop.
Changing an ADR is a deliberate act with a version bump, not a judgment call
inside a task.

### ADR-001 — Rust as the host language

**Decision.** Rust, single-threaded core, no async runtime in the VM.

**Why.** Small wasm binaries (~200KB vs ~2MB for Go), which matters because
distribution is how the thing gets used at all. Concrete enums and `match` are
both the fastest and the most legible construct available. No GC to inherit means
no GC to explain.

**Cost.** ~30% more lines than Go. Borrow-checker friction in the compiler, which
is mitigated by ADR-004 (VM owns everything, pass `&mut Vm`).

**Rejected.** *Go* — smaller line count and a free GC, but fat wasm binaries and
TinyGo drags in Asyncify. *OCaml* — excellent for compilers, but outside the
working toolchain.

### ADR-002 — Immutable locals, flat closures

**Decision.** Locals are immutable. Closures copy captures into `Rc<[Value]>` at
creation.

**Why.** Deletes the entire upvalue apparatus — open/closed lists, sorted
tracking, close-on-scope-exit — that a mutable-locals language requires. This is
the largest single simplification available, and it comes from a *language*
decision rather than an implementation trick.

**Cost.** Mutual recursion needs an explicit mechanism (see Closures, above).
Capture-heavy code allocates slightly more.

**Rejected.** Lua-style upvalues; boxed mutable environments.

### ADR-003 — Reference counting, no tracing GC

**Decision.** `Rc` throughout. No collector.

**Why.** Immutable locals mean the only cycle risk is explicit cells. A tracing
GC is 500–2,000 lines that touches every subsystem and is the main reason VMs
become unreadable.

**Cost.** Cycles through cells leak. Refcount traffic on hot paths.

**Mitigation.** `Weak` for known-cyclic patterns, or accept the leak — a
single-user language may leak knowingly.

**Rejected.** Mark-sweep; WasmGC (see Architecture).

### ADR-004 — Explicit frame stack; the interpreter never recurses

**Decision.** Calls push frames into a VM-owned `Vec`. The dispatch loop is flat.
The Rust stack is empty at every instruction boundary.

**Why.** This single decision yields: serialization, migration, coroutines,
generators, stack-depth limits, step limits, inspectable backtraces, and
independence from Asyncify — all from one piece of work.

**Cost.** Macroexpansion and any host callback that re-enters the VM must
trampoline rather than recurse. Bounded and worth it.

**Rejected.** Recursive `eval`; host-stack-based calls with Asyncify or JSPI for
suspension.

### ADR-005 — Serializable machine state is a first-class property

**Decision.** VM state is plain data with a canonical encoding. Suspension,
async, and migration are one mechanism.

**Why.** This is priority #2 and it silently disappeared from the earlier draft
spec. Naming it as a property constrains things that are otherwise left open:
whether handles may appear in serialized state (they may, as identity +
reacquisition intent), whether the intern table is part of a snapshot (it is),
whether `Value` has a canonical encoding (it must).

**Cost.** Host handles cannot be embedded as raw pointers. Every host adapter
must define its reacquisition semantics or explicitly refuse to migrate.

**Verification.** Round-trip is a property test, not a hope. See Part IV.

### ADR-006 — Slot-based bytecode, no register allocator

**Decision.** Numbered operand slots per function, allocated monotonically. No
liveness analysis, no reuse in v1.

**Why.** Captures most of a register VM's dispatch advantage over a stack VM
without the lowering complexity, wider encoding, or the class of compiler bugs
that liveness and moves introduce.

**Cost.** Larger frames than necessary. Fixed later by an optional last-use
reuse pass that changes no semantics.

**Rejected.** Stack VM (more dispatches, though simpler); full register
allocation (premature).

### ADR-007 — Closed core, open reader, open macros

**Decision.** ~12 special forms, fixed. Reader and macro system fully open.

**Why.** The kernel is the thing that must be readable to understand the
language. Every experiment worth running fits in the reader or a macro; `go`
blocks are a CPS-transforming macro, which is proof of how much headroom that is.

**Cost.** Some experiments will feel like they *want* a new special form. Treat
that feeling as a signal to look harder at the macro, not as grounds for
amendment.

**Rejected.** An extensible special-form registry — it blocks inline caching and
makes the compiler no longer authoritative about the language.

### ADR-008 — Reader configuration is file-scoped and declared

**Decision.** Reader macros are registered per file, declared near the top, and
frozen for the duration of that file's parse.

**Why.** A globally mutable reader table makes a file unparseable without
replaying the reader history that preceded it. That directly contradicts
constraint #1 — you can no longer hold the system at once, because the system now
includes its own history. This is the Racket `#lang` shape.

**Cost.** Cannot mutate the reader mid-file. Acceptable.

**Rejected.** Runtime-global mutable reader table (earlier position; withdrawn).

### ADR-009 — Metadata on forms from day one

**Decision.** Every form carries metadata. Source spans are populated by the
reader and preserved through macroexpansion.

**Why.** Two independent requirements ride on the same mechanism: usable error
messages (which are the development feedback loop, not a nicety) and
namespace-qualified symbols for macro hygiene. Cheap to build in, miserable to
retrofit.

**Cost.** Memory per form; a little plumbing in every reader path.

### ADR-010 — `Value` is a concrete enum; its size is asserted

**Decision.** Plain Rust enum, `Rc` payloads, no NaN-boxing. A test asserts
`size_of::<Value>() <= 24`.

**Why.** Legible, no allocation for primitives, and the compiler can see the
whole state space. NaN-boxing is a legibility disaster for a modest win.

**Cost.** 24 bytes rather than 16, because `Rc<str>` and `Rc<[T]>` are fat
pointers. Accepted.

**Rejected.** NaN-boxing; thin-pointer wrappers with an extra indirection.

### ADR-011 — One representation per collection type in v1

**Decision.** One implementation each for list, vector, map. Construction behind
constructor functions.

**Why.** Tiered small/large representations are a real win eventually and pure
speculation now. The constructors preserve the seam at zero cost.

**Rejected.** HAMT + RRB from the start (~1,200 lines, the single largest chunk
of a Clojure-alike, unjustified before workloads exist).

### ADR-012 — Eager sequences; no laziness

**Decision.** No lazy seqs. Transducers for composition.

**Why.** Laziness is Clojure's most expensive elegance: it infects every
collection operation, forces chunking hacks, wrecks locality, and moves errors
far from their cause. Transducers deliver the composition benefit and are faster.

**Cost.** Infinite sequences require explicit generators. With ADR-004 those are
nearly free.

### ADR-013 — Language features are unconditional; only host adapters are gated

**Decision.** Cargo features gate host capability — terminal, HTTP, JSON,
filesystem, RNG. They do **not** gate macros, metadata, or exceptions.
Authority reduction happens at runtime via the host registry.

**Why.** Feature-gating core semantics turns one system into 2ⁿ systems, which is
a direct assault on constraint #1. The VM knows exactly one generic `HostCall`
opcode; which host functions exist is a registry question, not a compilation
question.

**Cost.** Minimum binary carries the full language even when an app uses little
of it. Acceptable — the language is the small part.

**Rejected.** `#[cfg(feature = "macros")]`; per-profile language dialects.

### ADR-014 — Delegate standardized machinery, own everything semantic

**Decision.** Initial dependencies: `slotmap` (generational handle table),
`unicode-segmentation`, `crossterm`, `serde_json`. Later, as subsystems appear:
`bytes`, `miette`, `rand`.

**Why.** The test is *"would implementing this teach us something about our
language, or merely reproduce a protocol or standard algorithm?"* Unicode
segmentation, terminal portability, and JSON edge cases are the least readable
and most failure-prone lines we would otherwise write.

**Owned outright, never delegated:** form representation and metadata,
reader-macro dispatch, macro expansion, core AST, closure capture, bytecode and
compiler, instruction dispatch, frame and task representation, error semantics,
collection semantics, `Value`, and every host-boundary conversion.

**Rejected.** `logos` for tokenization — a Lisp tokenizer is ~150 lines and a
generated lexer fights character-level reader-macro dispatch. `hashbrown` — no
measured need at this scale.

### ADR-015 — One crate, shallow module layout

**Decision.**

```
src/  value.rs  form.rs  reader.rs  expand.rs  compile.rs
      bytecode.rs  vm.rs  host.rs  error.rs  main.rs
```

Start with inline `mod` blocks in one file if that is faster; the paths survive
extraction into files unchanged.

**Why.** 6–10 substantial files beats both a monolith and fifty fragments. Each
execution path stays mostly within one file, so following a value does not
require navigating a tree.

**Progression.** one file → one crate, inline modules → one crate, file modules →
library + binaries → workspace *only* where deployment boundaries justify it.

**Note.** Consider a generated amalgamated `lispylang_all.rs` reading view for
the context-window use case. Generated, not authoritative — the module system
stays the source of truth.

---

## Part IV — Development Process

The project is run as a human–agent loop campaign, not as a sequence of tasks.
Values when they conflict: **correctness > simplicity > efficiency > scale.**

Archetype: *sprawling experiment* early, becoming *personal dev tool* once the
REPL runs. Mode: **incremental**, framed by exit conditions rather than a
definition of "done." Loop count: **1 serial**, rising to 2 on disjoint file
sets only after a clean pilot.

### Phase 0 — Frame

Three documents are written and adversarially reviewed *before* any
implementation code. Each gets one fresh context told to find conflicts and gaps.

| Doc | Contents |
|---|---|
| `SPEC.md` | This document. Parts I–III are law. |
| `GUIDE.md` | Rust idioms for this codebase; what "good" looks like here; the semantic-trap checklist below |
| `RULES.md` | Agent hygiene and reviewer rejection rules, verbatim from the loop methodology |

`DECISIONS.tsv` is deferred. It earns its place once there is a per-module queue
with rulings worth pinning; before that it is ceremony.

**Exit condition, stated up front:** each iteration produces a runnable artifact
plus a one-paragraph result note. Stop a work stream when three consecutive
iterations produce no new signal.

### Phase 1 — The oracle

**This project has an unusually good oracle available, and it comes free from a
design requirement.** Constraint #1 already demands that every phase be
printable. A printable phase is a snapshot-testable phase. Inspectability and
verifiability are the same feature — build the printers first and the oracle
mostly exists.

Climb in this order:

**Rung 1 — it typechecks.** `cargo check`. Always on, free.

**Rung 2 — it runs.** `smoke.sh`: read, expand, compile, and execute a hello
program end to end; exit nonzero on failure. Write this before the reader is
finished — a failing smoke test is a better queue than an empty one.

**Rung 3 — behavior is pinned (golden files).** A corpus of `.xs` programs, each
with four committed snapshots:

```
tests/corpus/<name>.xs
tests/corpus/<name>.forms      # reader output, printed
tests/corpus/<name>.expanded   # post-macroexpansion forms
tests/corpus/<name>.disasm     # bytecode disassembly
tests/corpus/<name>.out        # stdout + final value
```

Four snapshots per program means a regression is localized to a phase before
anyone reads a diff. This is the single highest-leverage investment in the
project and it should exist by the end of week one.

**Rung 4 — behavior is specified.** A test suite written **in lispylang itself**
wherever possible, so it survives implementation churn and doubles as a
dogfooding pass. Keep it independent of the internals it tests.

**Property tests** — three, all of which pin a stated design property rather than
an implementation detail:

- *Reader round-trip.* `read(print(read(s))) == read(s)`. Catches metadata loss
  and printer/reader drift.
- *Serialization round-trip.* Suspend a running VM mid-computation, serialize,
  deserialize into a fresh VM, resume, and compare the final result and stdout
  against uninterrupted execution. **This is the oracle for constraint #2**, and
  without it that constraint is an aspiration rather than a property.
- *Differential testing.* For the subset that overlaps Clojure semantics, diff
  against Babashka. Useful early, discarded once the dialects intentionally
  diverge.

**Iron rule: the oracle is append-only during a campaign.** No golden file
regenerated to go green without a human reading the diff and saying why. An agent
that regenerates a snapshot to make a test pass has failed the task, and this is
an automatic reject.

**Determinism is a prerequisite for the oracle, not a nice-to-have.** Gensym
counters must be deterministic per compilation unit. Map iteration order must be
deterministic wherever it can reach output. Any nondeterminism makes golden files
flap, and flapping golden files get disabled, and then there is no oracle.

### Phase 2 — The loop

```
while task := queue.pop():
    result   = implement(task)               # 1 agent: task + SPEC + GUIDE + source
    findings = [review(diff), review(diff)]  # 2 agents: FRESH contexts, diff only
    apply(findings)                          # 1 agent: judges, applies what's real
    commit(narrow)                           # only files touched, attributed message
    verify(current_rung)
```

Reviewers receive the diff and none of the implementer's reasoning. Their job is
to enumerate reasons the change is broken — not to fix, praise, or approve.

**Mechanical queues for this project**, in rough order of use:

1. `cargo check` errors, grouped by file
2. The core-form list (12 items) — one loop iteration per form, reader → compiler → VM
3. The opcode list — one per instruction, with a disassembler case and a golden test
4. The builtin registry — one per host function, with its handle semantics
5. Failing golden snapshots, grouped by phase
6. The semantic-trap checklist below, run as a review pass over existing code
7. Fuzzer findings against the reader

"Make the VM faster" is not a queue. Profile, then enumerate the top ten sites as
one row each.

### Semantic-trap checklist (for `GUIDE.md`)

Regressions cluster where syntax matches but semantics don't. For this
project, that means:

- **Truthiness.** Only `nil` and `false` are falsy. `0`, `""`, and empty
  collections are truthy. Easy to get wrong in every conditional opcode.
- **Integer overflow.** Rust panics in debug and wraps in release. Decide the
  language semantics once — wrap, saturate, or throw — and implement it
  explicitly so debug and release agree. *Test the release build.*
- **Equality vs. identity.** `Rc` comparison is a pointer test. Language `=` is
  structural across collection types. Deriving `PartialEq` on `Value` will
  silently give the wrong answer.
- **Hash/equality agreement.** If `1` and `1.0` compare equal, they must hash
  equal — or decide they don't compare equal. Pick one, write it down.
- **Symbols vs. strings.** Symbols are interned and compare by id. Strings are
  not and compare by value. Mixing these is a whole bug class.
- **Serialization completeness.** A snapshot that omits the intern table resumes
  with wrong symbol identities and *appears to work*. Same for the cell heap and
  constants. This trap is silent, which is what makes it the dangerous one.
- **Handle validity across migration.** Generational keys catch reuse; they do
  not catch a handle that was valid in the source VM and meaningless in the
  target. Every adapter declares its reacquisition semantics or refuses.
- **Manual unwinding.** With an explicit frame stack, `try`/`finally` unwinding
  is hand-written. A Rust `?` early-return that skips frame cleanup leaks
  frames. Rust panics must never cross the VM loop.
- **Laziness assumptions.** Ported Clojure idioms may assume lazy evaluation.
  Eager `map` over an infinite generator hangs rather than erroring.

### Phase 3 — Pilot, then scale

Run the loop serially on **three** representative items — suggested: `if`, `let`,
and one host builtin — with a human reading every artifact. The pilot tests the
*loop*, not the items: are SPEC and GUIDE sufficient, do reviewers catch a
planted bug, is commit hygiene holding? Edit the docs until pilot output needs no
hand-fixing.

Scale only afterward, stopping at the first level that suffices:

1. Serial loop, unattended
2. Two loops, disjoint file sets (natural partition: `reader.rs` + `expand.rs`
   vs. `vm.rs` + `bytecode.rs`)
3. Separate worktrees — almost certainly unnecessary here

Parallelism multiplies mistakes exactly as fast as throughput.

### Phase 4 — Verification ladder

```
docs reviewed → pilot clean → implement → cargo check →
smoke passes → golden corpus green → in-language suite green →
serialization round-trip green → merge → soak → tag
```

**Merge ≠ release.** The soak is where leak checks, reader fuzzing, and
release-build divergence testing happen.

### Build order

Ship in this sequence; each milestone is a runnable artifact.

| # | Milestone | Exit condition |
|---:|---|---|
| 1 | Reader + printer + forms with metadata | Round-trip property test passes |
| 2 | Core AST + slot compiler + disassembler | Golden `.disasm` for hand-written forms |
| 3 | VM: frames, calls, closures, `if`/`let`/`fn` | `smoke.sh` runs a recursive function |
| 4 | Errors, `try`/`throw`, structured diagnostics | Failure cases in the golden corpus |
| 5 | Macro expansion + quasiquote + gensym | `defmacro` in-language; deterministic output |
| 6 | Collections, strings, bytes | In-language test suite begins |
| 7 | Host handle table + blocking file/stdio | `with-open` works; handles are generational |
| 8 | Serialization + suspend/resume | Round-trip property test passes |
| 9 | REPL | Becomes the primary development interface |
| 10 | Host adapters: terminal, TCP, JSON | Outside the line budget |

**Deferred until profiling names a specific pressure:** inline caches,
superinstructions, tiered collections, compact bytecode encoding, slot reuse,
alternative `Value` layouts, numeric specialization, async scheduling.

### The operator

The human does not write the code. The human:

- reads random diffs, reviewer findings, and commit messages continuously —
  anomalies (stub epidemics, over-justified workarounds, git weirdness) mean a
  rule is missing from `RULES.md`;
- **edits the loop, not the output** — one prompt or doc edit stops a failure
  class forever, one hand-fix stops it once;
- owns the gates: verifies by hand that the oracle actually ran, presses merge,
  decides when to tag.

Attention is front-loaded at Phase 0 and the pilot, sparse in the middle, dense
at gates.

---

## Part V — Open questions

1. **The name.** `lispylang` is a placeholder; `.xs` is the working extension.
2. **`loop`/`recur`** — attempt it as a macro first, admit it as a core form only
   on evidence.
3. **Multimethods** — likely the right dispatch mechanism (they subsume
   protocols and records at a cost recoverable via inline caching), but not
   scoped for v1.
4. **Overflow semantics** — wrap, saturate, or throw. Must be decided before
   milestone 3 and recorded as an ADR.
5. **Equality across numeric types** — whether `1` equals `1.0`. Decide with
   hashing in the same breath.
6. **Migration across versions** — whether a snapshot must survive a bytecode
   format change, or whether migration is same-build only. Same-build is far
   cheaper and probably right for v1.

---

*This document is law for implementers. Amendments are deliberate acts with a
version bump — never a judgment call inside a task.*
