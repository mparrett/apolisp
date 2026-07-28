# The soak's leak check, and what validates it

The soak's third leg (`soak.sh`, BUILD.md) runs two programs under valgrind and
asserts neither loses memory. That assertion is only worth something if the
check can fail, and neither program can make it fail: `churn.xs` allocates and
drops, and `cycle.xs` builds the cell cycle ADR-003 permits, which turns out
not to leak either. So the tool was validated by mutation instead.

## Pre-registration

Inject a definite leak into a path the soak fixtures hit hard, and expect
valgrind's `--errors-for-leak-kinds=definite --error-exitcode=9` to catch it.
Prediction: caught, straightforwardly. The interesting outcome would be a
survivor.

## What actually happened

**Mutant 1 — `std::mem::forget(Box::new([0u8; 64]))` in the `conj` native.**
`churn.xs` calls `conj` 30 times per iteration over 5,000 iterations, so this
is 150,000 leaks of 64 bytes.

Survived. Valgrind exited 0 and reported `definitely lost: 0 bytes in 0
blocks`.

It survived because it never happened. The heap summary is the tell:

```
total heap usage: 1,482,053 allocs, 1,482,052 frees, 127,622,370 bytes allocated
```

Allocations and frees differ by one — the single still-reachable block Rust's
runtime holds at exit — so the 150,000 boxes were never allocated at all. LLVM
elided them. A `Box` that is allocated, never read, and never freed is
unobservable, and the optimizer is entitled to delete it.

**Mutant 2 — the same, wrapped in `std::hint::black_box`.** Caught, exit 9:

```
9,599,936 bytes in 149,999 blocks are definitely lost in loss record 3 of 3
```

149,999 × 64 bytes exactly. The check works.

## The finding

BUILD.md already says to check that the mutant applied, because a substitution
whose pattern no longer matches leaves the tree untouched and the suite green
(`milestone-9-mutants.md`). This is a second way to get the same wrong answer,
and the existing rule does not catch it:

**`grep` confirmed the mutant was in the source, and the build recompiled, and
the mutation still never reached the binary.** Asserting the old text was
present — the milestone-9 rule — passes here. The mutation was real in
`src/lib.rs` and absent from the machine code.

Anything mutated under `--release` is exposed to this. The soak runs release
because that is the profile it exists to test, which means the soak is exactly
where the trap lives. A mutant that adds work with no observable effect is the
vulnerable shape: dead stores, unused allocations, pure calls whose results are
dropped. A mutant that *changes* an observable result is not, which is why the
milestone 1–10 passes were mostly safe by luck rather than by rule.

The check is the same one that caught it here: read the counters, not just the
verdict. `total heap usage` distinguishes "nothing leaked" from "nothing
happened", and every mutation tool has an equivalent — an instruction count, a
transcript, a timing. A verdict alone cannot tell the two apart, and the
flattering reading is still the wrong one.

## What the two subjects are worth

`cycle.xs` was written expecting it to leak, on the assumption that a
self-referential cell closes an `Rc` loop. It does not. ADR-025 made a cell an
index into a VM-owned generational arena, and Q19 says so in as many words —
"with cells as arena ids there is no `Rc` cycle to leak, so the strongest
practical complaint is gone."

That claim had never been tested. It has now: `2,406 allocs, 2,405 frees`,
nothing definitely lost. The file stays in the soak as a second subject rather
than as the check on the check.
