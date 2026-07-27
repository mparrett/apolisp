; Milestone 7: the handle table, blocking file and stdio, and `with-open`
; (ADR-016, ADR-042).
;
; `tmp-dir` is not defined here. The runner prepends it (ADR-042 part 5), which
; is why nothing below names a real path — a suite that hard-coded one would be
; testing the machine it ran on.

(def path-a (str tmp-dir "/a.txt"))
(def path-b (str tmp-dir "/b.txt"))

; --- writing and reading back -----------------------------------------------

(with-open [f (io/open path-a :write)]
  (io/write f "hello\n"))

(is= "hello\n"
  (with-open [f (io/open path-a :read)]
    (bytes-str (io/read-all f))))

; `io/write` answers how many bytes it wrote, not how many characters it was
; given. The distinction is the whole of ADR-018: this string is four
; characters and six bytes.
(is= 6
  (with-open [f (io/open path-b :write)]
    (io/write f "aééb")))

; :write truncates and :append does not, which is the only thing the two modes
; disagree about.
(with-open [f (io/open path-a :write)] (io/write f "one"))
(with-open [f (io/open path-a :append)] (io/write f "two"))
(is= "onetwo"
  (with-open [f (io/open path-a :read)] (bytes-str (io/read-all f))))

(with-open [f (io/open path-a :write)] (io/write f "three"))
(is= "three"
  (with-open [f (io/open path-a :read)] (bytes-str (io/read-all f))))

; A short read is a value, not a failure, and end of input is empty bytes —
; which is what lets a read loop end on a test rather than on a caught throw.
(with-open [f (io/open path-a :read)]
  (is= "th" (bytes-str (io/read f 2)))
  (is= "ree" (bytes-str (io/read f 99)))
  (is= 0 (bytes-len (io/read f 99))))

; --- what a failure looks like ----------------------------------------------

(def missing (str tmp-dir "/no-such-file.txt"))

(def open-failure (try (io/open missing :read) (catch e e)))
(is= :io-error (get open-failure :type))
(is= :not-found (get open-failure :kind))
(is= :open (get open-failure :operation))
(is= missing (get open-failure :path))

; ADR-042 part 1: `:path` is present only when the operation names one, so a
; failure that has no path has four keys rather than five with a nil in it.
(def read-failure
  (try
    (let [f (io/open path-a :read)]
      (io/close f)
      (io/read-all f))
    (catch e e)))
(is= :io-error (get read-failure :type))
(is= :closed (get read-failure :kind))
(is= :read (get read-failure :operation))
(is (= nil (get read-failure :path)))

; A misuse is a `:vm-error`, not an `:io-error`. Passing a string where a
; handle belongs is the same class of mistake as `(+ 1 "x")`, and nothing about
; the host was involved.
(is= :vm-error (get (try (io/read-all "not a handle") (catch e e)) :type))
(is= :vm-error (get (try (io/open path-a :sideways) (catch e e)) :type))

; --- the generation, which is the point of a handle -------------------------

; Closing twice succeeds. That is what a correct `with-open` does when the body
; closed explicitly, and erroring here would make the safe idiom the dangerous
; one (ADR-042 part 4).
(def h (io/open path-a :read))
(is (io/open? h))
(io/close h)
(is (not (io/open? h)))
(io/close h)

; A *stale* handle does not. The slot is reused under a bumped generation, so
; the old id now names something it never opened — the aliasing bug ADR-016 put
; a generation in the id to catch. Without it, `stale` and `fresh` would be the
; same handle and this would silently succeed.
(def stale (io/open path-a :read))
(io/close stale)
(def fresh (io/open path-b :read))
(is (not (= stale fresh)))
(is (not (io/open? stale)))
(is (io/open? fresh))
(is (throws? (io/close stale)))
(io/close fresh)

; --- with-open runs its cleanup on every path -------------------------------
;
; The matrix milestone 4 pinned for `finally` in `tests/vm.rs`, asked again
; through the macro that is supposed to be built out of it. A leaked handle is
; the failure ADR-039 gave as the reason faults and throws had to converge, so
; "the cleanup ran" is the assertion, not "the value was right".

; 1. Normal return.
(def h1 (io/open path-a :read))
(is= :body (with-open [f h1] :body))
(is (not (io/open? h1)))

; 2. The body throws.
(def h2 (io/open path-a :read))
(is (throws? (with-open [f h2] (throw :from-body))))
(is (not (io/open? h2)))

; 3. The body raises a *VM fault* rather than throwing explicitly. This is the
;    case ADR-039 was decided on: if an arity error inside the body skipped
;    cleanup, a handle would leak on exactly the failures nobody predicted.
(def h3 (io/open path-a :read))
(is (throws? (with-open [f h3] (no-such-global))))
(is (not (io/open? h3)))

; 4. The body closes the handle itself, and the cleanup closes it again.
;    Idempotence is what keeps this from being an error.
(def h4 (io/open path-a :read))
(is= :done (with-open [f h4] (io/close f) :done))
(is (not (io/open? h4)))

; Several bindings, closed innermost first, and all of them closed when the
; body throws. Hand-nesting is what this exists to prevent, so it has to hold
; for more than one resource.
(def m1 (io/open path-a :read))
(def m2 (io/open path-b :read))
(is (throws? (with-open [x m1 y m2] (throw :boom))))
(is (not (io/open? m1)))
(is (not (io/open? m2)))

; --- stdio -------------------------------------------------------------------

; `io/stdout` is a value, not a call: ADR-038 made a primitive an ordinary
; global, and ADR-042 part 4 keeps it one handle rather than a native minting a
; fresh one per call. It is also the buffered host and not a file descriptor
; (ADR-029), which is why this reaches the transcript exactly as `println` does.
(is= 9 (io/write io/stdout "to-stdout"))
(is (io/open? io/stdout))
(is (io/open? io/stdin))
(is= :vm-error (get (try (io/read-all io/stdout) (catch e e)) :type))
