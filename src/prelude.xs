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
