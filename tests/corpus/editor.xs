; editor.xs — a nano-shaped editor, in the style of legmacs: every function here
; is pure, and nothing in this file touches a terminal.
;
; The odd one out in this corpus. The others each pin one feature and are named
; for it; this one pins **composition** — whether the pieces still hold together
; at a size where a mistake in one can hide behind another. It is also the only
; entry that was a real program first: it runs interactively under a pty, edits
; a file on disk, and the write-up is `docs/notes/the-editor-program.md`.
;
; What is here is the pure core plus a scripted `reduce dispatch`. The 62-line
; shell that opens a tty and reads a file is deliberately absent — it is the part
; no golden can reach, which is the claim the pure/impure split was made to test.
;
; The surface the program has to build before it can start is itself the finding;
; see the header of each section.

; --- The surface the language does not have ----------------------------------
;
; Defined here rather than in the prelude on purpose. Whatever is still
; load-bearing when the editor works is what earns an ADR — the rule that
; produced ADR-046 through ADR-051.

(defmacro defn [name params & body]
  `(def ~name (fn ~name ~params ~@body)))

; Pairs of test/result, like Clojure's. Recursive, the way `with-open` is.
(defmacro cond [& clauses]
  (if (empty? clauses)
    nil
    `(if ~(first clauses)
       ~(first (rest clauses))
       (cond ~@(rest (rest clauses))))))

(defn inc [n] (+ n 1))
(defn dec [n] (- n 1))
(defn max2 [a b] (if (> a b) a b))
(defn min2 [a b] (if (< a b) a b))
(defn clamp [lo hi n] (max2 lo (min2 hi n)))

; --- Strings -----------------------------------------------------------------
;
; The cursor is a *character* column, so every edit needs a character-indexed
; slice. This file used to define one out of `str-scalars` + `take`/`drop` +
; `scalars-str`, which is quadratic in the column because `take` and `drop` are
; prelude `conj` loops. ADR-052 made `str-scalar-slice` native and the whole
; section went away — this program is the reason it exists, and Q34 is the
; measurement that argued for it.

; --- Buffer: pure state -> state ---------------------------------------------

(defn new-state [filename lines]
  {:lines (if (empty? lines) [""] lines)
   :cursor-row 0
   :cursor-col 0
   ; The column the user *meant*. Vertical motion through a short line clamps
   ; `:cursor-col` and would otherwise destroy the intent: down onto a 2-char
   ; line and down again onto a long one used to leave the cursor at 2. See
   ; `restates-goal`.
   :goal-col 0
   :scroll-row 0
   :filename filename
   :modified? false
   :message nil
   :pending nil
   ; On at startup, the way nano is: save and quit are the two chords nobody
   ; guesses. `dispatch` clears `:message` on every key but not this, so it
   ; survives until it is toggled off rather than until the first keystroke.
   :help? true
   :quit? false})

(defn line-count [state] (count (get state :lines)))
(defn line-at [state row] (nth (get state :lines) row))
(defn current-line [state] (line-at state (get state :cursor-row)))
(defn line-len [state row] (str-scalar-len (line-at state row)))

; Vertical motion lands at the goal column, or the end of the target line if it
; is shorter — so the cursor hugs a short line and springs back on a long one.
; This replaced a `clamp-cursor` that clamped against `:cursor-col`, which is the
; value the previous short line had already destroyed.
(defn move-vert [state row]
  (assoc state
         :cursor-row row
         :cursor-col (min2 (get state :goal-col) (line-len state row))))

(defn touch [state]
  (assoc state :modified? true))

; --- Editing -----------------------------------------------------------------

(defn insert-str [state s]
  (let [row (get state :cursor-row)
        col (get state :cursor-col)
        line (current-line state)
        next (str (str-scalar-slice line 0 col) s (str-scalar-slice line col (str-scalar-len line)))]
    (touch (assoc state
                  :lines (assoc (get state :lines) row next)
                  :cursor-col (+ col (str-scalar-len s))))))

(defn split-line [state]
  (let [row (get state :cursor-row)
        col (get state :cursor-col)
        line (current-line state)
        lines (get state :lines)
        before (vec-slice lines 0 row)
        after (vec-slice lines (inc row) (count lines))
        head (str-scalar-slice line 0 col)
        tail (str-scalar-slice line col (str-scalar-len line))]
    (touch (assoc state
                  :lines (vec (concat before [head] [tail] after))
                  :cursor-row (inc row)
                  :cursor-col 0))))

