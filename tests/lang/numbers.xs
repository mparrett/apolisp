; ADR-041 parts 2 and 3: equality and the numeric tower, from inside the
; language. These are the cases the entry argues about, so if the argument was
; wrong this is where it shows.

; --- equality is type-strict across int and float ---------------------------
(is (= 1 1))
(is (not (= 1 1.0)))
(is (= 1.0 1.0))
; `==` is the escape hatch, and it is the only one.
(is (== 1 1.0))
(is (not (== 1 2)))

; IEEE, reached by comparing numbers rather than bit patterns.
(is (not (= ##NaN ##NaN)))
(is (= -0.0 0.0))
(is (= ##Inf ##Inf))

; --- one abstraction, two representations -----------------------------------
(is (= '(1 2) [1 2]))
(is (= [] '()))
(is (not (= [1 2] [2 1])))
(is (= {:a 1 :b 2} {:b 2 :a 1}))
(is (not (= {:a 1} {:a 1 :b 2})))
; Nesting, where a shallow comparison would still look right.
(is (= [[1] {:k [2]}] [[1] {:k [2]}]))
(is (not (= [[1] {:k [2]}] [[1] {:k [3]}])))

; --- arithmetic coerces ------------------------------------------------------
(is= 3 (+ 1 2))
(is= 3.5 (+ 1 2.5))
(is= 1.0 (* 2 0.5))
(is= -1.5 (- 1.5))
(is= 0 (+))
(is= 1 (*))

; `/` is always a float, because there are no ratios.
(is= 3.5 (/ 7 2))
(is= 2.0 (/ 4 2))
(is= 0.5 (/ 2))

; Integer division is spelled differently, and is where zero is an error.
(is= 3 (quot 7 2))
(is= 1 (rem 7 2))
(is (throws? (quot 1 0)))
(is (throws? (rem 1 0)))

; --- the two overflow stories -----------------------------------------------
; Integers throw (ADR-037): a wrapped counter is a wrong answer with no
; diagnostic, and simulators cannot detect it.
(is (throws? (+ 9223372036854775807 1)))
(is (throws? (* 9223372036854775807 2)))
(is= 9223372036854775806 (- 9223372036854775807 1))

; Floats reach infinity, which is IEEE's own out-of-range value: neither wrong
; nor silent, and it prints as itself.
(is= ##Inf (* 1.0e308 10.0))
(is (== ##Inf (/ 1.0 0.0)))

; --- ordering ----------------------------------------------------------------
(is (< 1 1.5))
(is (> 2.5 2))
(is (<= 2 2))
(is (>= 2 2))
; `##NaN` is unordered against everything, so comparing with it is refused
; rather than answered.
(is (throws? (< 1 ##NaN)))

; --- truthiness (`TRAPS.md`) -------------------------------------------------
; Only nil and false are falsy. Every one of these is easy to get wrong.
(is (not (not 0)))
(is (not (not "")))
(is (not (not [])))
(is (not nil))
(is (not false))

; --- parsing a number out of a string (ADR-046) ------------------------------
; The literal grammar and the runtime grammar are one implementation, so these
; assertions double as assertions about what the reader accepts.
(is= 27 (parse-number "27"))
(is= -1 (parse-number "-1"))
(is= 5 (parse-number "+5"))
(is= 1.5 (parse-number "1.5"))
(is= 1.0e10 (parse-number "1e10"))
(is= ##Inf (parse-number "##Inf"))
(is= ##-Inf (parse-number "##-Inf"))
; NaN is unordered against itself (ADR-041), so this is how it says "a NaN".
(is (not (= (parse-number "##NaN") (parse-number "##NaN"))))

; `print` and `parse-number` are inverses over every number, which is the
; property that made the non-finite spellings move into the shared grammar
; rather than stay in the reader.
(is= 1.0 (parse-number (str 1.0)))
(is= -0.0 (parse-number (str -0.0)))
(is= ##Inf (parse-number (str ##Inf)))
(is= 9223372036854775807 (parse-number (str 9223372036854775807)))

; The result is a number, not a string that looks like one.
(is= 42 (+ 1 (parse-number "41")))
(is= 0.5 (/ (parse-number "1") (parse-number "2.0")))

; `nil` for a string that does not look like a number at all. This is the
; common path — a caller handing over whatever a peer sent — and it is a "no",
; not a failure.
(is= nil (parse-number "abc"))
(is= nil (parse-number ""))
(is= nil (parse-number "nil"))
; Strict about surrounding space, and the two sides differ for a reason worth
; knowing: `parse-number` is handed a whole string, while the reader only ever
; sees a token it has already split on whitespace. " 27" does not start like a
; number, so it is a plain "no". "27 " does, so it is a number that turned out
; not to be one — which is the fault case, not the nil case.
(is= nil (parse-number " 27"))
(is (throws? (parse-number "27 ")))

; But a fault for a string that looks like a number and is not one, because
; there the diagnostic is worth more than a second `nil` the caller has to
; guess about.
(is (throws? (parse-number "1abc")))
(is (throws? (parse-number "1.2.3")))
(is (throws? (parse-number "99999999999999999999")))
(is (throws? (parse-number "1e400")))

; Not a string at all is the ordinary type fault every primitive raises.
(is (throws? (parse-number 27)))
(is (throws? (parse-number nil)))
