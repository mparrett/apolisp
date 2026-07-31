#!/usr/bin/env bash
# Rung 5 (BUILD.md, ADR-055): does a check actually check anything?
#
# Each mutation breaks one load-bearing line and asserts the named test flips
# from pass to fail. A FAIL from cargo is the desired outcome here; what this
# script reports is whether the flip *happened*.
#
# Three things are asserted rather than left for a reader to notice, because
# every way a mutation rots is silent:
#
#   1. the edit changed the file      — a pattern that no longer matches leaves
#                                       the tree untouched and the suite green,
#                                       which is indistinguishable from a
#                                       survivor and reads as the flattering one
#   2. the mutant still builds        — a mutant that does not compile runs no
#                                       test at all
#   3. the named test failed          — from the exit status, not from grepping
#                                       output for a word
#
# `../reg-lisp` learned all three the hard way: twenty of its eighty-two checks
# had gone quiet without it showing, because the verdict came from a grep and an
# unconditional "a FAIL above is expected" message. It also found one dead check
# hiding behind a live one under a shared description, which is why descriptions
# are checked for duplicates here.
#
# Deliberately not part of `just verify`. Every mutation is a rebuild, so this is
# minutes rather than seconds, and a gate that slow is a gate people learn to
# skip. Run it when touching something a check is supposed to hold.
set -uo pipefail
cd "$(dirname "$0")"

total=0
flipped=0
survived_ok=0
dead=()
seen=()

restore() {
  for f in src/lib.rs src/prelude.xs tests/corpus/editor.xs; do
    [ -f "/tmp/apolisp-mut-$(basename "$f")" ] && cp "/tmp/apolisp-mut-$(basename "$f")" "$f"
  done
}
trap restore EXIT INT TERM
for f in src/lib.rs src/prelude.xs tests/corpus/editor.xs; do
  cp "$f" "/tmp/apolisp-mut-$(basename "$f")"
done

# file, old text, new text, cargo test args, description, [why it must survive]
#
# A sixth argument declares a mutation that *should not* flip, and says why. Two
# kinds of survivor are legitimate and neither is a hole: a claim no test can
# separate (a performance claim, Q18 milestone 6), and a guard against a failure
# so severe it is enforced twice on purpose. Both are predicted here rather than
# discovered, because milestones 4, 7 and 8 all found that a survivor written
# down in advance is evidence and the same survivor found afterwards reads as a
# discovery. An unexplained survivor is still a hole; a declared one that starts
# dying is also reported, because that means the reason stopped being true.
mutation() {
  local file="$1" old="$2" new="$3" check="$4" desc="$5" expect_survive="${6:-}"
  total=$((total + 1))
  echo
  echo "=== $desc"

  # A shared description is how a rotted check hides behind a live one.
  for s in "${seen[@]-}"; do
    if [ "$s" = "$desc" ]; then
      echo "!! DUPLICATE DESCRIPTION — another check already calls itself this"
      dead+=("$desc — duplicate description")
    fi
  done
  seen+=("$desc")

  if ! python3 - "$file" "$old" "$new" <<'PY'
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
src = open(path).read()
n = src.count(old)
if n != 1:
    print(f"   {n} matches", file=sys.stderr)
    sys.exit(1)
open(path, 'w').write(src.replace(old, new))
PY
  then
    echo "!! PROVES NOTHING — the pattern no longer matches $file, so nothing was mutated"
    dead+=("$desc — pattern no longer matches $file")
    restore
    return 0
  fi

  local out rc=0
  out="$(cargo test --quiet $check 2>&1)" || rc=$?
  restore

  # `error[E0xxx]` and "could not compile" are rustc; `error: test failed` is
  # cargo reporting the outcome this script *wants*. Matching `^error:` treated
  # every successful mutation as a broken one — caught on the first run, which is
  # the argument for a harness that says which of the three things went wrong
  # rather than just "no flip".
  if echo "$out" | rg -q 'could not compile|^error\['; then
    echo "!! PROVES NOTHING — the mutant does not build, so no test ran"
    dead+=("$desc — mutant does not build")
    return 0
  fi
  if [ "$rc" -eq 0 ]; then
    if [ -n "$expect_survive" ]; then
      echo "   survived, as declared: $expect_survive"
      survived_ok=$((survived_ok + 1))
      return 0
    fi
    echo "!! SURVIVED — $check passed under the mutation"
    dead+=("$desc — survived")
    return 0
  fi
  if [ -n "$expect_survive" ]; then
    echo "!! DECLARED SURVIVOR DIED — the reason it could not flip has stopped being true:"
    echo "   $expect_survive"
    dead+=("$desc — declared survivor now dies")
    return 0
  fi
  flipped=$((flipped + 1))
  echo "   killed"
}

