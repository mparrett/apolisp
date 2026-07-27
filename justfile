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

# Rung 3 + the properties
test:
    cargo test

# Legibility is a governing constraint, so formatting is part of verification
# rather than a separate ritual. Commit it on its own, never mixed with
# behaviour.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --all-targets -- -D warnings

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
verify: fmt-check check lint test
    cargo run --quiet -- sizes

# Install the advisory pre-commit hook (hooks/pre-commit). It never blocks a
# commit; it just makes formatting and lint findings visible the same day.
hooks:
    git config core.hooksPath hooks
    @echo 'advisory pre-commit hook installed — it warns, it never blocks'

# Regenerate golden files. Deliberately not part of `test`: a golden update is
# a behavioural change, and the review-gated rule (BUILD.md) means a human reads
# the diff and says why. Generating is what *creates* the diff — so generate
# deliberately, then justify every hunk before committing.
bless:
    for f in tests/corpus/*.xs; do cargo run --quiet -- read "$f" > "${f%.xs}.forms"; cargo run --quiet -- spans "$f" > "${f%.xs}.spans"; cargo run --quiet -- compile "$f" > "${f%.xs}.disasm"; done
    @echo 'goldens regenerated — run `git diff` and justify every hunk before committing'

# Constraint #1, on demand. The budget test prints the same numbers per layer.
lines:
    @wc -l src/*.rs