(defn join-prev [state]
  (let [row (get state :cursor-row)
        lines (get state :lines)
        prev (line-at state (dec row))
        merged (str prev (current-line state))]
    (touch (assoc state
                  :lines (vec (concat (vec-slice lines 0 (dec row)) [merged]
                                      (vec-slice lines (inc row) (count lines))))
                  :cursor-row (dec row)
                  :cursor-col (str-scalar-len prev)))))

(defn delete-back [state]
  (let [row (get state :cursor-row)
        col (get state :cursor-col)]
    (cond
      (> col 0)
      (let [line (current-line state)
            next (str (str-scalar-slice line 0 (dec col)) (str-scalar-slice line col (str-scalar-len line)))]
        (touch (assoc state
                      :lines (assoc (get state :lines) row next)
                      :cursor-col (dec col))))
      (> row 0) (join-prev state)
      true state)))

; --- Movement ----------------------------------------------------------------

(defn move-left [state]
  (let [col (get state :cursor-col) row (get state :cursor-row)]
    (cond
      (> col 0) (assoc state :cursor-col (dec col))
      (> row 0) (assoc state :cursor-row (dec row) :cursor-col (line-len state (dec row)))
      true state)))

(defn move-right [state]
  (let [col (get state :cursor-col) row (get state :cursor-row)]
    (cond
      (< col (line-len state row)) (assoc state :cursor-col (inc col))
      (< row (dec (line-count state))) (assoc state :cursor-row (inc row) :cursor-col 0)
      true state)))

(defn move-up [state]
  (if (> (get state :cursor-row) 0)
    (move-vert state (dec (get state :cursor-row)))
    state))

(defn move-down [state]
  (if (< (get state :cursor-row) (dec (line-count state)))
    (move-vert state (inc (get state :cursor-row)))
    state))

(defn line-start [state] (assoc state :cursor-col 0))
(defn line-end [state] (assoc state :cursor-col (line-len state (get state :cursor-row))))

; --- Keymap ------------------------------------------------------------------
;
; legmacs's shape: a chord is a string, and the keymap is a nested map. Lookup
; answers a command keyword, a sub-map (a prefix), or nil. One two-key chord
; (`C-x C-c`) on purpose — with only single keys the pending-prefix path in
; `dispatch` is never taken, and it is half the design.

; Entries are TAGGED — `[:command kw]` or `[:prefix submap]` — rather than
; distinguished by type at lookup. That is not a stylistic choice: the language
; has **no type predicates at all** (no `map?`, `keyword?`, `nil?`, `type`), so
; the only way to ask what a value is would be to `try` an operation that fails
; on the wrong one. Tagging is what legmacs does anyway, and it is better
; design; the gap merely removes the worse option.
(def global-keymap
  {"left" [:command :move-left]  "right" [:command :move-right]
   "up" [:command :move-up]      "down" [:command :move-down]
   "C-a" [:command :line-start]  "C-e" [:command :line-end]
   "RET" [:command :newline]     "DEL" [:command :delete-back]
   "C-o" [:command :save]      "C-g" [:command :toggle-help]
   "C-x" [:prefix {"C-c" [:command :quit] "C-s" [:command :save]}]})

; Which commands restate the goal column. Vertical motion preserves it — that is
; the whole point — and so does anything that does not move the cursor at all:
; `C-g` after a `down` onto a short line must not overwrite the goal with the
; clamped column. Everything expressing a horizontal intent is named here.
;
; Named in one place rather than maintained by each command, for the reason
; `text-rows` exists: a second copy of an invariant is a copy that will disagree.
(def restates-goal
  {:move-left true  :move-right true
   :line-start true :line-end true
   :newline true    :delete-back true})

(defn restate-goal [state] (assoc state :goal-col (get state :cursor-col)))

(defn apply-command [state c]
  (cond
    (= c :move-left) (move-left state)
    (= c :move-right) (move-right state)
    (= c :move-up) (move-up state)
    (= c :move-down) (move-down state)
    (= c :line-start) (line-start state)
    (= c :line-end) (line-end state)
    (= c :newline) (split-line state)
    (= c :delete-back) (delete-back state)
    (= c :quit) (assoc state :quit? true)
    (= c :toggle-help) (assoc state :help? (not (get state :help?)))
    ; `save` is the one command that is not pure, so it does not happen here:
    ; the shell performs it and the core only records the request. That split is
    ; what keeps this whole file testable.
    (= c :save) (assoc state :save-requested? true)
    true (assoc state :message (str "no command " c))))

