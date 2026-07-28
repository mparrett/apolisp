//! The terminal, via `crossterm` (ADR-045).
//!
//! ADR-014 delegates terminal portability for the reason it delegates JSON:
//! capability detection and key decoding are protocol, not language. What is
//! owned here is the conversion — a key becomes an ordinary map a program can
//! dispatch on, so nothing about the language knows a terminal exists.
//!
//! The REPL does **not** use this. ADR-044 chose plain stdin and ADR-045 does
//! not revisit it: wiring line editing into the prompt would make
//! `--no-default-features` produce a REPL that *behaves* differently rather
//! than one that is smaller, and a feature is supposed to subtract capability,
//! not change semantics.

use crate::host::{io_fault, misuse};
use crate::host::{IoKind, IoOp};
use crate::value::{MapObj, StrObj, Value};
use crate::vm::Vm;
use std::rc::Rc;

pub fn install(vm: &mut Vm) {
    vm.native("term/size", 0, false, |_, _| {
        let (w, h) = crossterm::terminal::size()
            .map_err(|e| io_fault(IoOp::Read, kind_of(&e), "the terminal size is unavailable"))?;
        Ok(Value::Vec(Rc::new(crate::value::VecObj(vec![
            Value::Int(w as i64),
            Value::Int(h as i64),
        ]))))
    });

    // Raw mode is process-global state that outlives a failed program, so a
    // crash with it on leaves the user's shell unusable. There is no `finally`
    // the host can install here — `with-open` is the language's answer and it
    // works on handles, not on modes — so this is documented rather than
    // solved, and it is the sharpest reason the terminal is an adapter rather
    // than a language feature.
    vm.native("term/raw-mode", 1, false, |_, a| {
        let on = !matches!(a[0], Value::Nil | Value::Bool(false));
        let r = if on {
            crossterm::terminal::enable_raw_mode()
        } else {
            crossterm::terminal::disable_raw_mode()
        };
        r.map(|()| Value::Nil)
            .map_err(|e| io_fault(IoOp::Write, kind_of(&e), "raw mode could not be changed"))
    });

    // Blocking, with an optional timeout in milliseconds. `nil` on timeout
    // rather than a throw: a key that has not arrived yet is not a failure, and
    // making it one would put a `try` inside every input loop.
    vm.native("term/read-key", 0, true, |vm, a| {
        let wait = match a.first() {
            None | Some(Value::Nil) => None,
            Some(Value::Int(ms)) if *ms >= 0 => Some(std::time::Duration::from_millis(*ms as u64)),
            Some(_) => return Err(misuse("`term/read-key` takes an optional timeout in ms")),
        };
        if let Some(d) = wait {
            match crossterm::event::poll(d) {
                Ok(false) => return Ok(Value::Nil),
                Ok(true) => {}
                Err(e) => {
                    return Err(io_fault(
                        IoOp::Read,
                        kind_of(&e),
                        "the terminal could not be polled",
                    ))
                }
            }
        }
        let ev = crossterm::event::read()
            .map_err(|e| io_fault(IoOp::Read, kind_of(&e), "the terminal could not be read"))?;
        Ok(key_value(vm, &ev))
    });
}

/// A key event as an ordinary map, so dispatch is a `get` and the language
/// learns nothing about terminals. Keys are sorted, because a map that reaches
/// a transcript has to print the same way twice (`BUILD.md`, determinism).
fn key_value(vm: &mut Vm, ev: &crossterm::event::Event) -> Value {
    use crossterm::event::{Event, KeyCode};
    let kw = |vm: &mut Vm, s: &str| Value::Keyword(crate::value::KwId(vm.interner.intern(s)));
    let text = |s: String| Value::Str(Rc::new(StrObj(s)));

    let Event::Key(k) = ev else {
        let t = kw(vm, "type");
        let other = kw(vm, "other");
        return Value::Map(Rc::new(MapObj(vec![(t, other)])));
    };
    let (kind, detail) = match k.code {
        KeyCode::Char(c) => ("char", text(c.to_string())),
        KeyCode::Enter => ("enter", Value::Nil),
        KeyCode::Backspace => ("backspace", Value::Nil),
        KeyCode::Left => ("left", Value::Nil),
        KeyCode::Right => ("right", Value::Nil),
        KeyCode::Up => ("up", Value::Nil),
        KeyCode::Down => ("down", Value::Nil),
        KeyCode::Tab => ("tab", Value::Nil),
        KeyCode::Esc => ("esc", Value::Nil),
        _ => ("other", Value::Nil),
    };
    let ctrl = k
        .modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL);
    let (k_char, k_ctrl, k_key) = (kw(vm, "char"), kw(vm, "ctrl"), kw(vm, "key"));
    Value::Map(Rc::new(MapObj(vec![
        (k_char, detail),
        (k_ctrl, Value::Bool(ctrl)),
        (k_key, kw(vm, kind)),
    ])))
}

/// Terminal errors are `std::io::Error`s underneath, so they classify the same
/// way everything else does — one `:kind` vocabulary, not one per adapter.
fn kind_of(e: &std::io::Error) -> IoKind {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => IoKind::PermissionDenied,
        std::io::ErrorKind::Interrupted => IoKind::Interrupted,
        std::io::ErrorKind::TimedOut => IoKind::Timeout,
        _ => IoKind::Other,
    }
}
