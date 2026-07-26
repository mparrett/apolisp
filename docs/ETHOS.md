# Ethos

A small Lisp in the Clojure dialect, with its own VM, in Rust — a substrate for
terminal applications, web services, and simulators. One user, one collaborator.
Optimized for legibility, experimentation, and interactive development; not for
generality, ecosystem compatibility, or stability.

## The four governing constraints

1. **Legibility budget.** The entire core is holdable at once — by one engineer
   and by one language model. This is a context-window constraint, not an
   aesthetic one, and it is the binding constraint on every other decision.
2. **Serializable machine state.** VM state is plain data. A running computation
   can be suspended, serialized, moved, and resumed. Cheap if decided on day one,
   effectively impossible to retrofit.
3. **Performance punching above its weight.** Fast relative to implementation
   size, through concrete representations and good constants — never through JIT,
   type inference, or speculative optimization. This is also *the fun part*:
   narrow, overfitted optimizations are a reason to build this rather than run
   Babashka.
4. **Correctness > simplicity > efficiency > scale.** When they conflict, that
   order.

## Priority order (for breaking ties)

Small readable line count → serializable state → raw speed → ergonomics.

Directional, not a scoring function — it names what gets sacrificed under
pressure. "Ergonomics last" means *inherit Clojure's surface rather than invent
one*. Error quality sits outside the ranking entirely: it is the feedback loop
everything else depends on.

## Seams exist for subtraction

A boundary earns its place when a subsystem can be **deleted or lifted out**.
That is a different driver from modularity, and it is the one that applies here:
needing a home for code is not a reason. One file until it hurts; inline `mod`
blocks are the seams. The test is mechanical — cut the subsystem out and see
whether it still builds (ADR-013, ADR-015).

## Freedom

No third-party users means no compatibility contract. That freedom is the entire
compensation for giving up adoption — it is not a risk to be managed. The
language may break on a Tuesday and be rebuilt on Wednesday.

The one surface where past-you is a user is a serialized snapshot, and ADR-029
holds that to same-build, fresh-VM resume. Everywhere else, break it.

**Non-goals:** third-party users · stable APIs · backward compatibility · package
management · version negotiation · plugin architectures · generic compiler
frameworks · "everything is extensible" · self-hosting · JIT compilation ·
multithreaded execution within one VM.

## What keeps this holdable

- Settled decisions: `ADR.md`, append-only. Supersede, never amend. No version
  bumps, no governance (ADR-022).
- Undecided decisions: `QUESTIONS.md`.
- Line budget, build order, and the oracle: `BUILD.md`.
- Known semantic landmines: `TRAPS.md`.
- Overloaded and unfamiliar terms: `GLOSSARY.md`.
- What the sibling repos already measured: `PRIOR-ART.md`.

The four-snapshot golden corpus is the one piece of process that earns its
weight: it makes reckless optimization safe, and it is how the whole system stays
discussable once the code is real.
