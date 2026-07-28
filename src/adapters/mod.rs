//! Host adapters: terminal, TCP, and JSON (ADR-045).
//!
//! Outside the line budget, and the boundary is the point — `BUILD.md`:
//! *substantial host capability is a Rust library behind the handle table, not
//! a language subsystem*. Nothing here decides anything about what a program
//! means. A socket is a `Host` variant reached through the same `io/read` and
//! `io/write` a file uses, and it refuses a snapshot through the same check
//! (ADR-029, ADR-043 part 5).
//!
//! One Cargo feature per capability (ADR-013). Cutting one out removes its
//! primitives and leaves the language alone, which `just subtract` builds and
//! tests rather than asserts.

use crate::vm::Vm;

#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "term")]
pub mod term;

/// Every adapter enabled in this build. The single edge each one has on the
/// VM: cut the module and the matching line here is all that stops compiling.
pub fn install(vm: &mut Vm) {
    #[cfg(feature = "tcp")]
    tcp::install(vm);
    #[cfg(feature = "term")]
    term::install(vm);
    #[cfg(feature = "json")]
    json::install(vm);
    let _ = vm;
}
