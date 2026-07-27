#!/usr/bin/env sh
# Rung 2 of the oracle (BUILD.md): read, expand, compile, and execute a hello
# program end to end. Written before the reader was finished, on purpose — a
# failing smoke test is a better queue than an empty one.
#
# Every stage exists as of milestone 5, so the "not built yet" exit code and the
# pending count are gone with it. They were the queue while the pipeline had
# holes; keeping them now would be machinery for a case that cannot arise, and
# an unreachable branch is a branch nobody tests.
set -u
cd "$(dirname "$0")"

PROG=tests/corpus/hello.xs

# Cargo owns the binary path, so a custom CARGO_TARGET_DIR does not leave this
# executing a stale artifact from the repository's own target directory.
stage() {
    name=$1
    printf -- '--- %s\n' "$name"

    cargo run --quiet -- "$name" "$PROG" >/dev/null
    status=$?

    if [ "$status" -eq 0 ]; then
        return 0
    fi

    printf 'smoke: FAILED at `%s` (exit %s)\n' "$name" "$status" >&2
    exit "$status"
}

cargo build --quiet || exit $?

stage read
stage expand
stage compile
stage run

echo "smoke: ok"
