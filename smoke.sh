#!/usr/bin/env sh
# Rung 2 of the oracle (BUILD.md): read, expand, compile, and execute a hello
# program end to end. Written before the reader was finished, on purpose — a
# failing smoke test is a better queue than an empty one.
#
# Stages run in *pipeline* order, which is not the order they are built in:
# expand is milestone 5, while compile and run are milestones 2 and 3. So this
# cannot stop at the first stage that does not exist yet — milestones 2 and 3
# would land with no way for smoke to reach them. Instead the driver exits with
# a distinct code for "not built yet", and only a stage that exists and *fails*
# stops the run.
#
# A stage that does not exist yet is still reported, never skipped silently, and
# smoke stays nonzero while any remain. That is the queue.
set -u
cd "$(dirname "$0")"

PROG=tests/corpus/hello.xs
NOT_IMPLEMENTED=3   # keep in sync with EXIT_NOT_IMPLEMENTED in src/main.rs

pending=0

# Cargo owns the binary path, so a custom CARGO_TARGET_DIR does not leave this
# executing a stale artifact from the repository's own target directory.
stage() {
    name=$1
    milestone=$2
    printf -- '--- %s\n' "$name"

    cargo run --quiet -- "$name" "$PROG" >/dev/null
    status=$?

    if [ "$status" -eq 0 ]; then
        return 0
    fi
    if [ "$status" -eq "$NOT_IMPLEMENTED" ]; then
        printf '    pending — milestone %s (BUILD.md)\n' "$milestone"
        pending=$((pending + 1))
        return 0
    fi

    printf 'smoke: FAILED at `%s` (exit %s)\n' "$name" "$status" >&2
    exit "$status"
}

cargo build --quiet || exit $?

stage read 1
stage expand 5
stage compile 2
stage run 3

if [ "$pending" -gt 0 ]; then
    printf 'smoke: %s stage(s) pending — the queue, not a regression\n' "$pending"
    exit 1
fi

echo "smoke: ok"
