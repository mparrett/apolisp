# Working on apolisp

A small Lisp in the Clojure dialect with its own VM, in Rust. One user, no
compatibility contract, no stability promise.

**This file is loaded into every session, so it spends the same budget
constraint #1 is about.** It routes and states the rules that break the project
if they slip. It does not restate the docs. Keep it that way.

## Read before writing code

`docs/ETHOS.md` first, all of it — it is short and it is what everything else
defers to. Then, as needed:

| Question | File |
|---|---|
| Has this been decided? | `ADR.md` — check the status line under the heading before trusting the body |
| What am I building, and what has to be green? | `BUILD.md` |
| Is this a known semantic landmine? | `TRAPS.md` |
| Is this deliberately undecided? | `QUESTIONS.md` |
| What does this overloaded term mean here? | `GLOSSARY.md` |

## The five rules

1. **Never regenerate a golden file to make a test pass.** That is a failed
   task, not a fix. Goldens *must* change when behaviour changes on purpose —
   the rule is a reviewed diff, not immutability.
2. **`ADR.md` is append-only.** Supersede with a new entry; never edit one in
   place, never bump a version. Factual corrections to decisions that still
   stand go in **Errata**.
3. **A decision that is not in `ADR.md` has not been made.** Ask, or open a
   question in `QUESTIONS.md`. Resolving it in a code comment and moving on is
   how the project acquires decisions nobody agreed to.
4. **The line budget is hard.** Over budget is a new ADR, not a nudged constant
   or a silenced test.
5. **Determinism is a prerequisite.** Anything that can reach output — map
   iteration, gensym counters, directory listings — is ordered explicitly.
   Nondeterminism makes goldens flap, flapping goldens get disabled, and then
   there is no oracle.

## Commands

```
just verify       # the gate: fmt, check, clippy, tests, subtraction build, Value size
just subtract     # ADR-013's harness: build and test with a host capability cut out
just test         # rung 3 and the properties
just smoke        # rung 2 — NONZERO ON PURPOSE while any stage is pending
just bless        # regenerate goldens; then read every hunk before committing
just lines        # constraint #1, per file and per layer
just hooks        # install the advisory pre-commit hook (warns, never blocks)
```

Run `just verify` before a commit. Its first pass shipped unformatted code with
a clippy error because neither check was in the one command anyone runs.

## Where things live

- **Language behaviour:** `src/lib.rs` — one file, inline `mod` blocks (ADR-015,
  ADR-031).
- **Process driver only:** `src/main.rs`. If a change there changes what a
  program *means*, it is in the wrong file.
- Properties test the **library**; goldens run the **binary**. Never rebuild a
  `target/debug/...` path by hand — use `CARGO_BIN_EXE_apolisp`.
- Dependencies are fixed by ADR-014. Adding one is an ADR.
- **The write-ups are `docs/*.html`, and they are frozen** (ADR-057). Thirteen
  pages on milestones 1–10 and the sessions after, indexed by `index.html`,
  true as of 2026-08-01 and **not maintained**. Do not refresh their counts
  when the code moves; a stale number in a dated artifact is not a defect. A
  later phase is a new series, not an edit to this one.

## Habits that have already paid

- **Pre-register.** Before a benchmark or a mutation check, write down what you
  expect and why; afterwards record whether it was refuted. The refutations are
  the interesting part. Two lines in a commit message is enough.
- **Check the mutant applied.** A pattern that no longer matches leaves the
  tree untouched and the suite green — indistinguishable from a survivor, and
  the flattering reading is the wrong one. Assert the old text was there before
  writing the new (`docs/notes/milestone-9-mutants.md`).
- **Break a new check while you are writing it, before you trust it.** Six here
  have been green while testing the wrong thing, and every one was a *true
  assertion applied to the wrong subject* — review reads the assertion, nothing
  reads the subject. Three were caught at write time; the other three had been
  believed for days. Mutate it, feed it an input unlike the others, or just print
  what it observed — the last costs thirty seconds and catches a check looking at
  nothing (`docs/notes/the-corpus-as-an-oracle.md`).
- **Separate commits** for docs, for behaviour, and for formatting.
