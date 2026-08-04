# apolisp tasks. See BUILD.md for what each rung of the oracle is for.
#
# Nothing here hard-codes `target/debug/apolisp`. Cargo owns that path, and
# under a custom CARGO_TARGET_DIR a hand-built one runs a stale binary while the
# tests compile somewhere else — a green suite testing yesterday's code.

default: check

# Rung 1
check:
    cargo check

# Rung 2
smoke:
    ./smoke.sh

# Rung 3 + the properties.
#
# `--no-fail-fast` because cargo stops launching test *binaries* after one of
# them fails, and the goldens are spread across several. Without it, a prelude
# change that breaks a golden in `compile.rs` hides one it also broke in
# `expand.rs`: the gate reports two failures, you fix two, and the third is
# still there. Found 2026-08-03 by a change that broke exactly that trio.
test:
    cargo test --no-fail-fast

# Legibility is a governing constraint, so formatting is part of verification
# rather than a separate ritual. Commit it on its own, never mixed with
# behaviour.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

#
# Its own target directory for the reason `subtract` has three: clippy drives
# rustc through a different wrapper, so sharing a directory with `check` and
# `test` means each of the three invalidates the other two on every run.
lint:
    CARGO_TARGET_DIR=target/clippy cargo clippy --all-targets -- -D warnings

# Q10 is open and release turns on overflow checks, so the two profiles must not
# be allowed to drift apart unobserved.
test-release:
    cargo test --release

# Everything that should be green before a commit.
#
# fmt-check and lint are in here rather than left on the side. The first pass of
# this project shipped unformatted code with a clippy error, and the reason is
# that neither was in the one command anyone runs — a gate you have to remember
# is not a gate.
verify: fmt-check check lint test subtract
    cargo run --quiet -- sizes

# ADR-013's subtraction harness, built rather than asserted. The features exist
# for exactly this: cutting a host capability out and finding that the language
# is untouched. Milestone 7 was the first milestone with a capability to cut, so
# this is the first run where the claim can be false.
#
# It is a full `test` and not a `check` on purpose. Compiling proves the cfgs
# line up; running proves `io/open` degrades to an ordinary unbound global and
# takes nothing else with it.
# Four points, not the whole 2^4 lattice: everything off, `fs` alone, and `term`
# alone. The middle points are the ones that catch a `#[cfg]` written as if two
# features always travel together — with only all-on and all-off, `any(a, b)`
# and `all(a, b)` are indistinguishable.
#
# `term` alone is here because ADR-051 made it a point that means something.
# Before it, `Host::File` was `fs`-only and the terminal could read keys and not
# paint; the variant is now `any(fs, term)`, and this is the only build that
# compiles that arm without `fs` also supplying it.
#
# Each configuration gets its own CARGO_TARGET_DIR, and that is the whole
# difference between this recipe taking 872 seconds and 31. Cargo keeps one set
# of artifacts per target directory, so cycling four feature sets through one
# directory means each invocation invalidates the last — six rebuilds from
# scratch here, and a seventh next time `test` runs with default features. The
# directories cost about 440 MB together, because `--no-default-features` cuts
# the dependency tree out and these builds are almost entirely this crate.
# Measured 2026-08-03, after `touch src/lib.rs`.
subtract:
    CARGO_TARGET_DIR=target/sub-none cargo clippy --no-default-features --all-targets -- -D warnings
    CARGO_TARGET_DIR=target/sub-none cargo test --no-default-features
    CARGO_TARGET_DIR=target/sub-fs cargo clippy --no-default-features --features fs --all-targets -- -D warnings
    CARGO_TARGET_DIR=target/sub-fs cargo test --no-default-features --features fs
    CARGO_TARGET_DIR=target/sub-term cargo clippy --no-default-features --features term --all-targets -- -D warnings
    CARGO_TARGET_DIR=target/sub-term cargo test --no-default-features --features term

# BUILD.md's ladder: `merge → soak → tag`. Two of the three legs run anywhere;
# the leak check needs valgrind and says so rather than passing quietly.
soak:
    ./soak.sh

# All three legs, in the container that has valgrind.
soak-linux:
    docker build --target soak --tag apolisp-soak .
    docker run --rm apolisp-soak

# BUILD.md's follow-up: the same gate, on Linux. Deliberately *not* a
# dependency of `verify` — it needs a running daemon, and a gate with an
# external prerequisite is a gate people learn to skip. This is a pre-tag and
# pre-soak step, not an inner-loop one.
#
# The image is hermetic (see Dockerfile), so a source change recompiles from
# scratch. That is the cost of the answer being about the source rather than
# about the container's history.
verify-linux:
    docker build --tag apolisp-verify .
    docker run --rm apolisp-verify

# Install the advisory pre-commit hook (hooks/pre-commit). It never blocks a
# commit; it just makes formatting and lint findings visible the same day.
hooks:
    git config core.hooksPath hooks
    @echo 'advisory pre-commit hook installed — it warns, it never blocks'

# Run the editor on a file. The corpus keeps the pure core and `examples/` keeps
# the shell, because one is goldenable and the other needs a tty — and a file is
# a compilation unit with no `load`, so running it means joining them here. The
# `(edit ...)` call is appended rather than baked into the shell, which is how a
# program takes an argument in a language with no argv.
#
# Release on purpose: the debug VM is roughly ten times slower and it shows.
edit FILE:
    @cargo build --release --quiet
    @mkdir -p target/examples
    @sed '/^; --- The session/,$d' tests/corpus/editor.xs > target/examples/editor.xs
    @cat examples/editor-shell.xs >> target/examples/editor.xs
    @printf '\n(edit "%s")\n' '{{FILE}}' >> target/examples/editor.xs
    @./target/release/apolisp run target/examples/editor.xs

# Rung 5 (ADR-055): does a check actually check anything? Each mutation breaks
# one load-bearing line and asserts the named test flips. Deliberately not in
# `verify` — every mutation is a rebuild, and a gate that slow gets skipped.
mutate:
    ./mutate.sh

# Regenerate golden files. Deliberately not part of `test`: a golden update is
# a behavioural change, and the review-gated rule (BUILD.md) means a human reads
# the diff and says why. Generating is what *creates* the diff — so generate
# deliberately, then justify every hunk before committing.
bless:
    for f in tests/corpus/*.xs; do cargo run --quiet -- read "$f" > "${f%.xs}.forms"; cargo run --quiet -- spans "$f" > "${f%.xs}.spans"; cargo run --quiet -- expand "$f" > "${f%.xs}.expanded"; cargo run --quiet -- compile "$f" > "${f%.xs}.disasm"; done
    # `.out` is updated where one exists and never created. Which programs run
    # is a decision the test asserts, not a set to infer. Exit 1 is a program
    # that failed, and since ADR-039 that is a transcript like any other — the
    # driver's own failures are 2 and 3, and those still stop the recipe.
    for f in tests/corpus/*.xs; do if [ -f "${f%.xs}.out" ]; then cargo run --quiet -- run "$f" > "${f%.xs}.out" || [ $? -eq 1 ]; fi; done
    # ADR-048: the prelude is compiled into every unit and printed in none of
    # them, so it needs its own golden or it is pinned nowhere.
    cargo run --quiet -- prelude > tests/prelude.disasm
    @echo 'goldens regenerated — run `git diff` and justify every hunk before committing'

# Milestone 9. A session is one compilation unit, so definitions and macros
# accumulate across inputs and the gensym counter never restarts (ADR-044).
repl:
    cargo run --quiet -- repl

# Constraint #1, on demand. The budget test prints the same numbers per layer.
lines:
    @wc -l src/*.rs
