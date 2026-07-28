#!/usr/bin/env sh
# The soak (BUILD.md). `merge → soak → tag` — this is the middle step, and
# until it was written it was a word in a document rather than something
# anyone could run.
#
# It holds the rungs too slow or too platform-bound for the commit gate.
# `just verify` stays fast enough to run before every commit precisely because
# this exists to catch what that trade gives up.
#
# Three legs, named by BUILD.md:
#
#   1. Release-build divergence. Q10 is open and the two profiles must not
#      drift apart unobserved. The goldens run the *binary*, so under
#      `--release` they are pinning the release artifact and not just
#      re-running library code.
#   2. Reader fuzzing, at a round count no commit gate should pay for. The
#      seeds are fixed, so a larger count is a superset of a smaller one and
#      never a different test.
#   3. Leak checks, which need valgrind and therefore need Linux.
#
# Any nonzero exit is a failure. Leg 3 is skipped loudly rather than silently
# where valgrind is missing — a skip that does not announce itself is how a
# gate turns into a decoration.
set -eu
cd "$(dirname "$0")"

ROUNDS=${APOLISP_FUZZ_ROUNDS:-500000}

echo '--- soak leg 1: release-build divergence'
cargo test --release

echo "--- soak leg 2: reader fuzzing, $ROUNDS rounds"
APOLISP_FUZZ_ROUNDS="$ROUNDS" cargo test --release --test reader \
    the_reader_survives_arbitrary_input

echo '--- soak leg 3: leak checks'
if ! command -v valgrind >/dev/null 2>&1; then
    echo 'soak: leg 3 SKIPPED — no valgrind here. `just soak-linux` runs all three.' >&2
    echo 'soak: ok (2 of 3 legs)'
    exit 0
fi

cargo build --release
# Cargo owns this path, and a custom CARGO_TARGET_DIR would otherwise leave
# valgrind measuring a stale binary from the repository's own target directory.
BIN="${CARGO_TARGET_DIR:-target}/release/apolisp"

# `definite` only. Rust's runtime leaves reachable allocations at exit by
# design, and counting those would make every run red and the check worthless.
VG='valgrind --quiet --leak-check=full --errors-for-leak-kinds=definite
    --error-exitcode=9'

# Two subjects. `churn.xs` allocates and drops; `cycle.xs` builds the cell
# cycle ADR-003 permits, which does not leak because ADR-025 made cells arena
# ids rather than shared pointers.
#
# Neither is a check on the check. A leak this language cannot express cannot
# be provoked by a program written in it, so what validates the tool is a
# mutation pass — run by hand and recorded in `docs/notes/soak-leak-check.md`,
# the same way every other test here earns its trust.
$VG "$BIN" run tests/soak/churn.xs
$VG "$BIN" run tests/soak/cycle.xs

echo 'soak: ok (3 of 3 legs)'
