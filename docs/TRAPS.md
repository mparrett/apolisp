# Semantic traps

A bug list, not a process. Regressions cluster where syntax matches Clojure but
semantics don't, and where a wrong answer looks like a right one. Worth a review
pass over existing code whenever a subsystem lands.

**Truthiness.** Only `nil` and `false` are falsy. `0`, `""`, and empty collections
are truthy. Easy to get wrong in every conditional opcode.

**Integer overflow.** Rust panics in debug and wraps in release. ADR-037 settles
it: arithmetic is checked and overflow throws, so the two builds agree by
construction rather than by testing. The trap survives as a rule about *new*
arithmetic — a `+` written with the plain operator instead of `checked_add` is a
silent divergence between profiles that nothing in debug will show you.
*Test the release build.*

**Equality vs. identity.** Derived `PartialEq` on `Value` follows Rust's variant
and payload equality — `Rc<T>` compares its *pointee*, not its address. That is
still the wrong answer for the language, most obviously because a derived compare
can never make a list equal a vector. Language `=` is structural and crosses
representations; `Rc::ptr_eq` is the only pointer test, and it is for explicit
identity only.

**Hash/equality agreement.** If `1` and `1.0` compare equal, they must hash equal
— or they must not compare equal. Pick one, write it down (Q13).

**Symbols vs. strings vs. keywords.** Symbols and keywords are interned and
compare by id; strings are not, and compare by value. Symbols and keywords share
an intern table but are distinct variants (ADR-025), so an id alone does not tell
you which one you have. Mixing any two of the three is a whole bug class.

**A template can capture a name the caller chose.** `` `(let [x 1] ~body) ``
binds the caller's `x` inside `body`, silently. Clojure refuses to compile that
form — its syntax-quote qualifies `x`, and a qualified symbol is not a legal
binding name — and ADR-040 gives that protection up on purpose, because with one
namespace the rest of qualification does nothing. The replacement is a habit
rather than a mechanism: **every name a template binds gets a `#`**. `x#` is a
fresh symbol per template and cannot collide with anything the caller wrote.

**Metadata loss through expansion.** A macro that rebuilds a form without carrying
spans forward produces errors that point at the expansion instead of the source.
This degrades silently — the code still runs, the diagnostics just get worse over
time. The reader round-trip property test catches printer drift but not this.

**Serialization completeness.** A snapshot that omits the intern table resumes
with wrong symbol identities and *appears to work*. Same for the cell heap and
constants. Silent, which is what makes it the dangerous one.

**Serialization of shared and cyclic structure.** A naive tree walk over an `Rc`
graph blows up exponentially on sharing, breaks identity for cells and atoms, and
does not terminate on a cell cycle — which ADR-003 explicitly permits (Q8).

**A read deadline raises `:would-block`, not `:timeout`.** `tcp/set-timeout`
sets `set_read_timeout`/`set_write_timeout`, and Rust documents the resulting
error kind as `WouldBlock` **or** `TimedOut` depending on the platform — Unix
gives the first, Windows the second. So a program handling a read deadline has
to accept both, and one written and tested on macOS will silently stop
retrying on Windows. `:timeout`'s only reliable raiser is `tcp/connect` with a
timeout argument, which is `connect_timeout` underneath and does return
`TimedOut`. Verified on macOS at milestone 10, and on Linux afterwards, where
it also gives `:would-block` — so the both-kinds tolerance is carrying Windows
alone, and no run has ever exercised the branch that needs it.

**`io/read` is a short read.** `(io/read sock n)` returns *up to* `n` bytes and
returns as soon as any arrive — asking for 99 with 3 in flight gives 3, with no
error and no way to tell that apart from a peer that sent exactly 3. Nothing in
the language reads exactly `n` bytes, so every framed protocol has to loop by
hand, and a first attempt works right up until a payload crosses a packet
boundary. Found by writing one (`notes/first-programs.md`), where it passed by
luck.

**`io/stdout` does not reach the terminal until the program ends.** Writes to it
go through `vm.emit` into `vm.out`, and `main.rs` prints that buffer after the
run. Two `println`s separated by 24 seconds of work arrive at the same instant.
For a batch program this is invisible; for anything interactive it is fatal, and
it fails silently — a pager driven under a pty consumed five keystrokes against
a blank screen and then painted all five frames at once, after quitting
(`notes/the-pager-program.md`). Input is live and output is buffered, which is
the asymmetry to hold on to: `term/read-key` really does block on the real tty.

The buffer is not a defect — it is ADR-029's oracle, and emitted effects being
captured rather than escaping is what makes constraint #2 a property. **Paint
with `(term/open)` instead** (ADR-051): it returns a handle on `/dev/tty`, and
`io/write` to a handle takes the `Host::File` path and reaches the terminal
immediately.

That a painting program then **cannot be snapshotted** is intended, not a
side effect — the handle is not reconstructible, so ADR-043 part 5 refuses the
capture, which is what keeps "output that escaped the buffer is not in the
`Image`" true. `io/stdout` is still buffered and still has no flush point, so
incremental output to a *pipe* has no answer (Q33).

Line endings are the tell if this is got wrong. A program that writes `\r\n` for
raw mode and paints via `io/stdout` gets `\r\r\n`, because the flush happens
after the `finally` has already restored the mode and the tty applies `ONLCR`.
Correct bytes, wrong mode, and nothing reports it.

