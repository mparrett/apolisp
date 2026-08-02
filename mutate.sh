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

# This edits the working tree in place and restores after each mutation, so for
# the length of a run the checkout is a mutant. Anything else reading the tree
# meanwhile sees broken source: a `just verify` started alongside a run failed
# here once and looked like a real regression until the timing was noticed.
# A second `mutate.sh` would be worse — the two would restore each other's
# mutations and every result would be noise.
LOCK="${TMPDIR:-/tmp}/apolisp-mutate.lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  echo "another mutate.sh is running (lock: $LOCK)" >&2
  echo "it rewrites src/ in place, so two at once produce nothing trustworthy" >&2
  exit 2
fi
# Combined with the restore below into one handler, because a second `trap` on
# the same signals *replaces* the first rather than adding to it — set separately,
# whichever came last would run and the other would silently never fire.

total=0
flipped=0
survived_ok=0
dead=()
seen=()

restore() {
  for f in src/lib.rs src/main.rs src/adapters/tcp.rs src/prelude.xs tests/corpus/editor.xs; do
    [ -f "/tmp/apolisp-mut-$(basename "$f")" ] && cp "/tmp/apolisp-mut-$(basename "$f")" "$f"
  done
}
cleanup() {
  restore
  rmdir "$LOCK" 2>/dev/null
}
trap cleanup EXIT INT TERM
for f in src/lib.rs src/main.rs src/adapters/tcp.rs src/prelude.xs tests/corpus/editor.xs; do
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

# --- The compiler (milestone 2) ---------------------------------------------
#
# `emit` recording the origin it was given is seeded above. The pass's survivor
# was ADR-028 rule 2 enforced *twice* — `try_form` cleared the tail flag and the
# `Call` arm checked the region counter, so neither could be observed alone. It
# was fixed by deleting the redundancy rather than adding a test, which is why
# the counter is mutable here at all: it is now the single enforcement point.

mutation src/lib.rs \
  'if tail && self.regions == 0 {' \
  'if tail {' \
  '--test compile' \
  'a tail call is suppressed inside a handler region (ADR-028 rule 2)'

mutation src/lib.rs \
  'Some(Core::Capture(self.add_capture(level, spec)))' \
  'Some(Core::Capture(0))' \
  '--test compile' \
  'a capture is registered at every level it crosses, not assumed to be the first'

# --- The VM (milestone 3) ---------------------------------------------------
#
# That pass had no survivors, which is only meaningful because the predictions
# were written first. M6 is the one with history: returning restored the wrong
# slot-stack length, it shipped into the working tree, and it was found by a
# program crashing rather than by a test. Read it next to milestone 4's M5, which
# mutates the same line the other way — truncating to the *wrong* length corrupts
# a value and dies; not truncating at all merely retains, and needed a test built
# for it.

mutation src/lib.rs \
  'if argc < fixed || (!p.variadic && argc > fixed) {' \
  'if false {' \
  "$VM" \
  'the callee prologue checks arity, at call time and in the callee (ADR-033)'

mutation src/lib.rs \
  'ex.slots[base + fixed as usize] = Value::List(Rc::new(ListObj(rest)));' \
  'ex.slots[base + fixed as usize] = if rest.is_empty() { Value::Nil } else { Value::List(Rc::new(ListObj(rest))) };' \
  "$VM" \
  'an empty rest parameter is an empty list, never nil (ADR-033, E-11)'

mutation src/lib.rs \
  'ex.slots.truncate(f.ret_len);' \
  'ex.slots.truncate(f.base);' \
  "$VM" \
  'returning restores the callers slot length, not the callees base — the bug that shipped'

mutation src/lib.rs \
  'let me = ex.frames[fi].closure.clone();
                ex.slots[base + dst as usize] = Value::Fn(me);' \
  'ex.slots[base + dst as usize] = Value::Nil;' \
  "$VM" \
  'GetSelf yields the running closure, which is what makes self-recursion identity'

