; xgrep PATTERN DIR — print every line in DIR containing PATTERN.
;
; The fifth program written under Q31's practice, and the first that uses
; ADR-058's arguments and ADR-060's listing. Pre-registered in
; `docs/notes/the-grep-program-prediction.md`; scored in `the-grep-program.md`.
;
; A fixed substring, not a regex: `str-index-of` is what the language has, and
; a regex engine is a subsystem rather than a program.

(def pattern (nth *command-line-args* 0))
(def dir (nth *command-line-args* 1))

; `(= nil (str-index-of ...))` rather than a `contains?`, which does not exist.
; The `0` is the offset to start from — ADR-049's third argument.
(def has?
  (fn has? [s sub]
    (not (= nil (str-index-of s sub 0)))))

; ADR-060's `:kind` is the whole reason this is one line. With no type
; predicates in the language, the alternative is opening every entry and
; treating the failure as the answer, which cannot tell a directory apart from
; a file it lacks permission on.
(def files
  (fn files [entries]
    (map (fn [e] (get e :name))
      (filter (fn [e] (= :file (get e :kind))) entries))))

(def read-file
  (fn read-file [path]
    (with-open [f (io/open path :read)]
      (bytes-str (io/read-all f)))))

; A line number is what makes a match findable, so the fold carries one. `split`
; on the whole file first, because there is no way to read a line at a time —
; and that is the line P2 says is the wall.
(def report-lines
  (fn report-lines [name lines]
    (loop [i 0]
      (if (= i (count lines))
        nil
        (do
          (when (has? (nth lines i) pattern)
            (println (str name ":" (+ i 1) ":" (nth lines i))))
          (recur (+ i 1)))))))

(def scan
  (fn scan [name]
    (let [path (str dir "/" name)]
      ; A file that will not read is reported and skipped rather than ending the
      ; run: a grep that dies on the first unreadable entry is a grep that
      ; cannot be pointed at a directory anybody else owns.
      (let [text (try (read-file path) (catch e nil))]
        (if (= nil text)
          (println (str name ": unreadable"))
          (report-lines name (split "\n" text)))))))

; `todo` and not `rest`: a local named `rest` would shadow the primitive for
; the whole body, and the recursive step is the one call that needs it.
(def run
  (fn run [names]
    (loop [todo names]
      (if (empty? todo)
        nil
        (do
          (scan (first todo))
          (recur (rest todo)))))))

(run (files (io/read-dir dir)))
