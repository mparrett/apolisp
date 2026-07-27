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

# On demand rather than in `verify`: a blanket warnings-as-errors policy is
# machinery this project does not need yet, but the lints are worth reading
# before a commit.
lint:
    cargo clippy --all-targets -- -D warnings

# Everything that should be green before a commit.
verify: fmt-check check test
    cargo run --quiet -- sizes

# Regenerate golden files. Deliberately not part of `test`: a golden update is
# a behavioural change, and the review-gated rule (BUILD.md) means a human reads
# the diff and says why. Generating is what *creates* the diff — so generate
# deliberately, then justify every hunk before committing.
bless:
    for f in tests/corpus/*.xs; do cargo run --quiet -- read "$f" > "${f%.xs}.forms"; cargo run --quiet -- spans "$f" > "${f%.xs}.spans"; done
    @echo 'goldens regenerated — run `git diff` and justify every hunk before committing'

# Constraint #1, on demand. The budget test prints the same numbers per layer.
lines:
    @wc -l src/*.rs
