; The prelude: the language defining its own definition forms.
;
; Compiled into the binary and expanded ahead of every unit. It exists to keep
; ADR-027's promise that `def` and `defmacro` are library macros rather than
; core forms — the expander knows `set-macro!` and nothing else about defining
; things, and the two forms everybody actually types are written here, in the
; language, over `set-macro!` and `set-global!`.
;
; Keep this small. Every line is core language under the ADR-030 budget, and a
; prelude is the easiest place in a project like this for a standard library to
; start growing by accident.

(set-macro! defmacro
  (fn defmacro [name params & body]
    `(set-macro! ~name (fn ~name ~params ~@body))))

(set-macro! def
  (fn def [name value]
    `(set-global! ~name ~value)))

; The conditionals every program writes, as macros over `if` — which is the
; only one of them the core has. `and` and `or` cannot be functions: they have
; to *not evaluate* what they skip.
(defmacro when [test & body]
  `(if ~test (do ~@body) nil))

(defmacro unless [test & body]
  `(if ~test nil (do ~@body)))

; Both yield the *value* that decided them, not a boolean: `and` gives the
; first falsy one and `or` the first truthy one, as in Clojure. `v#` is why
; neither repeats its test — evaluating it twice is where a side effect goes
; missing.
(defmacro and [& xs]
  (if (empty? xs)
    true
    (if (empty? (rest xs))
      (first xs)
      `(let [v# ~(first xs)] (if v# (and ~@(rest xs)) v#)))))

(defmacro or [& xs]
  (if (empty? xs)
    nil
    (if (empty? (rest xs))
      (first xs)
      `(let [v# ~(first xs)] (if v# v# (or ~@(rest xs)))))))

; ADR-016's promise, kept where it said it would be: `with-open` is a macro
; over `try`/`finally`, not a primitive. A primitive that called a language
; closure would re-enter the dispatch loop on the Rust stack, which ADR-004
; forbids (ADR-041 part 6).
;
; It recurses over the bindings rather than taking one, because a `with-open`
; that holds a single resource is one people nest by hand — and hand-nesting is
; the thing it exists to prevent.
;
; Note ADR-028 rule 2: a call in tail position inside the body is *not* a tail
; call, because the cleanup still has to run after it. A loop written inside a
; `with-open` accumulates frames.
(defmacro with-open [bindings & body]
  (if (empty? bindings)
    `(do ~@body)
    `(let [~(first bindings) ~(first (rest bindings))]
       (try
         (with-open ~(rest (rest bindings)) ~@body)
         (finally (io/close ~(first bindings)))))))