# Expressed at the compiler rather than in the VM: emitting no `TailCall` at all
# is the same observable — every call pushes a frame — and the VM-side version
# needs a `Frame` clone the type does not offer. Same line as the region guard
# above, mutated a second way, which is why both are here.
mutation src/lib.rs \
  'if tail && self.regions == 0 {' \
  'if false {' \
  "$VM" \
  'tail calls are emitted, without which a tail loop grows the frame stack'

# --- ADR-058 and ADR-059: the host boundary ---------------------------------

ARGS='--test lang a_program_receives_the_arguments_after_its_path'
ADAPT='--test adapters'

mutation src/main.rs \
  'apolisp::host::set_args(&mut vm, &args[3..]);' \
  'apolisp::host::set_args(&mut vm, &args[2..]);' \
  "$ARGS" \
  'the driver passes the arguments after the file, not the file itself (ADR-058)'

# The corpus entry is what makes this visible. Dropping the global from the
# `Image` does not fault on resume: the fresh VM `restore` builds has already
# bound it to `[]`, so a lost argument vector reads as a program that was given
# none. Quiet, which is the only kind of snapshot bug there is.
mutation src/lib.rs \
  'vm.gensym = img.gensym;' \
  'vm.gensym = img.gensym;
        crate::host::set_args(&mut vm, &[] as &[&str]);' \
  "$SNAP" \
  'a programs arguments survive the round trip as an ordinary global (ADR-058)'

mutation src/adapters/tcp.rs \
  'IoKind::Timeout,
                        "no connection arrived before the deadline",' \
  'IoKind::WouldBlock,
                        "no connection arrived before the deadline",' \
  "$ADAPT" \
  'an expired accept deadline is :timeout, because this clock is ours (ADR-059)'

mutation src/adapters/tcp.rs \
  'let restored = l
        .set_nonblocking(false)
        .map_err(|e| host_failed(IoOp::Accept, None, &e));
    outcome.and_then(|accepted| restored.map(|()| accepted))' \
  '    outcome' \
  "$ADAPT" \
  'the poll restores the listener, so clearing a deadline blocks again (ADR-059)'

# Declared. The first version of the test above missed this one — it timed out,
# reconnected, accepted again, and passed with the line deleted, because a second
# accept re-enters the poll and sets non-blocking itself. That test was rewritten
# to use a late-connecting peer; this mutation is the *other* restore, and no
# test here can separate it on the platforms this runs on.
mutation src/adapters/tcp.rs \
  'stream
            .set_nonblocking(false)
            .map_err(|e| host_failed(IoOp::Accept, None, &e))?;' \
  '' \
  "$ADAPT" \
  'an accepted socket is put back into blocking mode explicitly (ADR-059)' \
  'whether accept inherits the listeners mode is platform-specific, and neither macOS nor Linux inherits it — so on every platform this suite runs on, the line is unobservable and the guard is for the platform that does'

# --- ADR-060: io/read-dir ----------------------------------------------------

LANGIO='--test lang'

# BUILD.md rule 5 at a syscall. The suite creates its files in an order chosen
# so that creation order and sorted order differ — without which this mutation
# would survive on any filesystem that happens to hand entries back in the order
# they were made.
mutation src/lib.rs \
  '            entries.sort();' \
  '' \
  "$LANGIO" \
  'a listing is sorted, so it cannot flap a golden (ADR-060, BUILD.md rule 5)'

