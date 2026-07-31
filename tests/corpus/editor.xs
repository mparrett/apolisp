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
   ; The leftmost visible column. Without it a cursor past the right edge is
   ; painted at a column the terminal clamps, so you type where you cannot look.
   :scroll-col 0
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

; A clipped line ends in `>`, so text running past the right edge is visible
; rather than silently absent. Without it a narrow window and a short file look
; identical, which is how someone testing a resize concluded the editor was not
; repainting when it was.
;
; It marks the status and hint lines too, not just the buffer, because the claim
; is the same in all three places: there is more here than fits.
;
; What it does *not* do is make the text reachable. The cursor may still sit past
; the right edge — `paint` positions it in the editor's coordinates and the
; terminal clamps — so a long line remains hard to edit. That wants horizontal
; scrolling or soft wrap, and this is deliberately neither.
;
; `w` of 1 yields just the marker; `w` of 0 yields nothing, because
; `str-scalar-slice` refuses a negative bound and a zero-width column is not
; worth a special case beyond not crashing.
(defn clip [w s]
  (cond
    (<= (str-scalar-len s) w) s
    (< w 1) ""
    true (str (str-scalar-slice s 0 (dec w)) ">")))

; Whether the hint is actually painted. Asked here rather than read off `:help?`
; at each site, for the reason `text-rows` exists: `text-rows` reserves two rows
; for the hint and `frame` decides whether to draw it, and the moment that answer
; depends on anything beyond `:help?` — here, on the height — those two are back
; to deciding the same question separately. This is the `text-rows` bug one level
; up, and it was found the same way, by something varying that had not varied
; before (`notes/the-editor-program.md`).
(defn shows-help? [state rows]
  (and (get state :help?) (>= rows 3)))

; How many rows the buffer gets. `scroll-to-cursor` and `frame` must agree on
; this or the painted cursor lands on the wrong line, so both ask here.
;
; Never below 1. At zero, `scroll-to-cursor` computes `(inc (- row avail))` and
; puts the scroll *past* the cursor it exists to keep on screen, which breaks
; `scroll-row <= cursor-row` and reaches the terminal as `ESC[0;1H` — row zero in
; a 1-indexed protocol. Nothing caught it because `take` clamps a negative count
; to an empty page (ADR-050) rather than raising, so the frame came out quietly
; wrong. Unreachable until the shell started asking the size every iteration.
(defn text-rows [state rows]
  (max2 1 (if (shows-help? state rows) (- rows 2) (dec rows))))

; Only the chords that cannot be guessed. Sized to fit 32 columns, because the
; golden renders at 32 and clipping the hint would be a poor advertisement.
(defn help-line [state cols]
  (clip cols "C-o save  C-x C-c quit  C-g hide"))

; One axis, as a function, so the two axes cannot drift apart the way
; `scroll-to-cursor` and `frame` did over the row height.
(defn scroll-axis [pos start avail]
  (cond
    (< pos start) pos
    (>= pos (+ start avail)) (inc (- pos avail))
    true start))

; Both axes, in one place. `frame` and `paint` each call this once and then agree
; by construction — the alternative is two functions deciding what is visible,
; which is the bug this file has now produced twice.
(defn scroll-to-cursor [state cols rows]
  (assoc state
         :scroll-row (scroll-axis (get state :cursor-row)
                                  (get state :scroll-row)
                                  (text-rows state rows))
         :scroll-col (scroll-axis (get state :cursor-col)
                                  (get state :scroll-col)
                                  (max2 1 cols))))

; A line as seen through the horizontal window: `left` scalars skipped, `w` wide.
;
; The `<` overwrites the first visible column rather than shifting the text right
; by one. That keeps the screen column of the cursor exactly
; `cursor-col - scroll-col`, and an off-by-one there is a cursor that lies about
; where typing will land — the worst failure this program can have.
(defn window [left w s]
  (let [n (str-scalar-len s)
        vis (clip w (str-scalar-slice s (min2 left n) n))]
    (if (or (= left 0) (< (str-scalar-len vis) 1))
      vis
      (str "<" (str-scalar-slice vis 1 (str-scalar-len vis))))))

(defn status-line [state cols]
  (let [name (if (= (get state :filename) nil) "*scratch*" (get state :filename))
        flag (if (get state :modified?) " *" "")
        pos (str " " (inc (get state :cursor-row)) ":" (inc (get state :cursor-col)))
        msg (if (= (get state :message) nil) "" (str "  " (get state :message)))]
    (clip cols (str name flag pos msg))))

; A buffer shorter than the window is padded with blank lines, so the frame is
; always exactly `rows` tall and the status line sits on the bottom edge.
;
; Without this the footer floated directly under the last line of text and the
; rest of the window was left blank, which is wrong in a way that hid something
; worse: **the status line moving with the window edge is how a person sees that
; a resize was noticed at all.** Pinned to the content instead, a correct repaint
; at a new size is indistinguishable from no repaint, and the resize work looked
; broken to the one person testing it while being measurably fine.
;
; `max2 0` because `repeat` counts up to `n` with `=`, so a negative count does
; not terminate.
(defn pad-rows [n xs]
  (concat xs (repeat (max2 0 (- n (count xs))) "")))

