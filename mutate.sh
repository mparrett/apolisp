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

  # A mutant can hang rather than fail — `notes/milestone-8-mutants.md` has one
  # that stops decrementing fuel, and it terminated only because a step counter
  # happened to carry a guard. Bounded here rather than left to luck; a timeout
  # is reported as its own outcome, because "it never finished" is not "the check
  # caught it".
  local out rc=0
  out="$(perl -e '
    my $pid = fork();
    if ($pid == 0) { exec @ARGV or exit 127 }
    local $SIG{ALRM} = sub { kill 9, $pid; waitpid $pid, 0; exit 124 };
    alarm 120;
    waitpid $pid, 0;
    exit $? >> 8;
  ' -- sh -c "cargo test --quiet $check 2>&1")" || rc=$?
  restore

  if [ "$rc" -eq 124 ]; then
    echo "!! PROVES NOTHING — the mutant did not finish inside 120s"
    dead+=("$desc — timed out")
    return 0
  fi

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

# --- Semantics the VM decides (milestones 3, 6) -----------------------------

mutation src/lib.rs \
  'ex.slots[base + cond as usize],
                    Value::Nil | Value::Bool(false)' \
  'ex.slots[base + cond as usize],
                    Value::Nil' \
  "$LANG" \
  'falsiness: only nil and false are falsy, so false must not become truthy'

mutation src/lib.rs \
  '(Value::Float(x), Value::Float(y)) => x == y,' \
  '(Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),' \
  "$LANG" \
  'float equality is IEEE, not bit patterns: -0.0 equals 0.0 and NaN equals nothing'

mutation src/lib.rs \
  'Rc::make_mut(&mut out).0.extend(rest.iter().cloned());' \
  'for v in rest { Rc::make_mut(&mut out).0.insert(0, v.clone()); }' \
  "$LANG" \
  'conj adds at the back of a vector and the front of a list'

# --- The string surface ADR-049 named ---------------------------------------

mutation src/lib.rs \
  'string(&a[0], "str-scalar-len")?.chars().count() as i64' \
  'string(&a[0], "str-scalar-len")?.len() as i64' \
  "$LANG" \
  'str-scalar-len counts scalars, not bytes — the whole of ADR-049'

mutation src/lib.rs \
  'let from = index(&a[2], "str-index-of")?;' \
  'let from = index(&a[2], "str-index-of")? * 0;' \
  "$LANG" \
  'str-index-of honours its from offset, which is what keeps split linear'

# --- ADR-050: ordering, and the stability sort-by rests on ------------------

mutation src/prelude.xs \
  '(if (less? (first b) (first a))' \
  '(if (not (less? (first a) (first b)))' \
  "$LANG" \
  'merge takes from the left on a tie, which is what makes the sort stable'

mutation src/prelude.xs \
  '(vec-slice xs (if (< n 0) 0 (if (> n c) c n)) c)' \
  '(vec-slice xs (if (< n 0) 0 n) c)' \
  "$LANG" \
  'drop: the clamp above the count, without which it raises instead of emptying'

# --- ADR-053: vec-slice returns a vector whatever it was given ---------------

mutation src/lib.rs \
  'Ok(Value::Vec(Rc::new(VecObj(items[from..to].to_vec()))))' \
  'Ok(Value::List(Rc::new(ListObj(items[from..to].to_vec()))))' \
  "$LANG" \
  'vec-slice returns a vector, which is what take and drop promise their callers'

# --- The expander (milestone 5) ---------------------------------------------

mutation src/lib.rs \
  '// this (ADR-044 part 1).
        vm.reset_gensym();' \
  '// this (ADR-044 part 1).' \
  '--test expand' \
  'expand_all resets the gensym counter, so a unit expands the same way twice'

# --- The compiler (milestone 2) ---------------------------------------------

mutation src/lib.rs \
  'self.lines.push(o);' \
  'self.lines.push(SpanOrigin::Unknown);' \
  '--test compile' \
  'emit records the origin it was given rather than Unknown'

# --- The handler stack (milestone 4) ----------------------------------------
#
# `notes/milestone-4-mutants.md` ran five of these and predicted every outcome.
# Four are here. The fifth — unwinding drops the frames but keeps their slots —
# **cannot be written any more**, and that is the finding rather than an
# omission: it survived the whole suite when it was expressible, because the leak
# is bounded, never reaches a value and cannot move a high-water mark. The
# response was structural, not another test. `drop_frame` became the only place a
# frame is released, so writing the unwinding half alone means breaking returning
# too. The mutation below is what is left of it, and it dies loudly.

VM='--test vm'

mutation src/lib.rs \
  'while ex.frames.len() > h.frame + 1 {
            drop_frame(ex);
        }' \
  '' \
  "$VM" \
  'unwind drops the frames above the handler owner, through the same call a return uses'

mutation src/lib.rs \
  'ex.slots.truncate(f.ret_len);' \
  '' \
  "$VM" \
  'drop_frame gives the slots back, which nothing observed until ADR-055 found it'

mutation src/lib.rs \
  '                    .expect("ENDFINALLY outside an unwinding cleanup");
                return Err(p.unwind);' \
  '                    .expect("ENDFINALLY outside an unwinding cleanup");
                let _ = p.unwind;' \
  "$VM" \
  'ENDFINALLY re-raises the parked unwind rather than carrying on'

