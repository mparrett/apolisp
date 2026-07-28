# Prior art

Three sibling repos already tested large parts of this design space, with
measurements and post-mortems rather than opinions. This file records what
transfers, what refuses to transfer, and which of our ADRs each one bears on.

Order of relevance to us: **let-rs** (same language, opposite mutation
strategy), **reg-lisp** (same call protocol, measured), **wallisp** (same
questions, eight implementations, hypothesis-driven).

| Repo | Substrate | Shape | What it was arguing |
|---|---|---|---|
| `../wallisp` | C → wasm + native | 8 engines: tree-walker / CEK / bytecode × no-GC / mark-sweep / region / refcount | Which architecture and which collector actually cost what |
| `../reg-lisp` | Go | Register VM per *The Implementation of Lua 5.0*, built as 6 forked tiers | That a heap-resident call stack beats recursing into the host |
| `../let-rs` | Rust | CEK machine, workspace, zero-dep core | A readable small Lisp as a substrate for demos |

**On numbering.** All three keep their own ADR logs, and the numbers collide with
ours. A bare `ADR-004` here always means *ours*; a foreign one is always prefixed
with its repo (`let-rs ADR-004`).

---

## let-rs — the mutation warning, already run

**The single most valuable transfer in this file.** Our ADR-002 ends with: *"Do
not let this corner push all environments into `RefCell`."* let-rs is what
happens when it does.

**let-rs ADR-004 made every env slot `Rc<RefCell<Val>>`, and `letrec` is the
stated reason.** Not carelessness — a reasoned decision with alternatives
considered (Y-combinator desugaring, a separate `EnvRec` variant, two-pass
compile), each rejected for good cause. The recursion corner is the *default*
path to interior mutability, not an unlikely one.

**let-rs ADR-021 records what it cost: an unfixable leak.** The cycle is traced
node by node — `frame` holds `cell`, `cell` holds the closure, the closure's env
holds `frame`; every node has exactly one strong incoming reference and nothing
reaches zero. Three fixes were designed and all three still leave a two-node
cycle. The ADR's conclusion is that no clean fix exists without a substantially
more invasive engine change.

What that means for us, concretely: **ADR-003's "cycles through cells leak" is
not a corner case.** Under identity cells, every self-recursive closure stored in
its own cell is a two-node cycle. Every `defn` leaks, not just exotic code.

**The one shape that works.** let-rs ADR-015 broke the *globals* cycle by moving
globals into a Vm-owned table with a `Weak` back-edge — and it worked for exactly
one reason: the Vm's lifetime is strictly longer than every closure's, so there is
an unambiguous owner. That reason does not generalize to `letrec`, where the cells
must outlive nothing in particular.

This upgrades a hedge in our ADR-002 into a justified rule. "v1 may restrict
mutual recursion to module-level bindings" is not a convenience — module level is
*the only shape where the cycle has an owner*. → **Q17.**

**Also from let-rs:**

- **let-rs ADR-022 retrofitted source spans and calls the change "unavoidably wide — it
  touches the public API of `eval_str` and most internal error sites."** Direct
  confirmation of ADR-009's "miserable to retrofit." It also landed
  `Datum.span: Option<Span>` with macro-synthesized datums carrying `None`,
  which is precisely the silent-degradation trap in `TRAPS.md`.
- **let-rs ADR-020** put primitives in globals so `(define +)` overwrites them. Worth
  reading before we do inline caching, since redefinition is what an inline cache
  has to invalidate against.
- **let-rs ADR-031** added strings as the *ninth* `Val` variant, well after the enum
  looked settled. Evidence that `Value` grows, and an argument for asserting its
  size early (ADR-010) rather than after the fact.
- **let-rs ADR-023** designed a CESK migration and deferred it. Relevant if we ever
  want a store separate from the environment.
- The core crate is zero-dependency by decision (let-rs ADR-002), which is our
  ADR-014's delegate/own split reached from the other side.

**What does not transfer.** let-rs chose CEK, which wallisp measured as the
slowest of the three architectures. That is not a contradiction — let-rs
optimized for semantics you can read in one file (five rules in `step.rs`), and
we ranked speed above ergonomics. It is a good illustration that ADR-006 is a
priority statement, not a fact about VMs.

## reg-lisp — the call protocol, measured

Built as an argument that a small embedded VM is better served by Lua 5.0's
architecture than by one that recurses into the host stack once per call. Directly
our ADR-004.

**The numbers.** Deepest non-tail recursion surviving a clamped native stack,
same program on both sides:

