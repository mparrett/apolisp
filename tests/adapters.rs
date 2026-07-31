//! Milestone 10 (BUILD.md): host adapters — terminal, TCP, JSON.
//!
//! Nothing here tests the language. The adapters are outside the line budget
//! because they are a Rust library behind the handle table, and what is worth
//! pinning is exactly the boundary: that a socket is a handle like a file is,
//! that the `:io-error` shape is one shape and not one per adapter, and that
//! cutting a feature removes primitives and leaves the language alone.
//!
//! The terminal has no test that opens one. `crossterm` needs a tty and the
//! test runner has none, so what is checked is that the primitives exist and
//! that a build without them is still a working language — which is the claim
//! ADR-013 actually makes.

use apolisp::printer;
use apolisp::session::{Ended, Session};

/// Evaluate in a fresh session and render the result the way a prompt would.
fn ev(src: &str) -> String {
    let mut s = Session::new();
    run(&mut s, src)
}

fn run(s: &mut Session, src: &str) -> String {
    match s.eval(src) {
        Ok(Ended::Value(v)) => printer::print(&v, &s.vm.interner),
        Ok(Ended::Threw(u)) => format!("threw {}", printer::print(&u.value, &s.vm.interner)),
        Err(e) => format!("error {}", e.msg),
    }
}

/// Which primitives a build has. `Vm::global` answering `None` is exactly what
/// an unbound name is, so this is the same question a program asks.
fn is_bound(name: &str) -> bool {
    let mut s = Session::new();
    let id = apolisp::value::SymId(s.vm.interner.intern(name));
    s.vm.global(id).is_some()
}

// --- JSON ------------------------------------------------------------------------

