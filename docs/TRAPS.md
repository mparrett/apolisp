# Semantic traps

A bug list, not a process. Regressions cluster where syntax matches Clojure but
semantics don't, and where a wrong answer looks like a right one. Worth a review
pass over existing code whenever a subsystem lands.

**Truthiness.** Only `nil` and `false` are falsy. `0`, `""`, and empty collections
are truthy. Easy to get wrong in every conditional opcode.

**Integer overflow.** Rust panics in debug and wraps in release. Decide the
language semantics once (Q10) and implement it explicitly so the two builds agree.
*Test the release build.*

**Equality vs. identity.** `Rc` comparison is a pointer test; language `=` is
structural across collection types. Deriving `PartialEq` on `Value` silently gives
the wrong answer.

**Hash/equality agreement.** If `1` and `1.0` compare equal, they must hash equal
— or they must not compare equal. Pick one, write it down (Q13).

**Symbols vs. strings.** Symbols are interned and compare by id. Strings are not,
and compare by value. Mixing these is a whole bug class. Keywords, if they exist
(Q3), join this list.

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

**Laziness assumptions.** Ported Clojure idioms may assume lazy evaluation. Eager
`map` over an infinite generator hangs rather than erroring.
