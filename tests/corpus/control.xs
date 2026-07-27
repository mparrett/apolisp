; `if` with and without an else. A missing else is nil, and takes the `if`'s own
; position because it has no source text of its own.
(if (ready?) (go) (wait))
(if (ready?) (go))

; `do` runs its forms for effect and yields the last.
(do (one) (two) 3)

; A handler and a cleanup. The cleanup is emitted twice — once for the path
; where nothing was thrown, once for the path the VM enters while unwinding
; (ADR-034) — and the calls inside are not tail calls even where they look like
; it, because the frame still owns the handler record (ADR-028 rule 2).
(try
  (throw {:kind :boom})
  (catch e (report e))
  (finally (cleanup)))

(try (risky) (finally (cleanup)))

; Cells are the mutable layer (ADR-020). `set-cell!` yields the value written.
(set-cell! (cell 0) 1)

; The core create-or-rebind operation. `def` is the library macro over it
; (ADR-027); this is the thing it expands to.
(set-global! answer 42)
