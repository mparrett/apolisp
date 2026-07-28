; The cycle that does not leak.
;
; ADR-003 permits a cell cycle and `TRAPS.md` records that a naive walk over
; one does not terminate. This builds one on purpose — and it comes back
; clean, because ADR-025 made a cell an index into a VM-owned generational
; arena rather than a shared pointer. There is no `Rc` edge here to close a
; loop with, so refcounting has nothing to fail to collect.
;
; Q19 already asserts this ("with cells as arena ids there is no `Rc` cycle to
; leak, so the strongest practical complaint is gone"). Nothing tested it. The
; soak's leak leg is the first thing that has, and it is the reason the claim
; is now evidence rather than a sentence.
;
; So this file is not the check on the check — it is a second subject. What
; validates the leak check itself is a mutation pass, recorded in
; `docs/notes/soak-leak-check.md`, because a leak the language cannot express
; cannot be provoked by a program written in it.

(def c (cell nil))
(set-cell! c c)
(println "cycle built")
