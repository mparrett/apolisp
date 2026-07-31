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
   ; The leftmost visible column. Only meaningful with wrap off — wrapped text
   ; has no horizontal overflow, so `scroll-to-cursor` forces this to 0 there.
   :scroll-col 0
   ; Which screen row *within* `:scroll-row` the viewport starts at. A wrapped
   ; line is several screen rows, so a buffer row alone cannot say where the top
   ; of the window is.
   :scroll-sub 0
   ; Soft wrap, on by default. Off gives the clip-and-scroll-horizontally
   ; geometry, which is what you want for data files and long code lines.
   :wrap? true
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

; With wrap on, `up` and `down` move one *screen* row, which inside a long line
; means one window-width along it. Moving a buffer line instead would leap over
; every continuation row, which is what makes an editor feel broken with wrap on.
;
; `:goal-col` is therefore a *screen* column here, not a buffer column — see
; `restate-goal`. The two readings never mix, because every horizontal action
; restates it in whichever one is current.
(defn wrapped-down [state w]
  (let [row (get state :cursor-row)
        line (current-line state)
        sub (quot (get state :cursor-col) w)
        goal (get state :goal-col)]
    (cond
      (< (inc sub) (wrap-height w line))
      (assoc state :cursor-col (min2 (str-scalar-len line) (+ (* (inc sub) w) goal)))
      (< (inc row) (line-count state))
      (let [nl (line-at state (inc row))]
        (assoc state :cursor-row (inc row) :cursor-col (min2 (str-scalar-len nl) goal)))
      true state)))

(defn wrapped-up [state w]
  (let [row (get state :cursor-row)
        line (current-line state)
        sub (quot (get state :cursor-col) w)
        goal (get state :goal-col)]
    (cond
      (> sub 0)
      (assoc state :cursor-col (min2 (str-scalar-len line) (+ (* (dec sub) w) goal)))
      (> row 0)
      (let [pl (line-at state (dec row))
            start (* (dec (wrap-height w pl)) w)]
        (assoc state :cursor-row (dec row) :cursor-col (min2 (str-scalar-len pl) (+ start goal))))
      true state)))

(defn move-up [state w]
  (if (get state :wrap?)
    (wrapped-up state w)
    (if (> (get state :cursor-row) 0)
      (move-vert state (dec (get state :cursor-row)))
      state)))

(defn move-down [state w]
  (if (get state :wrap?)
    (wrapped-down state w)
    (if (< (get state :cursor-row) (dec (line-count state)))
      (move-vert state (inc (get state :cursor-row)))
      state)))

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
   "C-w" [:command :toggle-wrap]
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

(defn restate-goal [state w]
  (assoc state :goal-col
         (if (get state :wrap?) (rem (get state :cursor-col) w) (get state :cursor-col))))

(defn apply-command [state w c]
  (cond
    (= c :move-left) (move-left state)
    (= c :move-right) (move-right state)
    (= c :move-up) (move-up state w)
    (= c :move-down) (move-down state w)
    (= c :line-start) (line-start state)
    (= c :line-end) (line-end state)
    (= c :newline) (split-line state)
    (= c :delete-back) (delete-back state)
    (= c :quit) (assoc state :quit? true)
    (= c :toggle-help) (assoc state :help? (not (get state :help?)))
    ; Both scrolls reset: the pair-scroll means nothing with wrap off and the
    ; horizontal scroll means nothing with it on, so neither survives the switch.
    (= c :toggle-wrap) (assoc state :wrap? (not (get state :wrap?))
                              :scroll-col 0 :scroll-sub 0)
    ; `save` is the one command that is not pure, so it does not happen here:
    ; the shell performs it and the core only records the request. That split is
    ; what keeps this whole file testable.
    (= c :save) (assoc state :save-requested? true)
    true (assoc state :message (str "no command " c))))

(defn run-command [state w c]
  (let [next (apply-command state w c)]
    (if (get restates-goal c) (restate-goal next w) next)))

; A chord is printable when it is exactly one character and not a named key.
(defn printable? [chord] (= (str-scalar-len chord) 1))

; `dispatch` takes the window width because vertical movement depends on it once
; lines wrap. Threading it is deliberate over keeping a `:cols` in the state: a
; movement that silently depended on the last render having happened would be a
; worse thing to debug than a longer signature.
(defn dispatch [state w chord]
  (let [pending (get state :pending)
        table (if (= pending nil) global-keymap pending)
        hit (get table chord)
        state (assoc state :message nil :pending nil)]
    (cond
      (= hit nil)
      ; Typing is a horizontal intent like any other, and it does not go through
      ; `run-command`, so it restates the goal here.
      (if (and (= pending nil) (printable? chord))
        (restate-goal (insert-str state chord) w)
        (assoc state :message (str chord " is undefined")))
      (= (nth hit 0) :prefix) (assoc state :pending (nth hit 1))
      true (run-command state w (nth hit 1)))))

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