mutation src/lib.rs \
  'let h = ex
                    .handlers
                    .pop()
                    .expect("POPHANDLER with no open handler region");
                debug_assert_eq!(h.frame, fi, "a handler record outlived its frame");' \
  '' \
  "$VM" \
  'POPHANDLER pops, so a region does not outlive the code it guards'

mutation src/lib.rs \
  'let p = ex.pending.pop().expect("just checked");
            u.suppress(p.unwind);' \
  'let _p = ex.pending.pop().expect("just checked");' \
  "$VM" \
  'a displaced parked unwind is merged into the one that displaced it (ADR-028 invariant 3)'

# --- The snapshot (milestone 8) ---------------------------------------------
#
# `notes/milestone-8-mutants.md` ran all ten and predicted all ten, including
# four survivors. Every one of those four was the same kind: a field the encoder
# handles correctly that no program in the snapshot corpus ever populated. The
# strongest property in the project — cutting at every instruction boundary, over
# nine programs, in two forms — caught none of them, because a property only sees
# the state its inputs create.
#
# All four were closed by adding programs rather than by changing the encoder, so
# all ten should die here. Any survivor is that corpus having lost a program.

SNAP='--test snapshot'

mutation src/lib.rs \
  'out: vm.out.clone(),' \
  'out: Default::default(),' \
  "$SNAP" \
  'capture keeps the output buffer, which is what the transcript compares'

mutation src/lib.rs \
  'gensym: vm.gensym,' \
  'gensym: 0,' \
  "$SNAP" \
  'capture keeps the gensym counter, so a resume cannot reissue a name it handed out'

mutation src/lib.rs \
  'if img.fingerprint != fingerprint(chunk) {' \
  'if false {' \
  "$SNAP" \
  'restore checks the fingerprint, which is what makes code identity a check'

mutation src/lib.rs \
  'self.seen.insert(addr, id);' \
  '' \
  "$SNAP" \
  'the encoder shares repeated objects rather than copying them (ADR-043 part 2)'

mutation src/lib.rs \
  '        let pending = ex
            .pending
            .iter()' \
  '        let pending = ex
            .pending
            .iter()
            .take(0)' \
  "$SNAP" \
  'capture keeps the parked unwinds, so a cut mid-cleanup resumes into it'

mutation src/lib.rs \
  'free_handles: vm.free_handles.clone(),' \
  'free_handles: Vec::new(),' \
  "$SNAP" \
  'capture keeps the free list, so a reopened handle does not reuse a live id'

mutation src/lib.rs \
  'handle_generations: vm.handles.iter().map(|h| h.generation).collect(),' \
  'handle_generations: Vec::new(),' \
  "$SNAP" \
  'capture keeps the handle generations, so a reused slot hands out a different id'

mutation src/lib.rs \
  'vm.interner = Interner::restore(img.names.clone());' \
  '' \
  "$SNAP" \
  'restore rebuilds the interner rather than starting a fresh one'

mutation src/lib.rs \
  'Value::Float(f) => Ref::Float(f.to_bits()),' \
  'Value::Float(f) => Ref::Float((f + 0.0).to_bits()),' \
  "$SNAP" \
  'the encoder keeps a float bit for bit, so -0.0 survives (ADR-032)'

mutation src/lib.rs \
  'ex.fuel -= 1;' \
  '' \
  "$SNAP" \
  'fuel decrements, without which a fuelled run never suspends'

# --- The expander (milestone 5) ---------------------------------------------
#
# Five of that pass's six; the gensym reset is seeded above. All were fixed then,
# two of them by adding to the corpus rather than to the code — `macros.xs`
# claimed in a comment to have "a splice, and a tail the splice does not swallow"
# and did not, and no macro anywhere kept an argument as *data*, which is the only
# shape that can tell early expansion from late.

EXPAND='--test expand'

mutation src/lib.rs \
  'if head == self.names.quote {
                    return Ok(f);
                }' \
  '' \
  "$EXPAND" \
  'the expander does not walk into quote, which is data rather than code'

mutation src/lib.rs \
  "if name.len() > 1 && name.ends_with('#') {" \
  'if false {' \
  "$EXPAND" \
  'auto-gensym replaces a trailing # rather than handing back the written name'

mutation src/lib.rs \
  'if !plain.is_empty() {
                groups.push(call(Value::Sym(self.names.list), plain));
            }
            Ok(Items::Spliced' \
  'Ok(Items::Spliced' \
  "$EXPAND" \
  'a template keeps the items after a splice, which macros.xs once only claimed to cover'

mutation src/lib.rs \
  'let mut known = HashMap::new();
            for (v, o) in items[1..].iter().zip(&f.origins.children[1..]) {
                index_origins(v, o, &mut known);
            }' \
  'let known = HashMap::new();' \
  '--test compile' \
  'a form the macro passed through keeps its own position instead of the call site'

mutation src/lib.rs \
  'let args: Vec<Value> = items[1..].to_vec();' \
  'let mut args: Vec<Value> = Vec::new();
            for (v, o) in items[1..].iter().zip(&f.origins.children[1..]) {
                args.push(
                    self.form(LocatedForm { root: v.clone(), origins: o.clone() })?
                        .root,
                );
            }' \
  "$EXPAND" \
  'a macro receives its arguments as written, not already expanded'

echo
echo "=== $flipped of $total flipped, $survived_ok survived as declared"
if [ ${#dead[@]} -gt 0 ]; then
  echo
  echo "checks that proved nothing:"
  for d in "${dead[@]}"; do echo "  - $d"; done
  exit 1
fi
