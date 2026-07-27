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
