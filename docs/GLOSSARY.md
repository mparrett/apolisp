# Glossary

Terms used across `ETHOS.md`, `ADR.md`, `QUESTIONS.md`, `BUILD.md`, and
`TRAPS.md`. Only entries that are ambiguous, overloaded, or specific to this
project — not a Lisp tutorial.

## The four that get confused at 1am

**form** vs **value**
A *form* is a piece of source after reading: the tree the reader produces, which
carries source metadata (spans) and is what macros consume and produce. A *value*
is a thing the VM computes with at runtime — the `Value` enum in ADR-025.

The confusion is legitimate, because in a Lisp they overlap: macros are ordinary
language functions, so a macro receives a form *as a runtime value*. **Settled:
they are the same type** — forms *are* `Value`s (ADR-023). What distinguishes a
form is not its representation but its company: a value being treated as code
travels with a span origin beside it (ADR-026). So "form" means *a value the
reader produced or the expander is treating as code*.

**snapshot** (two unrelated meanings)
1. *Golden-file snapshot* — a committed expected-output file in `tests/corpus/`.
   Testing sense (`BUILD.md`, rung 3).
2. *VM snapshot* — a serialized `Image`: one `Vm` plus one suspended
   `Execution` (ADR-029).

They meet in exactly one place: the serialization round-trip property test, which
compares a resumed VM snapshot's output against golden-file snapshots. Elsewhere,
disambiguate.

**atom** (two unrelated meanings)
1. *Clojure sense* — a mutable identity box. That is what ADR-020 layer 2 means
   by "atoms," and it is a `Cell` here.
2. *Classical Lisp sense* — any non-list value.

This project always means sense 1. Sense 2 does not appear in the docs and should
not start.

**slot** vs **register**
A *slot* is a numbered operand location in a call frame (ADR-006). Bytecode looks
register-like (`ADD r2, r0, r1`) and the literature calls this a register VM, but
there is no register allocator, no liveness analysis, and no reuse — slots are
handed out monotonically per function. Call them slots; "register" implies
machinery that does not exist.

---

## Pipeline

**reader** — Turns source text into forms. Not "parser": it is character-driven,
dispatches on reader macros, and produces data rather than a grammar-shaped tree.

**reader macro** — A character-triggered extension to the reader (`#foo`, custom
delimiters). Registered per file and frozen for that parse (ADR-008, Q1).

**printer** — Forms and values back to text. Paired with the reader by the
round-trip property test.

**macro** — Language-level code that runs at compile time, taking forms and
returning forms. Open-ended (ADR-007). May introduce syntax and may change *when*
code runs; may **not** add VM primitives.

**macroexpansion** — The phase that runs macros to fixpoint. Requires a live VM,
since macros are language code — which is why compilation is not a pure function
of source (ADR-004).

**capture avoidance** — Keeping a macro's introduced symbols from colliding with
the call site's. Rides on symbol *naming*: syntax-quote resolves to qualified
names at read time, and gensym supplies the rest (ADR-024). Deliberately *not*
called hygiene — Clojure-style resolution is weaker than `syntax-rules`, and a
macro author can still construct a capturing symbol on purpose (erratum E-7).

**span** — A source position: line, column, length. Reader-owned and
compiler-consumed; language code can neither read nor attach one (ADR-026).

**span origin** — Where a span came from: `Source` (read from a file),
`Generated` (built by a macro, carrying the call site), or `Unknown`. Origins
live *outside* the value graph, in a carrier the expander threads alongside code,
with one origin per syntactic child of every aggregate. Naming `Generated` and
`Unknown` is what keeps span loss visible instead of silent (ADR-026).

**lineinfo** — Lua's name for the other half: an array parallel to the bytecode
where `lines[i]` is the span of instruction *i*. What lets a runtime error or
backtrace report a position at all.

**auto-gensym** — `x#` inside a template: one fresh symbol per *template*, so
two occurrences of `x#` in one template are the same name and two templates
never collide. Lowered by the expander, not the reader (ADR-040). Per template
rather than per expansion, as in Clojure, because the template is lowered once —
when the macro is defined.

