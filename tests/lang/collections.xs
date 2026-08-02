; ADR-041 parts 1 and 4: the collection surface, and what `nil` means to each
; operation.

; --- counting and emptiness --------------------------------------------------
(is= 0 (count nil))
(is= 0 (count []))
(is= 3 (count [1 2 3]))
(is= 2 (count {:a 1 :b 2}))
(is= 2 (count '(1 2)))
(is (empty? nil))
(is (empty? []))
(is (not (empty? [1])))

; --- reading -----------------------------------------------------------------
(is= nil (first nil))
(is= nil (first []))
(is= 1 (first [1 2]))
(is= '() (rest nil))
(is= '(2 3) (rest [1 2 3]))
; `rest` always yields a list, whatever it was handed.
(is (= '(2) (rest [1 2])))
(is= 2 (nth [1 2 3] 1))
(is (throws? (nth [1] 5)))
(is (throws? (nth [1] -1)))

; `get` is the forgiving one; `nth` is the strict one. That difference is the
; whole reason both exist.
(is= nil (get [1] 5))
(is= :none (get [1] 5 :none))
(is= nil (get nil :k))
(is= 1 (get {:a 1} :a))
(is= nil (get {:a 1} :b))
(is= :none (get {:a 1} :b :none))

; `contains?` asks about *keys*, which for a vector means indices.
(is (contains? {:a 1} :a))
(is (not (contains? {:a 1} 1)))
(is (contains? [:x :y] 1))
(is (not (contains? [:x :y] 2)))
(is (not (contains? nil :k)))

; --- building ----------------------------------------------------------------
; A vector grows at the back and a list at the front, because that is where
; each representation is cheap.
(is= [1 2] (conj [1] 2))
(is= [1 2 3] (conj [1] 2 3))
(is= '(2 1) (conj '(1) 2))
(is= '(1) (conj nil 1))
(is= {:a 1} (conj {} [:a 1]))

(is= {:a 1 :b 2} (assoc {:a 1} :b 2))
(is= {:a 9} (assoc {:a 1} :a 9))
(is= {:k 1} (assoc nil :k 1))
(is= [9 2] (assoc [1 2] 0 9))
(is (throws? (assoc [1] 5 :x)))
(is= {:b 2} (dissoc {:a 1 :b 2} :a))
(is= nil (dissoc nil :a))

; A map never holds two equal keys, whichever way it was built.
(is= 1 (count (assoc {:a 1} :a 2)))
(is= 1 (count (hash-map :a 1 :a 2)))
(is= 2 (get (hash-map :a 1 :a 2) :a))

; Order is not part of a map's identity, but `keys` and `vals` still agree with
; each other pairwise.
(is= '(:a :b) (keys {:a 1 :b 2}))
(is= '(1 2) (vals {:a 1 :b 2}))
(is= '() (keys nil))

; --- the collection an operation builds when handed nil ----------------------
; These four are the whole of ADR-041 part 4, and each one is a place Clojure
; and a plausible alternative disagree.
(is (= '(1) (conj nil 1)))
(is (= {:k 1} (assoc nil :k 1)))
(is (= 0 (count nil)))
(is (= '() (rest nil)))
; And `assoc` with nothing to associate is an arity error, not an empty map —
; `nil` standing in for a collection does not make the operation optional.
(is (throws? (assoc nil)))

; --- copy-on-write is invisible ----------------------------------------------
; `conj` mutates in place when the value is not shared (ADR-041 part 1), so the
; thing to pin is that the original never changes when it *is* shared.
(def base [1 2])
(def grown (conj base 3))
(is= [1 2] base)
(is= [1 2 3] grown)

; The same, through a binding that outlives the call.
(is= [1 2]
  (let [v [1 2]
        _ (conj v 3)]
    v))

; --- literals are calls (ADR-035) --------------------------------------------
(is= [1 2] (vector 1 2))
(is= {:a 1} (hash-map :a 1))
(is= '() (list))
(is= [1 2 3] (vec '(1 2 3)))
(is= '(1 2 3) (concat '(1) [2 3]))
(is= '() (concat))

; --- cells, the mutable layer (ADR-020) --------------------------------------
(def counter (cell 0))
(is= 0 (cell-get counter))
(set-cell! counter 5)
(is= 5 (cell-get counter))
; A cell is compared by identity, never by contents.
(is (not (= (cell 1) (cell 1))))
(is (= counter counter))

; --- `loop`/`recur` (ADR-047) ------------------------------------------------
(is= 55 (loop [i 1 acc 0] (if (> i 10) acc (recur (+ i 1) (+ acc i)))))
(is= :ok (loop [] :ok))

; Constant space. A count this size only completes if `recur` reuses the frame.
(is= 200000 (loop [i 0] (if (= i 200000) i (recur (+ i 1)))))

; The loop is not in the function's tail position here, and `recur` still is in
; the loop's — the two are different questions and only the second binds it.
(is= 6 (+ 1 (loop [i 0] (if (= i 5) i (recur (+ i 1))))))

; Bindings are sequential, as `let`'s are: `b` sees the `a` above it.
(is= [1 1] (loop [a 1 b a] [a b]))

; And they shadow, so the loop's own name wins over an outer one.
(is= 3 (let [a 99] (loop [a 1] (if (= a 3) a (recur (+ a 1))))))

; An inner `loop` takes the `recur` inside it; the outer one is untouched.
(is= 12 (loop [i 0 total 0]
          (if (= i 3)
            total
            (recur (+ i 1) (+ total (loop [j 0 s 0] (if (= j 4) s (recur (+ j 1) (+ s 1)))))))))

; The body is an implicit `do`, so everything but the last form runs for effect.
(is= :done (loop [i 0] (cell 1) (if (= i 2) :done (recur (+ i 1)))))

; A `recur` from a `catch` is allowed, and deliberately unlike Clojure. The VM
; pops a handler record when it dispatches to it, so by the time the catch body
; runs there is no region left to jump out of — `regions` is 0 and the ordinary
; tail-call rule permits it. With a `finally` there *is* a region, and the same
; rule refuses it; that refusal is pinned in `tests/compile.rs`.
(is= 500 (loop [i 0] (try (if (= i 500) i (throw :again)) (catch e (recur (+ i 1))))))
; The handler stack is clean afterwards rather than 500 records deep.
(is= :outer (try (throw :outer) (catch e e)))

; --- the sequence library (ADR-048) ------------------------------------------
; The first prelude functions. That they are callable at all is the point:
; before ADR-048 a closure could not survive from the prelude into a unit.
(is= [1 4 9] (map (fn [x] (* x x)) [1 2 3]))
(is= [] (map (fn [x] x) []))
; Works on a list as well as a vector, and always answers with a vector.
(is= [2 4 6] (map (fn [x] (* x 2)) '(1 2 3)))

(is= [3 4] (filter (fn [x] (> x 2)) [1 2 3 4]))
(is= [] (filter (fn [x] (> x 9)) [1 2 3]))

(is= 6 (reduce + 0 [1 2 3]))
(is= 0 (reduce + 0 []))
; The seed decides the result type, so reduce builds collections too.
(is= [10 20] (reduce (fn [acc x] (conj acc (* x 10))) [] [1 2]))

(is= [0 1 2] (range 3))
(is= [] (range 0))
(is= [:x :x] (repeat 2 :x))
(is= [] (repeat 0 :x))

(is= "a, b, c" (join ", " ["a" "b" "c"]))
(is= "1" (join "-" [1]))
(is= "" (join "-" []))
; The separator goes between, never after — the off-by-one this exists to stop.
(is= "1-2" (join "-" [1 2]))

; They compose, which is the whole reason to have them.
(is= 36 (reduce + 0 (map (fn [x] (* x 2)) (filter (fn [x] (> x 2)) (range 7)))))

; --- take / drop / sort (ADR-050) --------------------------------------------
(is= [1 2] (take 2 [1 2 3 4]))
(is= [3 4] (drop 2 [1 2 3 4]))
; Both clamp rather than raising. Asking for more than there is, or for a
; negative count, is how a caller says "all of it" and "none of it".
(is= [] (take 0 [1 2]))
(is= [1 2] (take 9 [1 2]))
(is= [] (take -1 [1 2]))
(is= [1 2] (drop 0 [1 2]))
(is= [] (drop 9 [1 2]))
(is= [1 2] (drop -1 [1 2]))
; take and drop partition, which is what merge sort rests on.
(is= [1 2 3] (concat (take 1 [1 2 3]) (drop 1 [1 2 3])))
; They have always accepted `concat`'s output, which is a *list*, and returned a
; vector. ADR-053 moved them onto `vec-slice`, so this is the assertion that says
; the primitive had to stay lenient about its input.
(is= [1 2] (take 2 (concat [1 2] [3 4])))
(is= [3 4] (drop 2 (concat [1 2] [3 4])))
; And that they hand back a vector, which the `=` above cannot see either.
(is= "[1 2]" (str (take 2 (concat [1 2] [3 4]))))
(is= "[3 4]" (str (drop 2 (concat [1 2] [3 4]))))

; --- vec-slice (ADR-053) ------------------------------------------------------
; Half-open, like `str-slice` and `str-scalar-slice`. Not `subvec`: Clojure's is
; vector-only and an O(1) view, and this copies and takes a list.
(is= [2 3] (vec-slice [1 2 3 4 5] 1 3))
(is= [1 2 3] (vec-slice [1 2 3] 0 3))
(is= [] (vec-slice [1 2 3] 1 1))
; Either bound may equal the count: that addresses the end.
(is= [] (vec-slice [1 2 3] 3 3))
(is= [] (vec-slice [] 0 0))
(is= [2 3] (vec-slice (concat [1 2] [3 4]) 1 3))
; A list in, a vector out — the conversion is the point, not an accident.
;
; Asserted through `str` because `=` crosses representations (ADR-041), so
; `(is= [1 2] ...)` is true of a list too and cannot see this at all. The first
; version of these three lines used `=` and a mutation that made `vec-slice`
; return a list survived the whole suite: the assertion was right, and about
; something else.
(is= "[1 2]" (str (vec-slice (concat [1 2]) 0 2)))
(is= "[2 3]" (str (vec-slice [1 2 3 4] 1 3)))
(is= "[]" (str (vec-slice [1 2 3] 1 1)))
; Unlike take/drop it raises, because a primitive should refuse a bound it cannot
; honour and let the clamping live in the two functions that promise clamping.
(is (throws? (vec-slice [1 2 3] 0 4)))
(is (throws? (vec-slice [1 2 3] 2 1)))
(is (throws? (vec-slice [1 2 3] 4 4)))
(is (throws? (vec-slice "abc" 0 1)))
; Shared structure is preserved rather than copied element-wise (ADR-021).
(is= [[1 2]] (vec-slice [[1 2] [3 4]] 0 1))

(is= [1 1 2 3] (sort [3 1 2 1]))
(is= [] (sort []))
(is= [1] (sort [1]))
; Strings order by code point, so uppercase sorts before lowercase. That is an
; order, not a collation — no locale is consulted.
(is= ["Apple" "apple" "fig" "pear"] (sort ["pear" "Apple" "apple" "fig"]))
(is= ["a" "bb" "ccc"] (sort-by str-scalar-len ["ccc" "a" "bb"]))
(is= [3 2 1] (sort-with (fn [a b] (> a b)) [1 3 2]))
; Sorting a list answers with a vector, like everything else here.
(is= ["a" "b"] (sort (keys (hash-map "b" 1 "a" 2))))

; Stable: equal keys come out in the order they went in, which is what makes
; sorting by one key and then another compose.
(is= [[0 :a] [0 :b] [1 :x] [1 :y]] (sort-by first [[1 :x] [0 :a] [1 :y] [0 :b]]))
; And the tie that a filter-based selection sort would have dropped entirely.
(is= 4 (count (sort [2 1 2 1])))

(is= -1 (compare 1 2))
(is= 1 (compare 2 1))
(is= 0 (compare 1 1))
; Ordering crosses Int and Float exactly as `<` does, so the two cannot disagree.
(is= 0 (compare 1 1.0))
(is= -1 (compare 1 1.5))
(is= -1 (compare "a" "b"))
(is= 0 (compare "a" "a"))
; Not a total order over every value: an unorderable pair is refused by name.
(is (throws? (compare 1 "a")))
(is (throws? (compare :a :b)))
(is (throws? (compare [1] [2])))
(is (throws? (compare ##NaN 1)))

; --- ADR-061: the dead-slot kill, and the cases where it must not fire -------

; The analysis clears a loop binding at its last read. These pin that it is a
; last read *on every path*, because the failure mode is not a crash: a slot
; cleared too early reads back as `nil`, and the program computes a wrong
; answer with complete confidence.

; The shape the whole entry exists for.
(is= [0 1 2 3] (loop [i 0 out []] (if (= i 4) out (recur (+ i 1) (conj out i)))))

; `filter`'s shape: the accumulator is read on both branches of an `if`, and
; only one of them runs. A read-count analysis refuses this; a branch-aware one
; kills on each path. Getting it wrong drops elements.
(is= [0 2 4] (loop [i 0 out []]
               (if (= i 6)
                 out
                 (recur (+ i 1) (if (= 0 (rem i 2)) (conj out i) out)))))

; Read twice in one argument list. The *first* read must not kill, or the
; second sees nil.
(is= [[0 0] [1 1]] (loop [i 0 out []]
                     (if (= i 2) out (recur (+ i 1) (conj out [i i])))))

; A binding read in a later argument after being read in an earlier one.
(is= 6 (loop [i 0 acc 0] (if (= i 4) acc (recur (+ i 1) (+ acc i)))))

; A closure capturing a loop binding. Captures are a list on the `fn` rather
; than `Core::Local` reads (ADR-002), so an analysis that only walked local
; reads would clear the slot before the closure copied it — and the closure
; would capture nil.
(is= 3 (loop [i 0 acc 0]
         (if (= i 3)
           acc
           (recur (+ i 1) (+ acc ((fn [] 1)))))))
(is= [1 2] (loop [i 0 out []]
             (if (= i 2)
               out
               (recur (+ i 1) (conj out ((fn capture [] (+ i 1))))))))

; Inside a `try`. The analysis refuses to kill anywhere in a handler region,
; because a catch can be entered from any point in its body — so a slot whose
; last textual read is in the body is still live if the catch reads it.
;
; **The body has to throw *after* reading the accumulator**, or this proves
; nothing. The first version of these assertions had a body that always
; succeeded, so the catch never ran, so a mutation removing the guard entirely
; passed them — caught by `just mutate`, which is the whole reason that rung
; exists. The interesting path is the one where the kill has already happened
; and the handler then reads the slot.
(is= [] (loop [i 0 out []]
          (if (= i 1)
            out
            (recur (+ i 1) (try (do (conj out i) (throw :x)) (catch e out))))))
(is= [0] (loop [i 0 out []]
           (if (= i 1)
             out
             (recur (+ i 1)
                    (try (do (conj out 99) (throw :x)) (catch e (conj out i)))))))
(is= [0 1] (loop [i 0 out []]
             (if (= i 2)
               out
               (recur (+ i 1) (try (conj out i) (catch e out))))))
(is= :caught (loop [i 0 out []]
               (if (= i 1)
                 :caught
                 (recur (+ i 1) (try (throw :x) (catch e (conj out i)))))))

; And the library functions that ride on all of it.
(is= [1 2 3] (map (fn [x] (+ x 1)) [0 1 2]))
(is= [0 2 4] (filter (fn [x] (= 0 (rem x 2))) [0 1 2 3 4 5]))
(is= 4 (count (split "," "a,b,c,d")))