mutation src/lib.rs \
  'if t.is_symlink() {
                        "symlink"
                    } else if t.is_dir() {' \
  'if false {
                        "symlink"
                    } else if t.is_dir() {' \
  "$LANGIO" \
  'a link to a directory is :symlink and not :dir (ADR-060)'

# --- ADR-061: the dead-slot kill and the consuming protocol ------------------

LANGC='--test lang'
DISASM='--test compile'

# The handler guard. A catch can be entered from anywhere in its region, so a
# slot whose last textual read is in the try body is still live. Removing the
# guard makes the analysis treat a try like ordinary control flow, and the loop
# that catches reads a cleared accumulator.
mutation src/lib.rs \
  'Core::Try(_) | Core::Recur(_) => all_reads(e, live),' \
  'Core::Try(t) => { for e in t.body.iter().rev() { liveness(e, live, kills); } }
            Core::Recur(_) => all_reads(e, live),' \
  "$LANGC" \
  'no slot is killed inside a handler region (ADR-061 part 1)'

# A closure captures outer locals through a list on the FnDef, not through
# Core::Local nodes. An analysis blind to that clears the slot before the
# closure copies it, and the closure captures nil.
mutation src/lib.rs \
  'Core::Fn(def) => {
                for c in &def.captures {
                    if let CaptureSpec::Local(id) = c {
                        live.insert(*id);
                    }
                }
            }
            Core::Literal(_) | Core::Capture(_) | Core::SelfFn | Core::Global(_) => {}
            // The branches are alternatives' \
  'Core::Fn(_) => {}
            Core::Literal(_) | Core::Capture(_) | Core::SelfFn | Core::Global(_) => {}
            // The branches are alternatives' \
  "$LANGC" \
  'a closures captured locals are live at the closure (ADR-061 part 1)'

# The branch union is what makes `filter` work: the accumulator is read on both
# arms of an `if` and only one runs. Collapsing the union into a sequential walk
# is *conservative* — it kills less — so no program changes answer and the
# goldens lose MOVEKILLs. The disassembly is the only thing that can see it.
mutation src/lib.rs \
  'let mut la = live.clone();
                liveness(a, &mut la, kills);
                let mut lb = live.clone();
                liveness(b, &mut lb, kills);
                *live = la.union(&lb).copied().collect();' \
  'liveness(a, live, kills);
                liveness(b, live, kills);' \
  "$DISASM" \
  'the two arms of an if are alternatives, not a sequence (ADR-061 part 1)'

# Declared. The kill and the clear are observationally identical by
# construction — that is what the analysis proves — so nothing in a suite of
# assertions can separate them. The benchmark is the check, and it is not a
# test.
mutation src/lib.rs \
  'let v = std::mem::replace(&mut ex.slots[base + src as usize], Value::Nil);
                ex.slots[base + dst as usize] = v;' \
  'ex.slots[base + dst as usize] = ex.slots[base + src as usize].clone();' \
  "$LANGC" \
  'MoveKill clears the source slot (ADR-061 part 1)' \
  'the whole point of the kill is that it changes nothing observable — it drops an Rc reference, and no assertion can see a refcount. Refuted only by the benchmark: with this mutation split goes back to 4.0x per doubling'

# Declared, same reason as the kill above: reverting `seq_items` to the clone it
# used to be restores an O(n) `nth` and changes no answer anywhere.
mutation src/lib.rs \
  'Value::Nil => Ok(&[]),
            Value::List(l) => Ok(&l.0),
            Value::Vec(x) => Ok(&x.0),' \
  'Value::Nil => Ok(Box::leak(Vec::new().into_boxed_slice())),
            Value::List(l) => Ok(Box::leak(l.0.clone().into_boxed_slice())),
            Value::Vec(x) => Ok(Box::leak(x.0.clone().into_boxed_slice())),' \
  "$LANGC" \
  'seq_items borrows rather than cloning, so nth is O(1) (ADR-041 part 1)' \
  'the clone is a cost and not a behaviour, so no assertion separates them — it leaks here only to keep the mutant compiling. The benchmark is the check: with this reverted, xgrep goes back to 4.0x per doubling even with ADR-061 in place, which is how the second quadratic stayed hidden behind conj for three measurements'

echo
echo "=== $flipped of $total flipped, $survived_ok survived as declared"
if [ ${#dead[@]} -gt 0 ]; then
  echo
  echo "checks that proved nothing:"
  for d in "${dead[@]}"; do echo "  - $d"; done
  exit 1
fi
