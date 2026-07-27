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

**Handle validity across migration.** Generational keys catch reuse; they do not
catch a handle that was valid in the source VM and meaningless in the target.
Every adapter declares its reacquisition semantics or refuses.

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
