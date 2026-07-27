; Rung 4 of the oracle (BUILD.md): the test suite written in the language
; itself, so it survives implementation churn and doubles as a dogfooding pass.
;
; This file is concatenated ahead of each test file by `tests/lang.rs`, because
; there is no `require` and one namespace (ADR-027, Q12). That the harness has
; to be pasted in is the clearest statement of what a module system would buy.
;
; A failing assertion throws, which ends the run with exit 1 and puts the
; failing form in the `--- threw` transcript. Stopping at the first failure is
; deliberate: with no counters and no runner, the thing that fails is the thing
; you read.

(defmacro is [e]
  `(if ~e nil (throw (list :assertion-failed (quote ~e)))))

; `is=` exists because `(is (= a b))` reports only that something was not equal.
; Both values go in the thrown record, which is what makes a failure readable
; without re-running anything.
(defmacro is= [expected actual]
  `(let [want# ~expected got# ~actual]
     (if (= want# got#)
       nil
       (throw (list :assertion-failed (quote ~actual) :expected want# :actual got#)))))

; A thrown value is any value (ADR-039), so catching one is how a test says
; "this should fail" without the harness knowing anything about faults.
(defmacro throws? [e]
  `(try ~e false (catch caught# true)))
