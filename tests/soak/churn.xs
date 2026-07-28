; The soak's leak leg (BUILD.md). Bounded live data, unbounded work.
;
; Every iteration allocates a vector, a map, a string, a closure, some bytes
; and a thrown value, and drops all of them. Nothing it makes is reachable at
; exit, so anything a leak check calls definitely lost here is a real finding
; and not a design decision — which is the distinction that makes this
; different from pointing valgrind at the test suite.
;
; The unwinding leg is deliberate: a handler stack is the most plausible place
; for the machine to keep a frame alive past its scope, and unwinding is the
; one path where the VM drops values without the compiler having said so.

(def build
  (fn build [n acc]
    (if (= n 0) acc (build (- n 1) (conj acc (str "row-" n))))))

(def churn
  (fn churn [n]
    (if (= n 0)
      :done
      (let [xs (build 30 [])
            m (hash-map :xs xs :n n :s (str "iter-" n))
            f (fn [k] (get m k))
            b (str-bytes (str "payload-" n))]
        (do
          (f :xs)
          (bytes-str b)
          (try (throw (hash-map :n n)) (catch e (get e :n)))
          (churn (- n 1)))))))

(println (churn 5000))
