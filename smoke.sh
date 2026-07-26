#!/usr/bin/env sh
# Rung 2 of the oracle (BUILD.md): read, expand, compile, and execute a hello
# program end to end. Written before the reader was finished, on purpose — a
# failing smoke test is a better queue than an empty one.
#
# Each stage is enabled as its milestone lands. A stage that does not exist yet
# must fail loudly rather than be skipped silently.
set -eu
cd "$(dirname "$0")"
cargo build --quiet

PROG=tests/corpus/hello.xs

echo "--- read"
./target/debug/apolisp read "$PROG" >/dev/null

echo "--- expand"   # milestone 5
./target/debug/apolisp expand "$PROG" >/dev/null

echo "--- compile"  # milestone 2
./target/debug/apolisp compile "$PROG" >/dev/null

echo "--- run"      # milestone 3
./target/debug/apolisp run "$PROG"

echo "smoke: ok"