LANG='--test lang'
GOLDEN='--test vm out_transcripts_match'

# --- ADR-052: str-scalar-slice ----------------------------------------------

mutation src/lib.rs \
  '(Some(f), Some(t)) if f <= t =>' \
  '(Some(f), Some(t)) if true =>' \
  "$LANG" \
  'str-scalar-slice: the f<=t guard, without which a backwards slice panics' \
  'the scan breaks at `to`, so with `from > to` it never finds `from` and the
   missing-bound error fires first. The guard is unreachable while the break
   stands, and both exist because reaching `s[f..t]` backwards is a panic where
   ADR-039 requires a throw — a process abort is worth enforcing twice.'

mutation src/lib.rs \
  'if n == to {
                    b_to = Some(b);' \
  'if n + 1 == to {
                    b_to = Some(b);' \
  "$LANG" \
  'str-scalar-slice: off by one on the upper bound'

# --- ADR-053: vec-slice, and take/drop on top of it -------------------------

mutation src/lib.rs \
  'if from > to || to > items.len() {' \
  'if from > to {' \
  "$LANG" \
  'vec-slice: the out-of-range bound check'

mutation src/prelude.xs \
  '(vec-slice xs 0 (if (< n 0) 0 (if (> n c) c n)))' \
  '(vec-slice xs 0 (if (> n c) c n))' \
  "$LANG" \
  'take: the clamp that makes a negative count mean none'

# --- The editor, whose geometry is held by one golden -----------------------

mutation tests/corpus/editor.xs \
  '(inc (quot (str-scalar-len s) w)))' \
  '(max2 1 (quot (str-scalar-len s) w)))' \
  "$GOLDEN" \
  'wrap-height: the row that holds the append cursor at an exact multiple'

mutation tests/corpus/editor.xs \
  '(max2 1 (if (shows-help? state rows) (- rows 2) (dec rows))))' \
  '(if (shows-help? state rows) (- rows 2) (dec rows)))' \
  "$GOLDEN" \
  'text-rows: the floor that keeps the scroll from passing the cursor'

mutation tests/corpus/editor.xs \
  'true (str (str-scalar-slice s 0 (dec w)) ">")))' \
  'true (str-scalar-slice s 0 w)))' \
  "$GOLDEN" \
  'clip: the marker that makes truncation visible'

mutation tests/corpus/editor.xs \
  '[(screen-offset state w (get state :scroll-row) (get state :scroll-sub)
                    (get state :cursor-row) (quot (get state :cursor-col) w))
     (rem (get state :cursor-col) w)]' \
  '[(- (get state :cursor-row) (get state :scroll-row)) (get state :cursor-col)]' \
  "$GOLDEN" \
  'cursor-screen: the wrapped cursor position paint depends on'

echo
echo "=== $flipped of $total flipped, $survived_ok survived as declared"
if [ ${#dead[@]} -gt 0 ]; then
  echo
  echo "checks that proved nothing:"
  for d in "${dead[@]}"; do echo "  - $d"; done
  exit 1
fi
