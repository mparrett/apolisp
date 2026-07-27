; Milestone 3's exit condition, as a program.

; Self-recursion through identity, not capture (ADR-002). The recursive call is
; in tail position, so it reuses the frame (ADR-028) and this runs in constant
; space no matter how large `n` is.
(set-global! factorial
  (fn factorial [n acc]
    (if (< n 2)
      acc
      (factorial (- n 1) (* acc n)))))
(println (factorial 20 1))

; The same shape run long enough that a missing tail call would be obvious: a
; hundred thousand frames is a crash, not a slow program.
(set-global! count-to
  (fn count-to [i limit]
    (if (< i limit)
      (count-to (+ i 1) limit)
      i)))
(println (count-to 0 100000))

; Non-tail recursion, which *does* grow the frame stack. Kept small on purpose —
; the point is that both shapes work, not that this one scales.
(set-global! sum-to
  (fn sum-to [n]
    (if (< n 1)
      0
      (+ n (sum-to (- n 1))))))
(println (sum-to 100))

; A closure over a local, called through a global (ADR-002: captures are copied
; at creation, never referenced).
(set-global! adder (fn [by] (fn [x] (+ x by))))
(println ((adder 40) 2))

; One parameter list, optionally ending in a rest parameter. `others` is an
; empty list when nothing extra was supplied, never nil (ADR-033, E-11).
(set-global! tally (fn [head & others] (list head others)))
(println (tally 1 2 3))
(println (tally 1))