**prelude** — `src/prelude.xs`, compiled into the binary and expanded ahead of
every unit. Where `def` and `defmacro` live, in the language, over `set-global!`
and `set-macro!` (ADR-027, ADR-040).

**compilation unit** — One file, read and expanded together. What a macro's
scope and the gensym counter are relative to: macros do not survive a unit and
do not reach an `Image`.

**gensym** — A generated unique symbol, used for capture avoidance. Must be
deterministic per compilation unit or golden files flap (`BUILD.md`).

**quasiquote** — Templated form construction (`` ` ``, `~`, `~@`).

**core form** — One of the 13 closed special forms the compiler knows (ADR-007,
ADR-027).
"Special form" means the same thing; prefer "core form" for the closed set.

**core AST** — The closed representation after macroexpansion, made only of core
forms. Input to the compiler.

**lowering** — Translating a higher construct into core forms or bytecode
(`with-open` lowers to `try`/`finally`).

**`Proto`** — One compiled function: code, the parallel `lines` array, constants,
capture descriptors, parameter count, and slot count (ADR-034). A *prototype*
rather than a function because it holds no captured values — a `Closure` is a
`Proto` plus the values captured at the moment it was created.

**`Chunk`** — What compiling one file produces: a flat `Vec<Proto>` with
`protos[0]` the top level and every nested `fn` an index into it (ADR-034).

**`set-global!`** — The core create-or-rebind operation on the global table
(ADR-027, spelled by ADR-034). `def` is the library macro over it, and `def` is
what anyone will actually write.

**disassembler / `.disasm`** — Prints bytecode back as readable instructions. Both
a debugging tool and a golden-file phase.

**`.xs`** — The working source file extension. Name is Q14.

## Runtime

**VM** — The bytecode interpreter plus all mutable runtime state (ADR-020).
Single-threaded.

**frame** — One function activation: its slots, return address, and bookkeeping.
Lives in a VM-owned `Vec`, never on the Rust stack (ADR-004).

**frame stack** — That `Vec`. "Explicit frame stack" emphasizes the contrast with
a recursive `eval`, where activations would live on the Rust stack and could not
be serialized.

**trampoline** — Returning to the dispatch loop instead of recursing, so the Rust
stack stays empty. Required of macroexpansion and any host callback that re-enters
the VM.

**closure** — A function plus its captured environment. Here *flat*: captures are
copied into an `Rc<[Value]>` at creation (ADR-002).

**capture** — A value copied into a closure at creation. Copied, not referenced —
that is the whole simplification.

**upvalue** — Lua's mechanism for a capture that may still be mutated by an
enclosing scope, requiring open/closed tracking. Appears in these docs **only as
the thing ADR-002 deletes**.

**cell** — The one mutable language object. `Value::Cell(CellId)` is an index
into a VM-owned generational arena, not a pointer; reads and writes go through
`&mut Vm` (ADR-025). Backs atoms, globals, recursive bindings, and hot
redefinition. *Not* Rust's `RefCell` — the core uses none, and the name collision
is unfortunate.

**identity cell** — A cell used to give something a stable identity before its
value exists; how mutual recursion is resolved (ADR-002).

**task** — A unit of execution with its own frame stack, cycling `Running →
Waiting(h) → Runnable → Completed | Failed` (ADR-017). v1 has exactly one; the
scheduler arrives only when a nonblocking adapter does (ADR-029).

**suspend / resume** — Stopping execution at an instruction boundary and
continuing later. Because frames are plain data, this is a move, not a capture.

**fuel** — A budget of instructions. Exhaustion is the v1 suspension trigger, and
it is what lets a snapshot be taken at an *arbitrary* boundary rather than one
built for the purpose (ADR-029).

**Vm / Execution / Image** — The snapshot boundary. `Vm` is the durable
world (intern table, globals, cell and handle arenas, registry). `Execution` is
one running computation (frames, slots, pc, handler stack, status, fuel). An
`Image` is a serializable DTO for one of each — same build, fresh VM, no live
handles (ADR-029).

**migrate** — Serializing a suspended execution and resuming it elsewhere. The
same representation as suspension, but not the same capability: v1 stops at
same-build, fresh-VM resume with live handles refused (ADR-029).

**handler stack** — Where active `try` handlers and pending `finally` blocks
live: VM-owned, inside the `Execution` image, and discharged before a tail call
reuses a frame (ADR-028).

**fault** — A failure the VM raises rather than a program throwing it: an arity
mismatch, an overflow, an unbound global, a call to a non-function. Since
ADR-039 a fault *is* a throw — it unwinds identically and a `catch` binds it —
and what distinguishes it is only its value's shape,
`{:type :vm-error :kind K :message "..."}`.

**unwind** — A failure in flight, and the act of delivering it: dropping frames
down to the innermost handler record and entering that record. It carries three
things, because only the first is a language value — the thrown value, the
origin of the instruction that raised it, and the errors it displaced.

**suppressed** — An error that a later one displaced. A cleanup that throws
while unwinding wins, and the error it interrupted is retained on it (ADR-028
invariant 3). In v1 the chain reaches the transcript and nothing else: a `catch`
binds the winning value alone (ADR-039 clause 4).

## Host boundary

**host** — Rust on the far side of the handle table. Everything the language
cannot do itself: files, terminal, network, clocks, randomness.

**handle** — An opaque, generational reference to a host resource
(`Value::Handle`). Language code never touches a Rust object directly (ADR-016).

**generational** — A key carrying a generation counter, so a reused index does not
silently alias a dead resource. From `slotmap`.

**handle table** — The VM-owned map from handles to live host resources. The one
part of VM state that is not serializable — an `Image` containing a live handle
is refused outright in v1 (ADR-029).

**reacquisition** — What an adapter does to make a handle meaningful again after
a snapshot moves: reopen the file, redial the socket, or refuse. Every adapter
declares its semantics (ADR-005, ADR-016).

**host registry** — The runtime table of available host functions. Which functions
exist is a registry question, not a compile-time one (ADR-013).

**`HostCall`** — The single generic opcode for calling into the host. One opcode,
many registered functions.

## Data

**symbol** — An interned name; compares by `SymId`, not by text.

**CellId / HandleId / BufferId** — Generational arena indices, not pointers. A
stale one is a typed error rather than a silent alias, and they serialize as
integers, which is what makes an `Image` ordinary data.

**`SymId`** — A symbol's interned index. Meaningful only relative to the intern
table it came from, which is why snapshots carry that table (Q9).

**intern table** — The map between symbol text and `SymId`. VM-owned, part of a
VM snapshot; omitting it is a silent corruption (`TRAPS.md`).

**keyword** — A self-evaluating named constant (`:read`). Its own `Value`
variant, interned in the same table as symbols but distinct from them, so type
predicates and printing stay obvious (ADR-025).

**structural equality** — Language `=`: compares contents across collection types.
Distinct from `Rc` pointer equality, which is identity. Deriving `PartialEq` on
`Value` gives you the wrong one.

**truthiness** — Only `nil` and `false` are falsy. `0`, `""`, and empty
collections are truthy.

**String / Bytes / Buffer** — Immutable UTF-8 text; immutable byte sequence;
mutable byte buffer. Conversion between text and bytes is always explicit
(ADR-018).

**scalar value vs. grapheme vs. byte** — Three distinct indexing levels for text,
each with its own operations. No level gets to be "the" default.

**eager** — Collection operations compute fully and immediately. The opposite of
Clojure's lazy seqs, which ADR-012 rejects.

**transducer** — A composable transformation independent of the collection it runs
over. The replacement for laziness as a composition mechanism.

**transient** — A temporarily mutable version of a persistent collection, used to
make bulk construction linear instead of quadratic. Whether these exist is Q6.

**HAMT / RRB** — Hash Array Mapped Trie and Relaxed Radix Balanced tree: the
standard persistent map and vector implementations. Rejected for v1 as ~1,200
lines (ADR-011).

**fat pointer** — A pointer carrying a length alongside the address (`Rc<str>`,
`Rc<[T]>`), hence two words. Why `Value` is 24 bytes and not 16.

**NaN-boxing** — Packing pointers and tags into unused bits of a float. Rejected
for legibility (ADR-010).

## Verification

**oracle** — Anything that can tell you the code is wrong without a human reading
it. The four rungs in `BUILD.md`, collectively.

**rung** — One level of the oracle ladder, from `cargo check` up to an
in-language test suite. Climb in order.

**golden file** — A committed expected-output file. A change to one is a change to
behavior, and regenerating one to go green is a failed task, not a fix.

**corpus** — `tests/corpus/`: the set of `.xs` programs plus their four golden
files each (`.forms`, `.expanded`, `.disasm`, `.out`).

**smoke test** — `smoke.sh`. One program, end to end, nonzero on failure. Exists
before the reader is finished.

**property test** — A test that pins a stated design property rather than a
specific output. There are three (`BUILD.md`).

**round-trip** — Two of the property tests. *Reader* round-trip:
`read(print(read(s))) == read(s)`. *Serialization* round-trip: suspend, serialize,
resume, compare against uninterrupted execution.

**differential testing** — Diffing behavior against another implementation
(Babashka) on the overlapping subset. Scope is Q16.

**Babashka** — A fast-starting Clojure interpreter. Used only as a differential
oracle; not a compatibility target.

**soak** — Post-merge, pre-tag testing: leak checks, reader fuzzing, release-build
divergence. Merge is not release.

**flapping** — A golden file that changes without the code changing, usually from
nondeterminism. Flapping files get disabled, and disabled files mean no oracle.

**mutation check** — Deliberately breaking a load-bearing line to confirm a test
actually fails. Proves a test *can* fail, which a passing suite does not (Q18).

**pre-registration** — Writing down what you expect from a benchmark before
running it, then recording whether it was refuted (`BUILD.md`).

## Project vocabulary

**legibility budget** — Constraint #1: the whole core must be holdable at once by
one engineer and one language model. A context-window constraint, not taste.

**line budget** — The per-layer table in `BUILD.md` that operationalizes it.
~5,200 lines of core; tests, adapters, and tooling sit outside it.

**seam** — A boundary that exists so a subsystem can be **deleted or lifted out**,
never so code has a home. Inline `mod` blocks are the seams (ADR-015).

**subtraction test** — Cutting a host subsystem out and checking it still builds.
What keeps "seams for subtraction" a fact rather than a feeling (ADR-013).

**blast radius** — How far a change reaches and whether it is reversible. The gate
on optimization work, replacing "profile first" (ADR-021).

**ADR** — Architecture Decision Record: a settled decision with its reasoning,
cost, and rejected alternatives. Append-only (ADR-022).

**erratum** — A factual correction to an ADR whose decision still stands. Lives
in `ADR.md`'s Errata section; the entry gets a pointer line. A correction that
changes the decision gets a superseding ADR instead.

**supersede** — How an ADR changes: write a new entry that replaces it and mark
the old one. Never edit in place, never bump a version.

**the freedom clause** — No third-party users, so no compatibility contract. The
compensation for giving up adoption, not a risk to manage (`ETHOS.md`).

## Prior art referenced

**Asyncify / JSPI** — Two ways to suspend a computation whose frames live on the
wasm stack. Both unnecessary here, because ADR-004 keeps frames off it.

**WasmGC** — Wasm's managed-heap proposal. Rejected: using it means compiling the
language directly to wasm, which puts frames back on the wasm stack and forfeits
constraint #2 (ADR-019).

**linear memory** — Wasm's flat byte array, where this VM's values live instead.

**`#lang`** — Racket's per-file language declaration. The shape ADR-008 borrows.

**CPS** — Continuation-passing style. How a `go` block macro can change when code
runs without any VM support, which is the evidence for how much headroom macros
have (ADR-007).

**inline cache / superinstruction** — Standard interpreter optimizations: memoize
a dispatch at its call site; fuse a common opcode pair into one. Both are
zero-gate work under ADR-021.

**`Rc` / `Weak` / `Drop`** — Rust reference counting; a non-owning reference that
breaks cycles; the destructor hook. See ADR-003.
