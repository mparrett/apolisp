# apolisp

A small Lisp in the Clojure dialect with its own VM, in Rust. Substrate for
terminal applications, web services, and simulators. One user; the language may
break on a Tuesday.

No code yet. What exists is the design, and it is meant to be read in this order.

## Normative — read to implement

1. **`docs/ETHOS.md`** — the four constraints and what gets sacrificed when they
   conflict. Short. Read it first and read all of it.
2. **`docs/ADR.md`** — every settled decision, with cost and rejected
   alternatives. Append-only: entries are superseded, never edited. Check the
   status line under a heading before trusting the body. Factual corrections to
   decisions that still stand are in **Errata** at the end.
3. **`docs/BUILD.md`** — the line budget, the milestone you are on, and the
   oracle that has to be green.

## Consult on demand

- **`docs/TRAPS.md`** — semantic landmines. Read before writing equality,
  arithmetic, unwinding, or serialization.
- **`docs/QUESTIONS.md`** — what is deliberately undecided, ordered by the
  milestone that forces the answer. If your task needs one of these, it is a
  question, not a judgment call.
- **`docs/GLOSSARY.md`** — overloaded terms. "Snapshot" means two things,
  "atom" means two things, and form-vs-value has a real answer.

## Evidence, not instruction

- **`docs/PRIOR-ART.md`** — what three sibling repos measured about these same
  choices. Useful for understanding *why* an ADR went the way it did; not
  required to implement anything.

## Not normative

- **`docs/archive/`** — superseded reviews, kept for the reasoning trail. They
  discuss documents that no longer exist and recommend things later ADRs
  overruled. Do not implement from them.

## Working rules

- A decision that is not in `ADR.md` has not been made. Ask, or open a question.
- Changing an ADR means adding one that supersedes it — never editing in place,
  never a version bump.
- Never regenerate a golden file to make a test pass. That is a failed task, not
  a fix.