; --- Soft wrap ---------------------------------------------------------------
;
; A buffer line becomes one or more *screen* rows. Everything below measures in
; screen rows, because that is what the window is made of; the buffer row is no
; longer a unit the geometry can use.

; One more row than the text strictly needs when the length is an exact multiple
; of the width, because the cursor can sit *after* the last character and that
; position has to exist somewhere. A 12-character line in a 12-column window is
; two screen rows, the second empty — which is what nano does and why.
;
; Without it, a cursor at the end of such a line reports a screen row belonging
; to the *next* buffer line, and `paint` draws it there. The frame would look
; right and the cursor would lie.
(defn wrap-height [w s]
  (inc (quot (str-scalar-len s) w)))

(defn wrap-seg [w s i]
  (let [n (str-scalar-len s)
        from (min2 (* i w) n)
        to (min2 (+ from w) n)]
    (str-scalar-slice s from to)))

; One buffer row's height in screen rows: its wrapped height, or 1 with wrap off.
; Rows past the end answer 1 so the viewport can run off the bottom of the file.
(defn line-height [state w row]
  (if (>= row (line-count state))
    1
    (if (get state :wrap?) (wrap-height w (line-at state row)) 1)))

; Screen rows from one (row, sub) position to another. The walk is bounded by the
; distance between them, not the file, which is why the scroll is stored as a
; pair rather than an absolute screen row.
(defn screen-offset [state w srow ssub crow csub]
  (if (= srow crow)
    (- csub ssub)
    (loop [r (inc srow) acc (- (line-height state w srow) ssub)]
      (if (>= r crow)
        (+ acc csub)
        (recur (inc r) (+ acc (line-height state w r)))))))

; Move a (row, sub) position forward by k screen rows, stopping at the last one.
(defn screen-advance [state w row sub k]
  (loop [r row s sub n k]
    (if (< n 1)
      [r s]
      (let [h (line-height state w r)]
        (cond
          (< (inc s) h) (recur r (inc s) (dec n))
          (< (inc r) (line-count state)) (recur (inc r) 0 (dec n))
          true [r s])))))

; One axis, as a function, so the two axes cannot drift apart the way
; `scroll-to-cursor` and `frame` did over the row height.
(defn scroll-axis [pos start avail]
  (cond
    (< pos start) pos
    (>= pos (+ start avail)) (inc (- pos avail))
    true start))

; Everything visible, decided in one place. `frame` and `paint` each call this
; once and then agree by construction — the alternative is two functions deciding
; what is on screen, which is the bug this file has now produced three times.
;
; Wrap off is two independent axes and the pair-scroll is unused. Wrap on is a
; single axis measured in screen rows, and horizontal scrolling does not exist
; because wrapped text has no horizontal overflow.
(defn scroll-to-cursor [state cols rows]
  (let [w (max2 1 cols)
        avail (text-rows state rows)]
    (if (not (get state :wrap?))
      (assoc state
             :scroll-sub 0
             :scroll-row (scroll-axis (get state :cursor-row) (get state :scroll-row) avail)
             :scroll-col (scroll-axis (get state :cursor-col) (get state :scroll-col) w))
      (let [state (assoc state :scroll-col 0)
            crow (get state :cursor-row)
            csub (quot (get state :cursor-col) w)
            srow (get state :scroll-row)
            ssub (get state :scroll-sub)]
        (if (or (< crow srow) (and (= crow srow) (< csub ssub)))
          ; Above the window: the cursor's own screen row becomes the top.
          (assoc state :scroll-row crow :scroll-sub csub)
          (let [off (screen-offset state w srow ssub crow csub)]
            (if (< off avail)
              state
              (let [adv (screen-advance state w srow ssub (inc (- off avail)))]
                (assoc state :scroll-row (nth adv 0) :scroll-sub (nth adv 1))))))))))