**A `recur` from a `catch` is allowed here, and is not in Clojure.** This VM
pops a handler record when it dispatches to it, so a catch body runs with no
open region and the ordinary tail-call rule permits re-entering the loop —
50,000 iterations through a firing `catch` stay in constant space. Add a
`finally` and there *is* an open region, and the same rule refuses it. So the
two shapes differ, the difference is not arbitrary, and code moved here from
Clojure will compile where it did not before rather than the other way round
(ADR-047 part 5).

**A length is bytes or scalars and never "length".** *(The `str-len` trap is
gone — ADR-049 removed the name. This is what replaced it and why.)* The surface
is byte-indexed: `str-byte-len` and `str-slice` speak bytes, `str-scalar-len`
and `str-scalars` speak characters, and mixing them is a real error rather than
a style question. Use `str-byte-len` for slicing and framing, `str-scalar-len`
for anything a person will look at — the prelude's `pad-left`/`pad-right`
already do.

The reason the old `str-len` was worse than `str-slice`'s byte indices, and the
rule for whatever gets added next: **an ambiguous unit is dangerous when it
flows into arithmetic and safe when it flows into an operation that validates.**
`str-slice` raises when a bound splits a character, so a mistake announces
itself; a length spent on subtraction is checked by nothing, and misaligns a
column by one space per non-ASCII character with no error at all
(`notes/the-report-program.md`).

**Removing the maximum with `filter` drops duplicates.** The obvious selection
sort — take the largest, `filter` it out, repeat — removes *every* element equal
to the largest rather than one of them, so a list with a repeated key comes back
short. It is the first thing anyone reaches for in a language with no `sort`,
and it is wrong on the first input with a tie.

**Handle validity across migration.** Generational keys catch reuse; they do not
catch a handle that was valid in the source VM and meaningless in the target.
Every adapter declares its reacquisition semantics or refuses.

**`get` cannot tell an absent key from a nil one.** `(get m :k)` answers `nil`
for both, so an assertion written as `(= nil (get m :k))` passes against exactly
the shape it was written to forbid. ADR-042 makes `:path` present only when the
operation names one, and the test pinning that survived a mutation which emitted
`:path nil` — see `notes/milestone-7-mutants.md`. Use `contains?` whenever
absence is the thing being claimed, and pin the key set with `count` when the
shape as a whole matters. The same hazard reaches any program that dispatches on
an optional key.

**The generation bumps when a slot is reused, not when it is released.** Bumping
at `close` looks like the obvious place and makes the id that just closed the
resource stale, so the second `close` a correct `with-open` performs — the body
closed explicitly, the cleanup closes again — reports the aliasing error instead
of being the no-op ADR-016 requires. Idempotent close and stale detection are
only compatible in that order.

**Adding a gensym to the prelude renumbers every later one.** The counter runs
while the prelude expands (ADR-040 resets it per unit, not per form), so a new
prelude macro that uses `x#` shifts `v#` in `and` and `or` for every unit after
it, and every `.expanded` golden moves. The diff will look like a regression in
programs that never call the new macro.

**Manual unwinding.** With an explicit frame stack, `try`/`finally` unwinding is
hand-written. A Rust `?` early-return that skips frame cleanup leaks frames. Rust
panics must never cross the VM loop.

**A bare `catch` swallows typos.** Since ADR-039 a VM fault is a throw, so
`(try (fetch-uesr id) (catch e :default))` catches the *unbound global* and
returns `:default` — the program runs, the misspelling never surfaces, and the
only trace is a `:kind :unbound` inside a value nobody looked at. Clojure's
`(catch Exception e ...)` has the same hazard and the same answer: catch around
the smallest expression that can fail, and look at `:kind` before deciding the
handler applies. There is no filter clause in v1 to make the language check this
for you.

**A caught error loses its position and its suppressed chain.** Both travel
beside the value, not inside it (ADR-039 clause 4), so a handler that re-throws
what it caught throws a value whose origin is now the `throw` in the handler.
The original position is gone, and `.out` will say the wrong line with complete
confidence. Re-throwing is not free the way it looks.

**Variadic rest is an empty list, not `nil`.** Clojure binds a rest parameter to
`nil` when nothing extra was supplied; ADR-033 binds it to an empty list. This is
the deviation most likely to be typed from muscle memory: an empty list is
*truthy* (see Truthiness above), so ported code testing `(if more ...)` or
`(nil? more)` takes the opposite branch — and takes it silently, with no error
anywhere. The compiler emitting `nil` here instead would be equally silent.

**A `let`-bound function cannot call itself.** `(let [f (fn [] (f))] (f))` looks
recursive and is not. ADR-033 makes `let` sequential, so the inner `f` is not in
scope while its own initializer is compiled, and ADR-002/ADR-027 restrict
recursion through a binding to module level. The inner `f` therefore compiles to
a **global** read — no compile error, because a global named `f` may well exist —
and the program fails at run time with an unresolved-global error pointing at the
inner call, which is not where the mistake is. `(fn f [] (f))` and
`(set-global! f (fn [] (f)))` are the two spellings that work.

**Laziness assumptions.** Ported Clojure idioms may assume lazy evaluation. Eager
`map` over an infinite generator hangs rather than erroring.

**Host-stack recursion does not fail cleanly.** If anything ever re-enters the VM
by recursing instead of trampolining (ADR-004), the failure on wasm is not a
depth limit. `../reg-lisp` documents a case where an interpreter overran a 64 KB
wasm stack and wrote *past* it into adjacent static memory — it presented as the
reader's macro map corrupting, and cost three sessions. Symptoms appear in an
unrelated subsystem.

**Tagged integers do not have the obvious range.** `../wallisp`'s fixnums turned
out to be 30-bit rather than 32-bit, and it surfaced only under a benchmark. If
integers are ever tagged, write down the real range and test its edges.