(defn frame [state cols rows]
  (let [state (scroll-to-cursor state cols rows)
        top (get state :scroll-row)
        left (get state :scroll-col)
        avail (text-rows state rows)
        page (pad-rows avail (take avail (drop top (get state :lines))))
        body (join "\n" (map (fn [l] (window left cols l)) page))
        foot (if (shows-help? state rows)
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

; --- The frame at a height that does not fit ---------------------------------
;
; Two rows is one short of what the hint needs, so `shows-help?` gives it up and
; the buffer keeps the row. Pinned because the shell now asks the terminal its
; size every iteration, which made this reachable by dragging a window edge; at
; zero text rows `scroll-to-cursor` put the scroll past the cursor and the
; painted position came out as `ESC[0;1H`, row zero in a 1-indexed protocol.
;
; The three numbers below are the assertion. `scroll-row` must never exceed
; `cursor-row` — that is the invariant the bug broke, and it is the reason this
; block prints state rather than only the frame.
(println "--- frame at 2 rows")
(println (frame final 32 2))

; Three heights, not one. A pin at two rows alone leaves the clamp untested —
; `shows-help?` has already given up the hint by then, so `(dec 2)` is 1 and the
; clamp never fires. Removing it survived the first version of this block, which
; is the failure the corpus note is about, found here by mutating on purpose.
; One row is where `(dec 1)` reaches zero and the invariant used to break.
(defn geometry [rows]
  (let [sq (scroll-to-cursor final 32 rows)]
    (println (str "rows " rows
                  "  text-rows " (text-rows final rows)
                  "  help " (shows-help? final rows)
                  "  scroll-row " (get sq :scroll-row)
                  "  cursor-row " (get sq :cursor-row)))))
(println "--- geometry as the height collapses")
(geometry 3)
(geometry 2)
(geometry 1)

; --- The frame fills a window taller than the buffer -------------------------
;
; Four lines of text in a ten-row window. The blanks are the assertion: without
; them the footer floats under the last line of text and the bottom of the window
; is left as it was, which is how a correct repaint at a new size becomes
; indistinguishable from no repaint at all.
;
; Pinned separately because the frame above cannot see it — at 32x6 the buffer is
; exactly as tall as the space, so no padding happens and the fix is invisible to
; the oracle that exists to catch it.
(println "--- frame at 10 rows, buffer of 4")
(println (frame final 32 10))
(println (str "frame lines " (count (split "\n" (frame final 32 10))) " for 10 rows"))

; --- The frame at a width that cuts ------------------------------------------
;
; Five columns, chosen so the frame shows all three cases at once: "hello" is
; exactly five and must NOT be marked (the `<=` boundary), "longer" and "!world"
; are over and must be, and "z" is short and untouched. Pinned
; separately for the same reason as the tall frame: nothing in the 32-column
; frame above is long enough to clip, so the marker is invisible to the oracle
; that exists to catch it. The hint and status lines are marked too, which is
; the decision worth pinning rather than the mechanism.
; Cursor forced home so the horizontal scroll stays out of it: with the cursor at
; column 6 this frame scrolls right instead, and the `>` markers this block
; exists to pin disappear. Two behaviours in one assertion is one assertion that
; pins neither.
(def home (assoc final :cursor-col 0 :goal-col 0 :scroll-col 0))
(println "--- frame at 5 columns")
(println (frame home 5 6))
(println (str "widths " (str (map (fn [l] (str-scalar-len l)) (split "\n" (frame home 5 6))))
              " (each must be 5 or less)"))

; The degenerate widths, which no terminal produces and `paint` would die on if
; `clip` threw. A zero column is the one that can: `str-scalar-slice` refuses a
; negative bound, so without the guard this is a fault inside the render loop.
(println (str "clip 1 [" (clip 1 "abcdef") "]  clip 0 [" (clip 0 "abcdef") "]"))

; --- Horizontal scrolling ----------------------------------------------------
;
; A 36-character line in a 12-column window. The invariant is the last field:
; the screen column must always land inside 0..11, because `paint` positions the
; cursor there and a cursor outside the window is one the terminal clamps — you
; would be typing at a column you cannot see. That is the failure this exists to
; prevent, and it is why the number is printed rather than the frame alone.
;
; `<` and `>` show which directions hold more text. When the cursor sits at the
; right edge with text beyond it, it lands *on* the `>` — the marker overwrites
; a column rather than shifting the text, which is what keeps the screen column
; equal to `cursor-col - scroll-col` with no correction term.
(def wide-line "0123456789abcdefghijklmnopqrstuvwxyz")
(def wide-state (new-state "d.txt" [wide-line "short"]))
(defn at-col [col]
  (let [st (scroll-to-cursor (assoc wide-state :cursor-col col) 12 4)
        left (get st :scroll-col)
        screen (- col left)]
    (println (str "col " col "  scroll " left "  screen " screen
                  (if (and (>= screen 0) (< screen 12)) "  on-screen" "  OFF-SCREEN")
                  "  [" (window left 12 wide-line) "]"))))
(println "--- horizontal scroll, 36 chars through a 12-column window")
(at-col 0)
(at-col 11)
(at-col 12)
(at-col 20)
(at-col 36)