(defn run-command [state c]
  (let [next (apply-command state c)]
    (if (get restates-goal c) (restate-goal next) next)))

; A chord is printable when it is exactly one character and not a named key.
(defn printable? [chord] (= (str-scalar-len chord) 1))

(defn dispatch [state chord]
  (let [pending (get state :pending)
        table (if (= pending nil) global-keymap pending)
        hit (get table chord)
        state (assoc state :message nil :pending nil)]
    (cond
      (= hit nil)
      ; Typing is a horizontal intent like any other, and it does not go through
      ; `run-command`, so it restates the goal here.
      (if (and (= pending nil) (printable? chord))
        (restate-goal (insert-str state chord))
        (assoc state :message (str chord " is undefined")))
      (= (nth hit 0) :prefix) (assoc state :pending (nth hit 1))
      true (run-command state (nth hit 1)))))

; --- Render: pure state x cols x rows -> string -------------------------------
;
; The whole frame is built as one string and handed to the shell. No escape
; sequence is emitted from here — a golden that contained cursor positioning
; would pin the terminal protocol rather than the editor.

(defn clip [w s] (if (<= (str-scalar-len s) w) s (str-scalar-slice s 0 w)))

; How many rows the buffer gets. `scroll-to-cursor` and `frame` must agree on
; this or the painted cursor lands on the wrong line, so both ask here.
(defn text-rows [state rows]
  (if (get state :help?) (- rows 2) (dec rows)))

; Only the chords that cannot be guessed. Sized to fit 32 columns, because the
; golden renders at 32 and clipping the hint would be a poor advertisement.
(defn help-line [state cols]
  (clip cols "C-o save  C-x C-c quit  C-g hide"))

(defn scroll-to-cursor [state rows]
  (let [row (get state :cursor-row)
        top (get state :scroll-row)
        avail (text-rows state rows)]
    (cond
      (< row top) (assoc state :scroll-row row)
      (>= row (+ top avail)) (assoc state :scroll-row (inc (- row avail)))
      true state)))

(defn status-line [state cols]
  (let [name (if (= (get state :filename) nil) "*scratch*" (get state :filename))
        flag (if (get state :modified?) " *" "")
        pos (str " " (inc (get state :cursor-row)) ":" (inc (get state :cursor-col)))
        msg (if (= (get state :message) nil) "" (str "  " (get state :message)))]
    (clip cols (str name flag pos msg))))

(defn frame [state cols rows]
  (let [state (scroll-to-cursor state rows)
        top (get state :scroll-row)
        page (take (text-rows state rows) (drop top (get state :lines)))
        body (join "\n" (map (fn [l] (clip cols l)) page))
        foot (if (get state :help?)
               (str (help-line state cols) "\n" (status-line state cols))
               (status-line state cols))]
    (str body "\n" foot)))

; --- The session -------------------------------------------------------------
;
; Chosen so each chord reaches a path the others do not: typing, `RET` splitting
; a line, motion within and between lines, `DEL` deleting back, and `C-x C-c` —
; the only two-key chord, and the only thing that exercises the pending-prefix
; branch in `dispatch`.
;
; The second half exists for the goal column, and is the reason the buffer ends
; up with lines of three different lengths. The last two chords walk up from
; column 6 of "longer", through the one-character line "z" — which clamps the
; cursor to column 1 — and back onto "!world". The status line then reads 2:7,
; and would read 2:2 if the goal column were not held. Without those two lines
; being different lengths the invariant is unpinned, which is the whole failure
; mode `notes/the-corpus-as-an-oracle.md` is about.
(def script
  ["h" "e" "l" "l" "o" "RET" "w" "o" "r" "l" "d"
   "C-a" "!" "up" "C-e" "?" "DEL" "down"
   "C-e" "RET" "z" "RET" "l" "o" "n" "g" "e" "r" "up" "up"
   "C-x" "C-c"])
(def final (reduce dispatch (new-state "demo.txt" [""]) script))
(println "--- frame")
(println (frame final 32 6))
(println "--- lines")
(println (str (get final :lines)))
(println (str "quit? " (get final :quit?) "  modified? " (get final :modified?)))
