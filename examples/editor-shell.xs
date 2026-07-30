; examples/editor-shell.xs — the impure half of the editor.
;
; NOT A STANDALONE PROGRAM. It is the 62 lines that `tests/corpus/editor.xs`
; deliberately leaves out: opening a tty, reading keys, painting, saving. The
; corpus keeps the pure core, which is the half a golden can reach; this keeps
; the half it cannot, which is the claim the pure/impure split was made to test
; (`docs/notes/the-editor-program.md`).
;
; A file is a compilation unit and there is no `load`, so running the editor
; means concatenating the two. `just edit FILE` does that, and appends the
; `(edit "FILE")` call — which is also how a program gets a filename in a
; language with no argv.
;
; `the_editor_shell_compiles_against_the_core` compiles the concatenation on
; every run. It cannot execute it, because that needs a terminal the test runner
; does not have, but compiling is enough to catch the thing that actually breaks:
; the core changing under a caller that lives in another file. That has happened
; four times — `scalars-take` and `scalars-drop` were deleted by ADR-052, and
; `text-rows` grew a companion — and each time this file was silently stale.

; --- The shell: the only part that touches the world -------------------------
;
; Everything above is pure. This is deliberately thin, because it is the part
; no golden can reach.

(def esc (scalars-str [27]))
(defn csi [s] (str esc "[" s))
(def clear-home (str (csi "2J") (csi "H")))

; ANSI is 1-indexed; the editor is 0-indexed.
(defn move-to [row col] (csi (str (inc row) ";" (inc col) "H")))

(defn read-chord [k]
  (let [name (get k :key) c (get k :char) ctrl (get k :ctrl)]
    (cond
      (= name :char) (if ctrl (str "C-" c) c)
      (= name :enter) "RET"
      (= name :backspace) "DEL"
      (= name :left) "left"
      (= name :right) "right"
      (= name :up) "up"
      (= name :down) "down"
      (= name :tab) "TAB"
      (= name :esc) "ESC"
      true "other")))

(defn read-lines [path]
  (with-open [f (io/open path :read)]
    (split "\n" (bytes-str (io/read-all f)))))

(defn save! [state]
  (with-open [f (io/open (get state :filename) :write)]
    (io/write f (join "\n" (get state :lines))))
  (assoc state :modified? false :save-requested? false
               :message (str "wrote " (get state :filename))))

(defn paint [tty state cols rows]
  (let [shown (scroll-to-cursor state rows)]
    (io/write tty (str clear-home
                       (join "\r\n" (split "\n" (frame shown cols rows)))
                       (move-to (- (get shown :cursor-row) (get shown :scroll-row))
                                (get shown :cursor-col))))))

(defn edit [path]
  (let [lines (read-lines path)]
    (with-open [tty (term/open)]
      (term/raw-mode true)
      (try
        (loop [state (new-state path lines)]
          ; Asked every iteration rather than once before the loop, which is the
          ; whole of resize handling. A window change wakes the blocked
          ; `term/read-key` by itself — crossterm takes SIGWINCH and delivers it
          ; as an event — so the loop turns over with no keystroke and the next
          ; paint is at the new size. Measured: `read-key` returns on the instant
          ; the window changes, and `term/size` already reports the new figures.
          ;
          ; The resize arrives as `{:type :other}`, so `dispatch` also writes
          ; "other is undefined" to the status line. That is the visible cost of
          ; doing this without touching the adapter, and it is left alone on
          ; purpose: the alternative binds `other` to a no-op and buys tidiness
          ; by making a genuinely unknown key silent too.
          (let [size (term/size)]
            (paint tty state (nth size 0) (nth size 1)))
          (let [next (dispatch state (read-chord (term/read-key nil)))
                next (if (get next :save-requested?) (save! next) next)]
            (if (get next :quit?) nil (recur next))))
        (finally
          (term/raw-mode false)
          (io/write tty clear-home))))))
