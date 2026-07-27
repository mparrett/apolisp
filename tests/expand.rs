//! Milestone 5 (BUILD.md): macro expansion, quasiquote, and gensym.
//!
//! The exit condition is "`defmacro` in-language; deterministic output". Both
//! halves are here: the prelude's `defmacro` is exercised by every test that
//! uses one, and determinism is checked by expanding the same source twice in
//! two VMs and comparing — a gensym counter that survived a unit would pass
//! every other test in this file and flap a golden.
//!
//! Nothing here regenerates a golden file.

use apolisp::value::{check_origins, LocatedForm};
use apolisp::{expand, printer, reader, vm};

mod common;
use common::check_goldens;

/// Expand a source string and print the resulting forms, one per line — the
/// same rendering the `.expanded` goldens use.
fn expanded(src: &str) -> Result<String, String> {
    let mut machine = vm::Vm::new();
    let forms =
        reader::read_all(src, &mut machine.interner).map_err(|e| e.render("<test>", src))?;
    let out = expand::expand_all(forms, &mut machine).map_err(|e| e.render("<test>", src))?;
    Ok(out
        .iter()
        .map(|f| printer::print(&f.root, &machine.interner))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn expand_ok(src: &str) -> String {
    expanded(src).unwrap_or_else(|e| panic!("{src:?}: {e}"))
}

fn expand_err(src: &str) -> String {
    expanded(src).expect_err(&format!("{src:?} should not expand"))
}

/// The forms plus their origins, for the tests that care where a node came
/// from.
fn expand_located(src: &str) -> (Vec<LocatedForm>, vm::Vm) {
    let mut machine = vm::Vm::new();
    let forms = reader::read_all(src, &mut machine.interner).expect("reads");
    let out = expand::expand_all(forms, &mut machine).expect("expands");
    (out, machine)
}

// --- Golden snapshots ---------------------------------------------------------

/// Rung 3 (BUILD.md), the phase milestone 5 adds. A program with no macros
/// expands to itself, which is what makes the diff between `.forms` and
/// `.expanded` readable as "what expansion did".
#[test]
fn expanded_snapshots_match() {
    check_goldens("expand", "expanded");
}

// --- ADR-027 / ADR-040: defining things ---------------------------------------

/// The exit condition. `defmacro` is not a form the expander knows — it is a
/// macro in `prelude.xs`, over the one form that is (`set-macro!`), and this is
/// the test that would fail if it were quietly promoted to a built-in.
#[test]
fn defmacro_comes_from_the_prelude_in_the_language() {
    assert_eq!(
        expand_ok("(defmacro one [] `(+ 1 0))\n(one)"),
        "(quote one)\n(+ 1 0)"
    );
    // `def` too, over `set-global!`.
    assert_eq!(expand_ok("(def x 1)"), "(set-global! x 1)");
    // And the expander's own form still works directly, since the prelude is
    // written in terms of it.
    assert_eq!(
        expand_ok("(set-macro! two (fn [] 2))\n(two)"),
        "(quote two)\n2"
    );
}

/// A `set-macro!` has run by the time a later form is expanded, and not before:
/// the top level is a sequence, one phase earlier than ADR-033 says it.
#[test]
fn a_macro_is_available_after_its_definition_and_not_before() {
    // Used before it is defined, `when` is an ordinary call and stays one.
    assert_eq!(
        expand_ok("(when a b)\n(defmacro when [t & b] `(if ~t (do ~@b) nil))"),
        "(when a b)\n(quote when)"
    );
}

// --- ADR-040: quasiquote ------------------------------------------------------

#[test]
fn a_template_lowers_to_the_calls_that_build_it() {
    for (src, want) in [
        // Only symbols need quoting; everything else evaluates to itself.
        ("`(a 1 :k \"s\")", "(list (quote a) 1 :k \"s\")"),
        ("`(a ~b)", "(list (quote a) b)"),
        // Splicing keeps the items on either side of it in order.
        ("`(1 ~@xs 4)", "(concat (list 1) xs (list 4))"),
        ("`(~@xs)", "(concat xs)"),
        // A vector with no splice is a direct `vector` call; with one it goes
        // through a list, because splicing is a list operation.
        ("`[1 ~x]", "(vector 1 x)"),
        ("`[~@xs]", "(vec (concat xs))"),
        ("`{:k ~v}", "(hash-map :k v)"),
        // Nothing inside a quote is a template.
        ("`(quote ~x)", "(list (quote quote) x)"),
    ] {
        assert_eq!(expand_ok(src), want, "{src}");
    }
}

/// The four ways a template can be wrong, each with a position.
#[test]
fn template_errors_say_where() {
    for (src, want) in [
        ("~x", "`unquote` outside a quasiquote"),
        ("~@x", "`unquote-splicing` outside a quasiquote"),
        ("`(a `b)", "quasiquote inside a quasiquote"),
        ("`{~@xs :k}", "map template"),
    ] {
        let err = expand_err(src);
        assert!(err.contains(want), "{src}: expected {want:?}, got {err:?}");
        assert!(err.contains(":1:"), "{src}: no position in {err:?}");
    }
}

// --- ADR-040: gensym ----------------------------------------------------------

/// Auto-gensym is per template, not per expansion — the same rule Clojure has,
/// because the template is lowered once when the macro is defined.
///
/// That is safe for the reason it is safe there: the generated name cannot
/// collide with anything the caller wrote, and a macro nested inside itself
/// shadows in the ordinary way. What it must never do is *leak the caller's
/// name*, which is the assertion below.
#[test]
fn auto_gensym_is_one_fresh_name_per_template() {
    let out = expand_ok("(defmacro twice [e] `(let [v# ~e] (+ v# v#)))\n(twice 21)");
    assert!(out.contains("(let [v__1 21] (+ v__1 v__1))"), "got {out}");

    // Two templates never collide, even for the same written name.
    let out = expand_ok(
        "(defmacro a [e] `(let [v# ~e] v#))\n(defmacro b [e] `(let [v# ~e] v#))\n(a 1)\n(b 2)",
    );
    assert!(out.contains("(let [v__1 1] v__1)"), "got {out}");
    assert!(out.contains("(let [v__2 2] v__2)"), "got {out}");

    // The caller's own `v` is untouched by the macro's.
    assert_eq!(
        expand_ok("(defmacro twice [e] `(let [v# ~e] (+ v# v#)))\n(let [v 1] (twice v))"),
        "(quote twice)\n(let [v 1] (let [v__1 v] (+ v__1 v__1)))"
    );
}

/// Determinism, which is the half of the exit condition a golden cannot check
/// on its own: a counter that survived a compilation unit would still produce
/// *a* name every time, just not the same one.
#[test]
fn expansion_is_deterministic_across_units() {
    let src = "(defmacro t [e] `(let [v# ~e] v#))\n(t 1)\n(t 2)";
    assert_eq!(expand_ok(src), expand_ok(src));

    // Twice through one VM, which is what a REPL will do (milestone 9).
    let mut machine = vm::Vm::new();
    let once = {
        let forms = reader::read_all(src, &mut machine.interner).expect("reads");
        expand::expand_all(forms, &mut machine).expect("expands")
    };
    let twice = {
        let forms = reader::read_all(src, &mut machine.interner).expect("reads");
        expand::expand_all(forms, &mut machine).expect("expands")
    };
    let print = |fs: &[LocatedForm], m: &vm::Vm| {
        fs.iter()
            .map(|f| printer::print(&f.root, &m.interner))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(print(&once, &machine), print(&twice, &machine));
}

#[test]
fn gensym_is_available_to_a_macro_that_computes_a_name() {
    let out = expand_ok("(defmacro t [] (list (quote quote) (gensym \"n\")))\n(t)");
    assert!(out.ends_with("(quote n__1)"), "got {out}");
}

// --- Expansion order and bounds -----------------------------------------------

/// Expansion runs to a fixed point: a macro may expand into a call to another
/// macro, including one defined later than it but before the call site.
#[test]
fn expansion_reaches_a_fixed_point() {
    assert_eq!(
        expand_ok(
            "(defmacro when [t & b] `(if ~t (do ~@b) nil))\n\
             (defmacro unless [t & b] `(when (if ~t false true) ~@b))\n\
             (unless :c :x)"
        ),
        "(quote when)\n(quote unless)\n(if (if :c false true) (do :x) nil)"
    );
}

/// Quoted data is not code. An expander that walked into a `quote` would
/// rewrite a program's data, and the program would still run — which is why
/// this is a test and not a comment.
#[test]
fn a_quote_is_data_and_is_not_expanded() {
    assert_eq!(
        expand_ok("(defmacro when [t & b] `(if ~t (do ~@b) nil))\n(quote (when a b))"),
        "(quote when)\n(quote (when a b))"
    );
}

/// A macro receives *forms*: unevaluated, and unexpanded.
///
/// This is the rule everyone knows and nothing was checking. Expanding a
/// macro's arguments before invoking it left the entire suite green, because
/// every macro in it used its arguments as code — where expanding early and
/// expanding late produce the same answer. It takes a macro that keeps an
/// argument as *data* to tell the two apart.
#[test]
fn a_macro_receives_unexpanded_forms() {
    assert_eq!(
        expand_ok(
            "(defmacro inner [] :expanded)\n             (defmacro outer [x] `(quote ~x))\n             (outer (inner))"
        ),
        "(quote inner)\n(quote outer)\n(quote (inner))"
    );
}

/// ADR-036 gives the expander the forms *it* produces. The reader's bound
/// cannot see them: nothing was read.
#[test]
fn expansion_is_bounded() {
    // A macro that rewrites to itself makes no progress.
    let err = expand_err("(defmacro loop2 [] `(loop2))\n(loop2)");
    assert!(err.contains("rewriting to itself"), "got {err:?}");

    // A macro that nests deeper every time is bounded too, by depth rather
    // than by count.
    let err = expand_err("(defmacro deep [n] `(list ~n (deep ~n)))\n(deep 1)");
    assert!(
        err.contains("nested more than") || err.contains("rewriting to itself"),
        "got {err:?}"
    );
}

/// A macro is language code, so it can fail the way language code fails — and
/// the diagnostic points at the *call site*, which is the position the person
/// reading it can act on.
#[test]
fn a_macro_that_fails_reports_the_call_site() {
    let err = expand_err("(defmacro bad [] (throw :boom))\n(bad)");
    assert!(err.contains("macro `bad` threw :boom"), "got {err:?}");
    assert!(err.contains(":2:"), "the call site, not the body — {err:?}");

    // Arity is the callee's, checked at call time (ADR-033), and reaches the
    // expander as an ordinary fault value (ADR-039).
    let err = expand_err("(defmacro one [x] `(+ ~x 1))\n(one)");
    assert!(err.contains(":kind :arity"), "got {err:?}");

    // A macro must be a function.
    let err = expand_err("(set-macro! nope 42)");
    assert!(err.contains("must be a function"), "got {err:?}");
}

// --- ADR-026: where expanded code says it came from ---------------------------

/// The macro diagnostic ADR-026's verification list asks for, point 4.
///
/// A node the macro built carries `Generated(call site)`; a node it passed
/// through unchanged keeps the `Source` position it was read at. Both halves
/// matter: the first is what makes an error inside a macro point at the call
/// rather than nowhere, and the second is what keeps an error in *your* code
/// pointing at your code even when a macro moved it.
#[test]
fn macro_output_carries_the_call_site_and_keeps_what_it_passed_through() {
    use apolisp::error::SpanOrigin;
    use apolisp::value::Value;

    let src = "(defmacro when [t & b] `(if ~t (do ~@b) nil))\n(when (ready?) (go))";
    let (forms, _vm) = expand_located(src);
    let call = &forms[1];

    // The `if` the macro built has no source text of its own.
    assert!(
        matches!(call.origins.origin, SpanOrigin::Generated(_)),
        "the expansion root should be generated, got {:?}",
        call.origins.origin
    );

    // `(ready?)` is the argument, passed through — it keeps its own position,
    // and that position is where the caller wrote it.
    let test = &call.origins.children[1];
    let span = match test.origin {
        SpanOrigin::Source(s) => s,
        other => panic!("a passed-through argument should keep its source: {other:?}"),
    };
    assert_eq!(&src[span.start as usize..span.end as usize], "(ready?)");

    // The origins still describe the value they travel with, everywhere.
    let mut problems = Vec::new();
    for f in &forms {
        check_origins(&f.root, &f.origins, src, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:?}");

    // And a form nobody touched keeps its source origin end to end.
    let (plain, _) = expand_located("(f 1)");
    assert!(matches!(plain[0].origins.origin, SpanOrigin::Source(_)));
    assert!(matches!(plain[0].root, Value::List(_)));
}

/// Origins survive the *whole* corpus, not just the one program written to
/// exercise them. This is the check that would have caught milestone 1's dead
/// span property if it had existed then.
#[test]
fn expanded_origins_hold_over_the_corpus() {
    for path in common::corpus_files() {
        let src = std::fs::read_to_string(&path).expect("corpus file reads");
        let (forms, _vm) = expand_located(&src);
        let mut problems = Vec::new();
        for f in &forms {
            check_origins(&f.root, &f.origins, &src, &mut problems);
        }
        assert!(problems.is_empty(), "{}: {problems:?}", path.display());
    }
}
