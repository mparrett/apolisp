; What the resolver has to decide about a symbol: a local, a capture, a capture
; of a capture, the running function itself, or a global.
(set-global! nested
  (fn [a]
    (let [b 2]
      (fn [c]
        (fn [d] (list a b c d))))))

; Self-recursion resolves through identity, never through a capture (ADR-002),
; and in tail position it is a tail call (ADR-028).
(set-global! count-down
  (fn count-down [n acc]
    (if (< n 1)
      acc
      (count-down (- n 1) (+ acc n)))))

; One parameter list, optionally ending in one rest parameter (ADR-033 rule 3).
; `others` is an empty list when nothing extra was supplied, never nil.
(fn [head & others] others)
