; Milestone 4: the paths a cleanup has to run on, and what a failure looks like
; when nothing catches it (ADR-028, ADR-039).
;
; Every cleanup prints. "Exactly once" is then readable off the transcript
; rather than argued about a state machine — a cleanup that ran twice is a
; second line, and one that never ran is a missing line.

; 1. Nothing thrown. The cleanup runs on the way past.
(println :one (try :body (finally (println :cleanup-1))))

; 2. Thrown from the body. The handler binds it and the cleanup still runs.
(println :two (try (throw :from-body) (catch e e) (finally (println :cleanup-2))))

; 3. Thrown from the *handler*. The catch region nests inside the finally region
;    (ADR-034), so a throw out of the handler still runs the cleanup.
(println :three
  (try (try (throw :first) (catch e (throw :second)) (finally (println :cleanup-3)))
       (catch e e)))

; 4. Thrown from the cleanup itself. The cleanup's error wins and the original
;    is retained on it as suppressed, which a `catch` does not see — it binds
;    the value alone (ADR-028 invariant 3, ADR-039 clause 4).
(println :four
  (try (try (throw :original) (finally (throw :from-cleanup)))
       (catch e e)))

; 5. A VM-raised fault is a throw like any other since ADR-039, so a handler
;    catches one and binds the map whose shape that entry fixes.
(println :five (try (no-such-global) (catch e e)))

; 6. Unwinding crosses frames. The throw is four calls below the handler, and
;    `(+ 0 ...)` is what keeps the recursion from being a tail call.
(set-global! deep (fn deep [n] (if (< n 1) (throw :from-the-bottom) (+ 0 (deep (- n 1))))))
(println :six (try (deep 3) (catch e e) (finally (println :cleanup-6))))

; 7. Nothing catches this one. The cleanup runs on the way out and throws in its
;    turn, so the transcript carries the winner, the position it was raised at,
;    and the error it displaced.
(try (throw :uncaught) (finally (println :cleanup-7) (throw :from-cleanup-7)))
