//! JSON, as a total function in one direction and a partial one in the other
//! (ADR-045 part 6).
//!
//! `serde_json` parses and prints; the mapping to our values is ours, because
//! that is the half ADR-014 says never to delegate — every host-boundary
//! conversion is owned outright. What is delegated is string escaping, number
//! formats, and the conformance surface, which is protocol rather than
//! language.

use crate::host::{io_fault, misuse, string_arg};
use crate::host::{IoKind, IoOp};
use crate::value::{MapObj, StrObj, Value, VecObj};
use crate::vm::{Fault, Vm};
use std::rc::Rc;

pub fn install(vm: &mut Vm) {
    vm.native("json/decode", 1, false, |_, a| {
        let text = string_arg(&a[0], "json/decode")?;
        let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| {
            // The position is the one place a parse error is worth quoting the
            // library on: it names a line and column in the *document*, which
            // is a thing the caller has and we do not.
            io_fault(
                IoOp::Decode,
                IoKind::InvalidData,
                format!("not valid JSON at line {}, column {}", e.line(), e.column()),
            )
        })?;
        Ok(from_json(&parsed))
    });

    vm.native("json/encode", 1, false, |_, a| {
        let j = to_json(&a[0])?;
        Ok(Value::Str(Rc::new(StrObj(j.to_string()))))
    });
}

/// JSON to value. Total: every JSON document is some value.
///
/// Object keys become **strings**, not keywords. Our keywords are interned and
/// would accept `"a b"`, producing a `:a b` that prints as two forms — and
/// type-strict `=` (ADR-041) makes the wrong choice fail loudly at the lookup
/// rather than subtly. Keywordising is something a caller can do; undoing it is
/// not available to anyone.
fn from_json(j: &serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        // An integer when it is one and fits, a float otherwise. ADR-041's
        // tower has no bignum, so a JSON integer past `i64` becomes a float and
        // loses precision — stated rather than hidden, and the alternative is
        // refusing a document a conforming parser accepted.
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(s) => Value::Str(Rc::new(StrObj(s.clone()))),
        serde_json::Value::Array(xs) => {
            Value::Vec(Rc::new(VecObj(xs.iter().map(from_json).collect())))
        }
        // Key order comes from `serde_json`, whose default `Map` is a
        // `BTreeMap` — so keys arrive sorted and iteration is deterministic
        // without anything here doing it again.
        //
        // A sort *was* here, and a mutation pass showed it could be deleted
        // with the whole suite green, because the library had already done it
        // (`notes/milestone-10-mutants.md`). Milestone 2's finding applies:
        // two mechanisms enforcing one rule means no test can say which is
        // working, so the redundancy goes rather than the test.
        //
        // The dependency is real and worth naming: enabling `serde_json`'s
        // `preserve_order` feature swaps the `BTreeMap` for an `IndexMap` and
        // this becomes document order. Still deterministic, but different, and
        // every golden holding a decoded object would move.
        serde_json::Value::Object(o) => Value::Map(Rc::new(MapObj(
            o.iter()
                .map(|(k, v)| (Value::Str(Rc::new(StrObj(k.clone()))), from_json(v)))
                .collect(),
        ))),
    }
}

/// Value to JSON. Partial, and every refusal is an `:io-error` with
/// `:kind :invalid-data` naming what could not be represented.
fn to_json(v: &Value) -> Result<serde_json::Value, Fault> {
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        // JSON has no spelling for these. ADR-032 made the reader and printer
        // agree on one for every float a program can hold; JSON is a third
        // party that agrees with neither. Emitting `NaN` produces a document no
        // conforming parser reads, and emitting `null` turns a number into an
        // absence — a throw is the only option that does not lie.
        Value::Float(f) => match serde_json::Number::from_f64(*f) {
            Some(n) => serde_json::Value::Number(n),
            None => {
                return Err(io_fault(
                    IoOp::Encode,
                    IoKind::InvalidData,
                    "JSON has no spelling for ##NaN, ##Inf, or ##-Inf",
                ))
            }
        },
        Value::Str(s) => serde_json::Value::String(s.0.clone()),
        Value::List(l) => {
            serde_json::Value::Array(l.0.iter().map(to_json).collect::<Result<_, _>>()?)
        }
        Value::Vec(x) => {
            serde_json::Value::Array(x.0.iter().map(to_json).collect::<Result<_, _>>()?)
        }
        Value::Map(m) => {
            let mut o = serde_json::Map::new();
            for (k, val) in &m.0 {
                // Strings and keywords both encode, because a program that
                // built a map to send is likelier to have used keywords than to
                // have remembered this decision. Reading it back gives strings,
                // and that asymmetry is the cost of not keywordising on decode.
                let key = match k {
                    Value::Str(s) => s.0.clone(),
                    _ => {
                        return Err(io_fault(
                            IoOp::Encode,
                            IoKind::InvalidData,
                            "a JSON object key must be a string",
                        ))
                    }
                };
                o.insert(key, to_json(val)?);
            }
            serde_json::Value::Object(o)
        }
        other => {
            return Err(misuse(format!(
                "`json/encode` cannot represent a {}",
                crate::value::kind_name(other)
            )))
        }
    })
}
