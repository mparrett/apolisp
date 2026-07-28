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
