; Every type refusal `prim::wants` composes, pinned.
;
; This suite exists because of a measurement rather than a hunch. When `wants`
; was introduced, mutating its format string flipped 19 rendered messages and
; produced exactly one failure in the whole suite — `tests/vm.rs`, on a
; substring. `ETHOS.md` puts error quality outside the priority order, as the
; feedback loop everything else depends on, and it was checked at one site in
; nineteen.
;
; What is pinned is the *composition*, not nineteen literals for their own sake.
; A refusal names three things — the operation, what it wanted, and what it got
; — and each is a separate way to be wrong, so the assertions below vary the
; kind across sites, catch a helper that reports itself instead of its caller,
; and guard the neighbouring messages a future tidy-up would be tempted to fold
; in.

; A refusal, as its message. Evaluating without throwing yields `:did-not-throw`
; rather than nil, so a primitive that quietly starts accepting bad input fails
; here with a readable `is=` record instead of a confusing one downstream.
(defmacro refused [e]
  `(try (do ~e :did-not-throw) (catch caught# (get caught# :message))))

; --- the nineteen sites, one assertion each -------------------------------
;
; The value passed is a different kind at nearly every site on purpose. If they
; were all keywords, the third slot of the message would be pinned for exactly
; one kind and a `wants` that hardcoded it would pass.

(is= "`count` needs a collection, not a keyword" (refused (count :k)))
(is= "`first` needs a sequence, not a int" (refused (first 1)))
(is= "`rest` needs a sequence, not a bool" (refused (rest true)))
(is= "`get` needs a collection, not a keyword" (refused (get :k 1)))
(is= "`contains?` needs a collection, not a string" (refused (contains? "s" 1)))
(is= "`empty?` needs a collection, not a float" (refused (empty? 1.5)))
(is= "`assoc` needs a map or vector, not a keyword" (refused (assoc :k 1 2)))
; A vector is the interesting refusal here: `assoc` takes one and `dissoc` does
; not, so this pair would survive a helper that shared one `what` between them.
(is= "`dissoc` needs a map, not a vector" (refused (dissoc [1] 1)))
(is= "`cell-get` needs a cell, not a int" (refused (cell-get 1)))
(is= "`bytes-str` needs bytes, not a string" (refused (bytes-str "s")))
(is= "`bytes-len` needs bytes, not a vector" (refused (bytes-len [])))
(is= "`gensym` needs a string or symbol prefix, not a int" (refused (gensym 1)))
(is= "`concat` needs lists or vectors, not a keyword" (refused (concat :k)))
(is= "`vec` needs a list or vector, not a map" (refused (vec {})))
(is= "`nth` needs a sequence, not a keyword" (refused (nth :k 0)))
(is= "`str-byte-len` needs a string, not a int" (refused (str-byte-len 1)))
(is= "`nth` needs an integer index, not a keyword" (refused (nth [1] :k)))
(is= "`keys` needs a map, not a vector" (refused (keys [1])))
(is= "`+` needs a number, not a keyword" (refused (+ 1 :k)))

; --- the kind is read from the value, not fixed at the site ---------------
;
; One operation, three kinds. This is the sharpest single claim about the third
; slot: the assertions above vary op and kind together, so they cannot separate
; "each site names its own kind" from "each site happens to name a kind".

(is= "`first` needs a sequence, not a int" (refused (first 1)))
(is= "`first` needs a sequence, not a map" (refused (first {})))
(is= "`first` needs a sequence, not a string" (refused (first "s")))

; --- a shared helper names its caller, not itself --------------------------
;
; `seq_items`, `string` and `index` are each reached by several primitives and
; take the operation's name as an argument. A helper that reported its own name
; would give one message for all of them, and every assertion above would still
; pass — each names a different op, but only ever one op per helper.

(is= "`nth` needs a sequence, not a keyword" (refused (nth :k 0)))
(is= "`vec-slice` needs a sequence, not a keyword" (refused (vec-slice :k 0 1)))
(is= "`scalars-str` needs a sequence, not a keyword" (refused (scalars-str :k)))

(is= "`str-byte-len` needs a string, not a int" (refused (str-byte-len 1)))
(is= "`parse-number` needs a string, not a int" (refused (parse-number 1)))

(is= "`nth` needs an integer index, not a keyword" (refused (nth [1] :k)))
(is= "`str-slice` needs an integer index, not a keyword" (refused (str-slice "s" :k 1)))

; --- the neighbours, which `wants` deliberately does not compose -----------
;
; Each of these sits next to an absorbed site and reads differently on purpose,
; which makes each one a standing invitation to fold it in for consistency.
; Folding any of them in would delete a decision: the first names the unit
; question ADR-018 and ADR-049 exist to force, and the `compare` pair says two
; different things because they are two different mistakes (ADR-050).

(is= "`count` on a string: say the unit — `str-byte-len` or `str-scalar-len` (ADR-018, ADR-049)"
     (refused (count "text")))
(is= "`nth` needs a non-negative index, not -1" (refused (nth [1] -1)))
(is= "`nth` index 9 of 1" (refused (nth [1] 9)))
(is= "`compare` has no ordering for keywords; it orders numbers and strings (ADR-050)"
     (refused (compare :a :b)))
(is= "`compare` orders numbers with numbers and strings with strings, not a int with a string"
     (refused (compare 1 "s")))
(is= "`quot` by zero" (refused (quot 1 0)))

; --- the refusals are refusals ---------------------------------------------
;
; Every assertion above would also pass if `refused` returned the expected
; string by some route that never threw. These check the macro's own two
; outcomes, so the suite cannot pass while observing nothing.

(is= :did-not-throw (refused (count [1 2])))
(is (throws? (count :k)))