; The visible text, exactly `avail` screen rows of it, blank-padded past the end
; of the buffer. This subsumes the old `pad-rows`: a function that knows how many
; screen rows it owes cannot come up short.
(defn screen-rows [state w avail]
  (loop [r (get state :scroll-row) sub (get state :scroll-sub) out []]
    (if (>= (count out) avail)
      out
      (if (>= r (line-count state))
        (recur r sub (conj out ""))
        (let [line (line-at state r)]
          (if (>= sub (line-height state w r))
            (recur (inc r) 0 out)
            (recur r (inc sub)
                   (conj out (if (get state :wrap?)
                               (wrap-seg w line sub)
                               (window (get state :scroll-col) w line))))))))))

; Where the cursor lands on screen, in both modes. `paint` positions the terminal
; cursor from this, so an error here is a cursor that lies about where typing goes.
(defn cursor-screen [state w]
  (if (get state :wrap?)
    [(screen-offset state w (get state :scroll-row) (get state :scroll-sub)
                    (get state :cursor-row) (quot (get state :cursor-col) w))
     (rem (get state :cursor-col) w)]
    [(- (get state :cursor-row) (get state :scroll-row))
     (- (get state :cursor-col) (get state :scroll-col))]))

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

(defn frame [state cols rows]
  (let [state (scroll-to-cursor state cols rows)
        w (max2 1 cols)
        avail (text-rows state rows)
        body (join "\n" (screen-rows state w avail))
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
; 32 columns, matching the frame the golden renders, because with wrap on the
; width is part of what a keystroke means.
(def final (reduce (fn [st c] (dispatch st 32 c)) (new-state "demo.txt" [""]) script))
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
; `:wrap? false` is not decoration. With wrap on this frame flows "!world" onto a
; second row and the `>` markers this block exists to pin disappear — which it
; did, silently, the moment wrap landed. A pin that does not state its mode is a
; pin that changes meaning when the default does.
(def home (assoc final :cursor-col 0 :goal-col 0 :scroll-col 0 :wrap? false))
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
; Wrap off, for the same reason: wrapped text has no horizontal overflow, so with
; the default this pin reported the cursor OFF-SCREEN at every column past the
; window and was pinning nothing but the absence of the feature.
(def wide-state (assoc (new-state "d.txt" [wide-line "short"]) :wrap? false))
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

; --- Soft wrap ---------------------------------------------------------------
;
; The same 36-character line, wrapped into a 12-column window instead of scrolled
; through one. Three screen rows where the clip geometry showed one.
(def wrapped (assoc (new-state "d.txt" [wide-line "tail"]) :wrap? true))
(println "--- wrapped frame, 12 columns")
(println (frame wrapped 12 8))

; The cursor's screen position, which is the number `paint` uses. Both fields
; must stay inside the window at every column, and the row must advance once per
; window-width along the line — that is what makes it wrap rather than scroll.
(defn wrapped-at [col]
  (let [st (scroll-to-cursor (assoc wrapped :cursor-col col) 12 8)
        at (cursor-screen st 12)]
    (println (str "col " col "  screen-row " (nth at 0) "  screen-col " (nth at 1)
                  (if (and (>= (nth at 1) 0) (< (nth at 1) 12)) "  on-screen" "  OFF-SCREEN")))))
(println "--- wrapped cursor")
(wrapped-at 0)
(wrapped-at 11)
(wrapped-at 12)
(wrapped-at 25)
(wrapped-at 36)

; `down` moves one screen row, so inside a wrapped line it advances by the window
; width and the buffer row does not change. Moving a buffer line here would skip
; the rest of the line, which is the thing that makes wrap feel broken.
(println "--- wrapped down/up move screen rows, not buffer rows")
(defn trace [label st]
  (println (str label "  row " (get st :cursor-row) "  col " (get st :cursor-col))))
; Column 25, deliberately past the first segment: its screen column is 1 and its
; buffer column is 25, so a `:goal-col` that kept the buffer reading would send
; the next `down` somewhere else entirely. Starting at column 3 hides that, since
; there the two readings are the same number — which is how the first version of
; this trace let that mutant live.
(def w0 (restate-goal (assoc wrapped :cursor-row 0 :cursor-col 25) 12))
(trace "start        " w0)
(def w1 (move-down w0 12))
(trace "after down   " w1)
(def w2 (move-down w1 12))
(trace "after down   " w2)
(def w3 (move-down w2 12))
(trace "after down   " w3)
(def w4 (move-down w3 12))
(trace "onto line 2  " w4)
(trace "back up      " (move-up w4 12))

; The same three downs with wrap off walk buffer lines instead, and run out of
; buffer after one. Both geometries, one assertion apart.
(def u0 (restate-goal (assoc wrapped :wrap? false :cursor-row 0 :cursor-col 3) 12))
(trace "unwrapped    " u0)
(trace "after down   " (move-down u0 12))
