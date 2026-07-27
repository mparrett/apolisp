; Milestone 5: expansion, quasiquote, and gensym (ADR-040).
;
; Every form here is chosen to *distinguish*, not to look representative — the
; milestone-2 lesson. Each one compiles differently under some plausible wrong
; expander, and the `.expanded` golden is where that shows.

; `def` and `defmacro` come from the prelude, in the language, over the two
; forms the core actually has (ADR-027).
(def answer 42)

; A template with an unquote, a splice, and a tail the splice does not swallow.
; A wrong `concat` grouping reorders these and the value still looks plausible.
(defmacro when [test & body]
  `(if ~test (do ~@body) nil))

; Auto-gensym. Both occurrences are one name, and it cannot collide with the
; `v` the caller passes in — which is the whole point, and is what a template
; without `#` would get wrong silently.
(defmacro twice [e]
  `(let [v# ~e] (+ v# v#)))

; A macro that expands into another macro call, so expansion has to reach a
; fixed point rather than run once.
(defmacro unless [test & body]
  `(when (if ~test false true) ~@body))

; Collection templates: a vector without splicing is a direct `vector` call, a
; map is a `hash-map` call, and a spliced vector goes through a list.
(defmacro pair [a b] `[~a ~b])
(defmacro tagged [k v] `{:tag ~k :value ~v})
(defmacro all [& xs] `[:all ~@xs])

; Quoted data is not code: the `when` inside this quote is a list, not a call,
; and an expander that walked into it would rewrite it.
(def quoted-call '(when a b))

(println (when (< 0 answer) :positive))
(println (unless (< answer 0) :not-negative))
(println (twice 21))
(println (pair :a :b) (tagged :k 1) (all 1 2))
(println quoted-call)