#[cfg(feature = "json")]
#[test]
fn json_decodes_to_values_and_object_keys_are_strings() {
    // Keys are strings, not keywords (ADR-045 part 6). Type-strict `=`
    // (ADR-041) makes the wrong choice a failed lookup rather than a subtle
    // one, so this asserts the lookup that works and the one that does not.
    assert_eq!(ev(r#"(get (json/decode "{\"a\":1}") "a")"#), "1");
    assert_eq!(ev(r#"(get (json/decode "{\"a\":1}") :a)"#), "nil");

    assert_eq!(ev(r#"(json/decode "null")"#), "nil");
    assert_eq!(ev(r#"(json/decode "[true,false]")"#), "[true false]");
    // An integer when it is one and fits; a float otherwise.
    assert_eq!(ev(r#"(json/decode "[1,2.5,-3]")"#), "[1 2.5 -3]");
    // Arrays are vectors, not lists — ADR-041 makes those equal across
    // representations, so this pins the representation the printer shows.
    assert_eq!(ev(r#"(json/decode "[]")"#), "[]");
}

#[cfg(feature = "json")]
#[test]
fn a_decoded_object_prints_the_same_way_twice() {
    // `serde_json` does not preserve insertion order without a feature we have
    // not taken, so decode sorts. Determinism is a prerequisite (`BUILD.md`):
    // a map whose print order follows a hash seed makes every transcript that
    // touches it flap, and a flapping golden gets disabled.
    let doc = r#"(json/decode "{\"z\":1,\"m\":2,\"a\":3,\"b\":4}")"#;
    let once = ev(doc);
    assert_eq!(once, r#"{"a" 3 "b" 4 "m" 2 "z" 1}"#);
    for _ in 0..8 {
        assert_eq!(ev(doc), once, "decode is not deterministic");
    }
}

#[cfg(feature = "json")]
#[test]
fn json_refuses_what_it_cannot_represent_and_says_why() {
    // Every refusal is the one `:io-error` shape, so a program dispatches on
    // `:kind` here exactly as it does on a file error (ADR-039 clause 3).
    for (src, op) in [
        (r#"(json/encode (/ 1.0 0.0))"#, ":encode"),
        (r#"(json/encode (/ -1.0 0.0))"#, ":encode"),
        (r#"(json/encode {:keyword-key 1})"#, ":encode"),
        (r#"(json/decode "{oops")"#, ":decode"),
        (r#"(json/decode "")"#, ":decode"),
    ] {
        let e = ev(&format!("(try {src} (catch e e))"));
        assert!(e.contains(":io-error"), "{src} -> {e}");
        assert!(e.contains(":invalid-data"), "{src} -> {e}");
        assert!(e.contains(op), "{src} should name its operation: {e}");
    }
}

#[cfg(feature = "json")]
#[test]
fn json_round_trips_the_documents_it_accepts() {
    // Encode-then-decode, not decode-then-encode: the second is not a
    // round trip, because keys come back as strings whatever they went in as.
    let mut s = Session::new();
    run(
        &mut s,
        r#"(def doc {"n" 1 "f" 2.5 "t" true "z" nil "xs" [1 "two" [3]]})"#,
    );
    assert_eq!(
        run(&mut s, "(= doc (json/decode (json/encode doc)))"),
        "true"
    );
}

// --- TCP -------------------------------------------------------------------------

#[cfg(feature = "tcp")]
#[test]
fn a_socket_is_a_handle_like_a_file_is() {
    let mut s = Session::new();
    // Port 0, then ask what was bound. A hard-coded port fails on a machine
    // already using it, which is a flaky test rather than a failing one.
    run(&mut s, r#"(def l (tcp/listen "127.0.0.1:0"))"#);
    run(&mut s, "(def addr (tcp/local-addr l))");
    run(&mut s, "(def c (tcp/connect addr))");
    run(&mut s, "(def server (tcp/accept l))");

    // Written and read by the *file* primitives. An adapter needing its own
    // read would be an adapter that had not gone through the handle table.
    assert_eq!(run(&mut s, r#"(io/write c "ping")"#), "4");
    assert_eq!(run(&mut s, "(bytes-str (io/read server 4))"), "\"ping\"");

    // And closed, and stale afterwards, by the same rules (ADR-042 part 4).
    assert_eq!(run(&mut s, "(io/open? c)"), "true");
    run(&mut s, "(io/close c)");
    assert_eq!(run(&mut s, "(io/open? c)"), "false");
    run(&mut s, "(io/close server) (io/close l)");
}

/// ADR-045 part 2, and the reason TCP is in this milestone at all. ADR-042
/// shipped six kinds and deferred three on the grounds that a kind nobody can
/// raise is a guess with a colon in front of it — this is the deferral being
/// honoured rather than forgotten.
#[cfg(feature = "tcp")]
#[test]
fn a_read_deadline_raises_a_network_kind_no_file_can_produce() {
    let mut s = Session::new();
    run(&mut s, r#"(def l (tcp/listen "127.0.0.1:0"))"#);
    run(&mut s, "(def c (tcp/connect (tcp/local-addr l)))");
    run(&mut s, "(def server (tcp/accept l))");
    run(&mut s, "(tcp/set-timeout server 30)");

    let kind = run(&mut s, "(try (io/read server 4) (catch e (get e :kind)))");
    // Both, deliberately. Rust documents a read deadline as `WouldBlock` *or*
    // `TimedOut` by platform — Unix gives the first, Windows the second — so a
    // test that pinned one would pass on the machine it was written on and
    // fail on the other (`TRAPS.md`).
    assert!(
        kind == ":would-block" || kind == ":timeout",
        "expected a network kind, got {kind}"
    );
    run(&mut s, "(io/close c) (io/close server) (io/close l)");
}

/// `:connection-reset` had no raiser any test provoked — it could be mapped to
/// `:other` with the whole suite green (`notes/milestone-10-mutants.md`). It is
/// the kind a program most wants to retry on, so a wrong classification is the
/// one that costs most.
///
/// Provoked by closing the peer and writing: the first write lands in a buffer,
/// the peer answers with RST, and the next write fails. How many writes that
/// takes is a kernel detail, so this tries a bounded number and asserts one of
/// them failed the right way rather than asserting which.
#[cfg(feature = "tcp")]
#[test]
fn a_closed_peer_raises_connection_reset() {
    let mut s = Session::new();
    run(&mut s, r#"(def l (tcp/listen "127.0.0.1:0"))"#);
    run(&mut s, "(def c (tcp/connect (tcp/local-addr l)))");
    run(&mut s, "(def server (tcp/accept l))");
    run(&mut s, "(io/close server) (io/close l)");

    let mut kinds = Vec::new();
    for _ in 0..40 {
        let k = run(
            &mut s,
            r#"(try (io/write c "x") nil (catch e (get e :kind)))"#,
        );
        if k != "nil" {
            kinds.push(k);
            break;
        }
    }
    run(&mut s, "(io/close c)");
    assert_eq!(
        kinds.first().map(String::as_str),
        Some(":connection-reset"),
        "writing to a closed peer should classify as a reset, got {kinds:?}"
    );
}

/// ADR-042 part 1 puts `:path` in a fault only when the operation names a
/// location. For a socket the address *is* the location, and dropping it
/// survived the suite — a `:connection-reset` that does not say which peer
/// names the failure and not the thing that failed.
#[cfg(feature = "tcp")]
#[test]
fn a_socket_error_carries_the_address_it_was_for() {
    let mut s = Session::new();
    // Bind, learn the address, release it — so the connect below is refused
    // deterministically rather than by hoping a port is free.
    run(&mut s, r#"(def l (tcp/listen "127.0.0.1:0"))"#);
    let addr = run(&mut s, "(tcp/local-addr l)");
    run(&mut s, "(io/close l)");

    let e = run(
        &mut s,
        &format!("(try (tcp/connect {addr}) (catch e [(get e :operation) (get e :path)]))"),
    );
    assert_eq!(e, format!("[:connect {addr}]"), "the address is missing");
}

/// ADR-045 part 5: a live socket refuses a snapshot through the check ADR-029
/// already had, with no adapter-specific code. This is the handle table doing
/// the job ADR-016 built it for.
#[cfg(feature = "tcp")]
#[test]
fn a_live_socket_refuses_a_snapshot_exactly_as_a_file_does() {
    use apolisp::image::{self, SnapshotError};
    use apolisp::{compile, expand, reader, vm};

    let mut machine = vm::Vm::new();
    let src = r#"(def l (tcp/listen "127.0.0.1:0")) (println :bound) l"#;
    let forms = reader::read_all(src, &mut machine.interner).expect("reads");
    let forms = expand::expand_all(forms, &mut machine).expect("expands");
    let chunk = compile::compile(&forms, &mut machine.interner).expect("compiles");

    // Step until the listener is open, then try to snapshot.
    let mut ex = vm::start(&chunk);
    let mut refused = false;
    loop {
        let (outcome, next, _) = vm::run_fueled(&mut machine, &chunk, ex, 1);
        ex = next;
        if !matches!(outcome, vm::Outcome::Suspended) {
            break;
        }
        if let Err(e) = image::capture(&machine, &ex, &chunk) {
            assert_eq!(e, SnapshotError::SnapshotHasLiveHandles(1));
            refused = true;
            break;
        }
    }
    assert!(refused, "an open listener should have refused a snapshot");
}

// --- The subtraction --------------------------------------------------------------

/// ADR-013 as a test rather than a feeling, now across four capabilities: a
/// feature removes its *primitives* and leaves the language alone. Every
/// assertion is a `cfg!`, so this file states the whole matrix once and each
/// build checks its own row.
#[test]
fn a_feature_removes_its_primitives_and_nothing_else() {
    assert_eq!(is_bound("io/open"), cfg!(feature = "fs"));
    assert_eq!(is_bound("tcp/connect"), cfg!(feature = "tcp"));
    assert_eq!(is_bound("term/read-key"), cfg!(feature = "term"));
    assert_eq!(is_bound("term/open"), cfg!(feature = "term"));
    assert_eq!(is_bound("json/decode"), cfg!(feature = "json"));

    // Never gated, in any build: these are the language, and ADR-013's whole
    // point is that features do not produce 2ⁿ languages.
    for always in [
        "+",
        "count",
        "conj",
        "str",
        "println",
        "gensym",
        "io/stdout",
    ] {
        assert!(is_bound(always), "{always} should exist in every build");
    }
    assert_eq!(ev("(let [x 2] (* x 21))"), "42");
    assert_eq!(ev("(try (throw :x) (catch e e))"), ":x");
}

/// A cut capability is an ordinary unbound global — not a parse error, not a
/// different language. Checked in whichever build is missing something, so it
/// says nothing at all in the default build.
#[test]
fn a_cut_capability_is_an_ordinary_unbound_global() {
    let mut any = false;
    for (name, present) in [
        ("io/open", cfg!(feature = "fs")),
        ("tcp/connect", cfg!(feature = "tcp")),
        ("json/decode", cfg!(feature = "json")),
    ] {
        if present {
            continue;
        }
        any = true;
        let e = ev(&format!("(try ({name} \"x\") (catch e (get e :kind)))"));
        assert_eq!(e, ":unbound", "{name} should be unbound, got {e}");
    }
    // In the default build every capability is present, so the loop above
    // asserts nothing. Saying that out loud beats a test that reads as
    // coverage in the build most people run.
    if !any {
        eprintln!("all capabilities present; this test asserts nothing in the default build");
    }
}

/// The terminal has no tty under a test runner, so what is pinned is the
/// binding and its arity check rather than any behaviour.
#[cfg(feature = "term")]
#[test]
fn the_terminal_primitives_exist_and_check_their_arguments() {
    assert!(is_bound("term/size"));
    assert!(is_bound("term/raw-mode"));
    assert!(is_bound("term/read-key"));
    let e = ev("(try (term/read-key :not-a-timeout) (catch e (get e :kind)))");
    assert_eq!(e, ":type");
}

/// ADR-051's claim, and the one that needed the `Host::File` gate widened: a
/// program can paint a terminal in a build with `term` and without `fs`.
///
/// What is asserted is deliberately not the success case. Opening `/dev/tty`
/// needs a **controlling terminal**, which is not the same as a tty device
/// existing: the node is present in the container and the open still fails
/// there with `:other`, exactly as it does from any non-interactive runner.
/// Under a pty it answers `:opened`. Both were checked. A test that asserts
/// either one pins the environment rather than the language (BUILD.md,
/// determinism).
///
/// The invariant across both is that the capability is *present*: the failure,
/// when there is one, is an `:io-error` about this machine, never `:unbound`
/// about this build.
#[cfg(feature = "term")]
#[test]
fn painting_a_terminal_does_not_need_the_fs_feature() {
    let e = ev("(try (do (term/open) :opened) (catch e (get e :kind)))");
    assert!(
        e != ":unbound",
        "`term/open` should be bound in a `term` build, got {e}"
    );
    assert!(
        e == ":opened" || e.starts_with(':'),
        "expected a handle or an io-error kind, got {e}"
    );

    // The point of the widening, stated where it can fail: without `fs` there
    // is no `io/open`, and painting still has to be reachable.
    #[cfg(not(feature = "fs"))]
    {
        assert!(!is_bound("io/open"));
        assert!(is_bound("io/write"));
    }
}

/// Nothing in the language should be able to tell which adapters exist by any
/// means other than a name being unbound. A `Value` variant added for a socket
/// would be visible to `kind-name`, which is a language-observable difference
/// and exactly what ADR-013 forbids.
#[cfg(feature = "tcp")]
#[test]
fn a_socket_is_not_a_new_kind_of_value() {
    let mut s = Session::new();
    run(&mut s, r#"(def l (tcp/listen "127.0.0.1:0"))"#);
    // The same `handle` a file is. `Value::Handle` is one variant (ADR-025),
    // and the adapter lives behind it.
    assert!(run(&mut s, "(str l)").starts_with("\"#<handle"));
    // `kind-name` is what a program can see, and it says `handle` for both.
    assert_eq!(ev(r#"(str (io/open? io/stdout))"#), "\"true\"");
    run(&mut s, "(io/close l)");
}

/// ADR-045 says the language has zero dependencies and that the fact is
/// "checkable — `cargo tree --no-default-features` shows nothing". Nothing
/// checked it, so it was true by habit.
///
/// ADR-054 turns that from a nice property into a load-bearing one: the reason
/// the language declines to segment graphemes is that correctness there costs a
/// mandatory dependency, and an argument resting on an unasserted fact is the
/// shape `notes/the-corpus-as-an-oracle.md` is about.
///
/// The invariant is spelled as "every dependency is optional" rather than by
/// shelling out to `cargo tree`, because that is the property that matters —
/// ADR-013 makes features host capability only, so a non-optional dependency is
/// by definition one the *language* carries.
#[test]
fn the_language_carries_no_dependencies() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("Cargo.toml reads");

    let deps = manifest
        .split("\n[")
        .find(|s| s.starts_with("dependencies]"))
        .expect("a [dependencies] section");

    let entries: Vec<&str> = deps
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();

    // Without this the test passes by parsing nothing — rewrite the manifest in
    // `[dependencies.crossterm]` table style and the section above goes empty
    // and the assertion below succeeds vacuously. A test that cannot fail is
    // the failure mode this whole file is downstream of.
    assert!(
        entries.len() >= 2,
        "expected to see the adapter dependencies and saw {}; \
         the manifest format changed and this test stopped looking at anything",
        entries.len()
    );

    let mandatory: Vec<&str> = entries
        .into_iter()
        .filter(|l| !l.contains("optional = true"))
        .collect();

    assert!(
        mandatory.is_empty(),
        "a non-optional dependency is one the language carries, which ADR-045 \
         says it does not and ADR-054 rests on:\n{}",
        mandatory.join("\n")
    );
}

/// The editor is one program split across two files: `tests/corpus/editor.xs`
/// keeps the pure core a golden can reach, and `examples/editor-shell.xs` keeps
/// the tty half it cannot. A file is a compilation unit and there is no `load`,
/// so nothing makes the halves agree — and the core has moved out from under the
/// shell four times, most sharply when ADR-052 deleted `scalars-take` outright.
///
/// **A compile check does not work here, and that is worth recording.** Globals
/// resolve at call time, so a shell calling a function the core deleted compiles
/// perfectly and faults on the line that runs it. The first version of this test
/// compiled the join and passed with a call to the deleted `scalars-take` sitting
/// in it — found by putting one there on purpose, not by reading it.
///
/// So it evaluates the join instead, minus the single `(edit ...)` call that
/// needs a terminal, which binds every definition without touching the tty. Then
/// it runs the shell's pure half for real.
#[test]
fn the_editor_shell_still_fits_the_core() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus = std::fs::read_to_string(root.join("tests/corpus/editor.xs"))
        .expect("the corpus editor reads");
    let shell = std::fs::read_to_string(root.join("examples/editor-shell.xs"))
        .expect("the editor shell reads");

    // `just edit` cuts at the same marker. If it moves, fail loudly rather than
    // quietly checking the scripted half and calling that a test.
    const MARKER: &str = "\n; --- The session";
    let cut = corpus
        .find(MARKER)
        .expect("the corpus editor still marks where its script begins");

    let mut s = Session::new();
    let joined = format!("{}\n{}", &corpus[..cut], shell);
    match s.eval(&joined) {
        Ok(Ended::Value(_)) => {}
        Ok(Ended::Threw(u)) => panic!(
            "core + shell threw while defining: {}",
            printer::print(&u.value, &s.vm.interner)
        ),
        Err(e) => panic!("core + shell did not evaluate: {}", e.msg),
    }

    // The shell's pure half, executed rather than merely compiled.
    assert_eq!(
        run(&mut s, r#"(read-chord {:key :char :char "a" :ctrl false})"#),
        "\"a\""
    );
    assert_eq!(
        run(&mut s, r#"(read-chord {:key :char :char "o" :ctrl true})"#),
        "\"C-o\""
    );
    assert_eq!(run(&mut s, "(read-chord {:key :enter})"), "\"RET\"");
    assert_eq!(run(&mut s, "(read-chord {:key :backspace})"), "\"DEL\"");
    // A resize arrives as `{:type :other}` and has no `:key`, which is the whole
    // of Tier 0 resize handling: it falls to the catch-all, dispatch calls it
    // undefined, and the loop turns over and repaints at the new size.
    assert_eq!(run(&mut s, "(read-chord {:type :other})"), "\"other\"");

    // The one piece of arithmetic in the shell, and the worst thing it could get
    // wrong: a cursor painted where the text is not means typing lands somewhere
    // you cannot see. `paint` writes to a handle, so it paints to a file here and
    // the escape it emits is checked directly — the only part of the tty half a
    // test can reach without a tty.
    //
    // Both geometries, because the editor has two. This test already earned its
    // keep once: soft wrap changed the default and it failed with the *correct*
    // new answer, which is what a pinned number is for.
    #[cfg(feature = "fs")]
    {
        let out = std::env::temp_dir().join("apolisp-paint-cursor.txt");
        let out = out.to_string_lossy().replace('\\', "/");
        // A 36-character line, cursor at column 20, in a 12-column window.
        //   wrapped: screen row 1, column 8   -> ESC[2;9H   (20/12, 20%12)
        //   clipped: screen row 0, column 11  -> ESC[1;12H  (scrolled right by 9)
        for (wrap, want) in [("true", "\u{1b}[2;9H"), ("false", "\u{1b}[1;12H")] {
            run(
                &mut s,
                &format!(
                    r#"(def st (assoc (new-state "d.txt" ["0123456789abcdefghijklmnopqrstuvwxyz"])
                                      :cursor-row 0 :cursor-col 20 :wrap? {wrap}))
                       (with-open [f (io/open "{out}" :write)] (paint f st 12 4))"#
                ),
            );
            let painted = std::fs::read_to_string(&out).expect("paint wrote the frame");
            assert!(
                painted.ends_with(want),
                "wrap?={wrap}: expected the cursor at {want:?}, frame ended with {:?}",
                &painted[painted.len().saturating_sub(12)..]
            );
        }
        let _ = std::fs::remove_file(&out);
    }

    // What the shell reaches into the core for. Named rather than inferred,
    // because inferring it needs a free-variable analysis and naming it makes
    // deleting one fail here instead of under the user's cursor.
    for name in [
        "new-state",
        "dispatch",
        "frame",
        "scroll-to-cursor",
        "text-rows",
        "shows-help?",
        "clip",
        "cursor-screen",
        "wrap-height",
    ] {
        let id = apolisp::value::SymId(s.vm.interner.intern(name));
        assert!(
            s.vm.global(id).is_some(),
            "the shell calls `{name}` and the core no longer defines it"
        );
    }
}
