# apolisp tasks. See BUILD.md for what each rung of the oracle is for.

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

# Everything that should be green before a commit.
verify: check test
    ./target/debug/apolisp sizes

# Regenerate golden files. Deliberately not part of `test`: a golden update is
# a behavioural change, and the review-gated rule (BUILD.md) means a human reads
# the diff and says why. Run this only after doing that.
bless:
    cargo build
    for f in tests/corpus/*.xs; do ./target/debug/apolisp read "$f" > "${f%.xs}.forms"; done
    @echo "goldens regenerated — `git diff` and justify every hunk before committing"

# Constraint #1, on demand.
lines:
    @wc -l src/*.rs
