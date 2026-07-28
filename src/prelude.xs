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

; --- Sequences ---------------------------------------------------------------
;
; The first prelude *functions*, and the thing Q29 was open about. They are
; compiled into every unit's chunk *after* the unit's own protos (ADR-048), so
; adding one here never moves a program's proto indices and never touches a
; `.disasm` golden. That property is what made these affordable; before it, four
; functions cost 160 golden lines in each of nine corpus programs.
;
; Keep this small, for the reason the rest of this file is small: every line is
; core language under ADR-030.
;
; They take and return vectors. There is no laziness in this language, and
; `conj` puts a value where a vector is cheap to extend — so a seq abstraction
; would buy nothing here but a second collection to explain.

(def map
  (fn map [f xs]
    (loop [in xs out []]
      (if (empty? in) out (recur (rest in) (conj out (f (first in))))))))

(def filter
  (fn filter [p xs]
    (loop [in xs out []]
      (if (empty? in)
        out
        (recur (rest in) (if (p (first in)) (conj out (first in)) out))))))

; Three arguments, always. Clojure's one-argument-init form takes the first
; element as the seed and errors on empty, which is two behaviours behind one
; name — and the explicit seed is what makes the empty case obvious.
(def reduce
  (fn reduce [f init xs]
    (loop [in xs acc init]
      (if (empty? in) acc (recur (rest in) (f acc (first in)))))))

(def range
  (fn range [n]
    (loop [i 0 out []] (if (= i n) out (recur (+ i 1) (conj out i))))))

(def repeat
  (fn repeat [n x]
    (loop [i 0 out []] (if (= i n) out (recur (+ i 1) (conj out x))))))

; `str` over a collection, with a separator between rather than after — the
; off-by-one every program writing this by hand gets wrong once.
(def join
  (fn join [sep xs]
    (if (empty? xs)
      ""
      (loop [in (rest xs) out (str (first xs))]
        (if (empty? in) out (recur (rest in) (str out sep (first in))))))))

; --- Strings (ADR-049) -------------------------------------------------------
;
; `split` and the two padders, written here because they are the three every
; text program defines before it can start. The padders use `str-scalar-len`
; and not `str-byte-len`, which is the whole point: a column lines up because
; the library counts characters, not because each caller remembered to.

(def split
  (fn split [sep s]
    (loop [from 0 out []]
      (let [hit (str-index-of s sep from)]
        (if (= hit nil)
          (conj out (str-slice s from (str-byte-len s)))
          (recur (+ hit (str-byte-len sep)) (conj out (str-slice s from hit))))))))

(def pad-right
  (fn pad-right [width s]
    (let [n (- width (str-scalar-len s))]
      (if (< n 1) s (str s (join "" (repeat n " ")))))))

(def pad-left
  (fn pad-left [width s]
    (let [n (- width (str-scalar-len s))]
      (if (< n 1) s (str (join "" (repeat n " ")) s)))))