| Native stack cap | reg-lisp | let-go VM | let-go AOT |
|---|---|---|---|
| 64 KB (TinyGo's wasm default) | 1,000,000 | 38 | 999 |
| 512 KB | 1,000,000 | 331 | 8,167 |

And on speed, a register VM ran **1.4–1.6× faster than let-go's stack-based
bytecode VM** across fib, tak, and a tail loop — modest, same-direction support
for ADR-006.

**The failure story is the part to keep.** In July 2026 the recursive-interpreter
shape blocked let-go's TinyGo/wasm port for three sessions. TinyGo's wasm target
falls back to a 64 KB stack, one host nesting per language call overflowed it, and
it wrote *past* the stack into adjacent static memory. **It presented as the
reader's macro map corrupting, not as a stack overflow.** Thresholds were clean
and monotonic: 64 KB failed at `fib(20)`, 128 KB at `fib(30)`, 256 KB fine.

That is ADR-004 and ADR-019 in one incident: the cost of frames-on-the-host-stack
is not a graceful depth limit, it is silent memory corruption that presents as an
unrelated subsystem failing. Worth a line in `TRAPS.md` on its own.

The AOT comparison sharpens it further — the same functions lowered to native Go
with unboxed ints buy a 25× better constant and nothing structural, still linear
in the cap. reg-lisp's phrasing: *"You can make each frame 25× cheaper; you can't
make it not a frame."* Compilation does not reach the call protocol.

**A gap in our design that reg-lisp already hit.** Tier 5 carries source positions
end to end, and the mechanism has two halves: `Node` carries `Line`/`Col` from the
reader, *and* `Proto.lines` is an `[]int` parallel to the bytecode, so instruction
*i* knows its source line. We have the first half (ADR-009) and **nothing for the
second**. Without it, runtime errors and tracebacks cannot report a position, and
our `.disasm` golden files have nowhere to show one. → folded into **ADR-023**.

**Two findings from that tier that bear on our oracle:**

- A completeness test found *every* function's `RETURN` attributed to line 0 —
  the kind of hole a corpus does not surface, because output still looks right.
- **A mutant that never restored the compiler's line counter passed the entire
  suite**, because every test program had its subexpressions on the same line as
  their enclosing form. The corpus was green and the mechanism was dead.

reg-lisp's answer is `./verify.sh mutate`, which deletes the load-bearing line and
shows the test flipping. Our iron rule keeps golden files honest about *changes*;
it says nothing about whether a test could ever fail. → **Q18.**

**Ordering advice we happen to already follow.** reg-lisp ADR-015 put source
positions before macros, on the grounds that *"macro-expanded code reporting
positions from the original form is a known-hard problem, and solving it after
expansion exists is strictly worse."* Our build order has metadata at milestone 1
and macros at milestone 5. Keep it that way.

**One cost datum for the option we rejected.** reg-lisp keeps `Node` (AST) and
runtime values as separate types, converting at the `eval` boundary — our Q2
option B′. reg-lisp's ADR-013 records that `valueToNode` had to become iterative on both
axes *and* cap nesting depth. That is the conversion boundary charging rent,
which is roughly what we predicted when rejecting B′.

## wallisp — eight engines, hypothesis-driven

Same tiny Lisp implemented eight ways in C, benchmarked on wasm and native, with
pre-registered predictions and a falsification log per experiment.

**Architecture is the lever; the collector is a rounding error by comparison.**
Bytecode ran 2.3–3.9× faster than the tree-walker across five benchmark shapes,
constant across program shapes. GC strategy moved things 0.94×–1.83×. Supports
ADR-006, and suggests our optimization attention belongs in dispatch, not
allocation.

**The finding that argues against us — H12: refcounting was the *slowest* GC
strategy tested**, ~1.1–1.25× worse than mark-sweep. The mechanism: mark-sweep is
lazy and barely fires when the arena is big enough, while refcounting pays
inc/dec eagerly on every reference. The penalty tracked *call volume*, not
allocation. And the line counts do not rescue it either — the refcount engine was
560 lines against mark-sweep's 596, so refcounting did not even buy simplicity.

ADR-003 justified `Rc` on line count ("a tracing GC is 500–2,000 lines"), and
wallisp got a working mark-sweep for ~146 lines over its no-GC baseline. Two
things blunt the transfer, and they are worth stating rather than waving at:

1. In C, every value is a cell in one arena, so tracing is easy. In safe Rust,
   tracing has to walk Rust-owned structures, which is where the 500–2,000 lines
   actually come from.
2. But ADR-004 gives us something wallisp's tree-walker had to build by hand: an
   explicit frame stack *is* a precise root set, enumerable for free. wallisp's
   `lisp_gc` needed a shadow stack at every `eval` entry to get what we get from
   the frame representation we already committed to.

Not enough to reverse ADR-003 tonight, and reversing it would touch every
subsystem — ADR-021 says that class of change needs an argument, and this is the
beginning of one rather than the whole of one. → **Q19.**

**Refcounting also forced a structural change.** wallisp's `lisp_rc` needed an
explicit `eval()` trampoline rather than C recursion, because refcounting must
release the tail-call frame *after* its body runs. We are trampolined already
(ADR-004), so we pay this one by default — but it is a real interaction between
refcounting and tail calls, and it lands on **Q4**.

**On tail calls (Q4), three usable facts:**

- The measured cost of TCO **flipped sign across toolchain versions** — +7% on
  one clang/V8 pair, −5% on a later one. Their standing advice: treat throughput
  deltas under 10% as "depends on which compiler you build with today."
- It was kept anyway, on the grounds that for a Lisp, proper tail calls are close
  to table stakes and a loop that dies at a fixed depth is a bad surprise.
- **"TCO fixes the stack, not the heap."** Frame reuse gives a constant call
  stack, but each iteration still allocated a frame that was never reclaimed, so
  a long tail loop died on arena exhaustion instead of stack exhaustion. Our
  `Rc` frames drop on reuse, so we get the heap half for free — one of the few
  places refcounting wins outright.

**Calibration for "punching above its weight."** wallisp's best engine sits 20.7×
off hand-written JS and 68× off `-O2` C on fib. On one benchmark the C baseline
folded the whole loop to closed form, which no interpreter can reach. Useful for
keeping constraint #3 honest about what the target actually is.

**Two methods worth stealing:**

- **Pre-registered predictions with a falsification log.** Write down what you
  expect *before* running the benchmark, then record whether it was refuted.
  Several of wallisp's headline findings are refutations of its own hypotheses —
  region-drop being *faster* than no-GC, refcounting's penalty tracking calls
  rather than allocation, the metacircular win having a different cause than
  predicted. Under ADR-021 we optimize without a gate; pre-registration is what
  turns that freedom into knowledge instead of folklore.
- **Honest empty cells.** Its engine grid marks two combinations "not built" and
  explains the invariant that makes them incoherent, rather than leaving a gap
  that reads as an oversight.

**One tagging trap for `TRAPS.md`:** wallisp's fixnums turned out to be 30-bit,
not 32-bit, and it surfaced only under a benchmark. If we ever tag integers, the
usable range is not the obvious one.

---

## Outside reading

Not a sibling repo, and recorded because it argues against a decision we made
rather than for one.

- **[Bytecode-to-source mapping](https://tidefield.dev/bytecode-to-source-mapping/)**
  — makes the case for `(offset, line)` pairs at run boundaries with binary
  search, over a parallel array indexed by instruction. Correct for line
  numbers and near-useless for spans: measured on our corpus, run-length over
  spans compresses 0.89 where run-length over lines compresses 0.24. It is the
  clearest statement of the cost ADR-023 point 2 chose to pay, which is why it
  is worth keeping the link. Filed as **Q30**.
- **[Rust and memory allocators](https://pranitha.dev/posts/rust-and-memory-allocators/)**
  — *filed unread, 2026-07-27; the site returns 403 to an automated fetch.*
  Recorded against **Q19** and **ADR-021**, which are where it would bear:
  Q19 reopened `Rc`-versus-tracing on `../wallisp`'s measurement that
  refcounting was the slowest of four strategies, with the penalty tracking
  *call volume* rather than allocation. Anything that changes what an
  allocation costs in Rust changes the weight of that result. Read it before
  running the benchmark Q19 is waiting for, not after.

---

## What this changed

| Change | Where |
|---|---|
| Spans must reach bytecode, not just forms — instruction-indexed `lines` | ADR-023 |
| Module-level-only mutual recursion promoted from hedge to rule, with the reason | Q17 |
| Mutation checks: prove a test can fail, not just that it passes | Q18 |
| `Rc`-vs-tracing reopened as a real question, with evidence on both sides | Q19 |
| Host-stack recursion presents as unrelated corruption, not a clean overflow | `TRAPS.md` |
| Tagged integers do not have the obvious range | `TRAPS.md` |
| Pre-registered predictions as the habit that makes ADR-021's freedom pay | `BUILD.md` |
| The parallel `lines` array has a cheaper alternative that only works for line numbers | Q30 |
