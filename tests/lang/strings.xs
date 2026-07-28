; ADR-018 and ADR-041 part 5: strings are not sequences, and every conversion
; between text and bytes is spelled out.
;
; Non-ASCII on purpose throughout. A string API that is only ever tested on
; ASCII is a string API whose units nobody has checked.

(is= "" (str))
(is= "abc" (str "a" "bc"))
; `str` displays rather than prints, so a string inside it loses its quotes and
; a collection keeps its own.
(is= "a1:k" (str "a" 1 :k))
(is= "[1 2]" (str [1 2]))
(is= "nil" (str nil))

; --- units are named, never assumed ------------------------------------------
; `count` refuses a string rather than picking a unit for you.
(is (throws? (count "abc")))
; ADR-049: both names say their unit, because the one that did not was the
; place a wrong assumption cost nothing to make and nothing to notice.
(is= 3 (str-byte-len "abc"))
(is= 6 (str-byte-len "héllo"))
(is= 5 (str-scalar-len "héllo"))
(is= 3 (str-scalar-len "abc"))
; The old spelling of scalar length still agrees, and still allocates.
(is= 5 (count (str-scalars "héllo")))
; Empty is empty in both units.
(is= 0 (str-byte-len ""))
(is= 0 (str-scalar-len ""))

(is= [104 233] (str-scalars "hé"))
(is= "hé" (scalars-str [104 233]))
(is= "" (scalars-str []))
; A scalar value is not any integer: surrogates and out-of-range are refused.
(is (throws? (scalars-str [55296])))
(is (throws? (scalars-str [1114112])))

; --- slicing is by byte, and refuses to split a character --------------------
(is= "hé" (str-slice "héllo" 0 3))
(is= "" (str-slice "abc" 1 1))
(is= "abc" (str-slice "abc" 0 3))
(is (throws? (str-slice "héllo" 0 2)))
(is (throws? (str-slice "abc" 0 9)))
(is (throws? (str-slice "abc" 2 1)))

; --- text and bytes ----------------------------------------------------------
(is= 3 (bytes-len (str-bytes "hé")))
(is= 2 (bytes-len (str-bytes "ab")))
(is= "hé" (bytes-str (str-bytes "hé")))
(is= 0 (bytes-len (str-bytes "")))

; A round trip through bytes is the identity on text, which is the property
; that matters and the one a wrong length unit would break.
(is= "héllo, world" (bytes-str (str-bytes "héllo, world")))
(is= "héllo" (scalars-str (str-scalars "héllo")))

; --- strings are values ------------------------------------------------------
(is (= "abc" "abc"))
(is (not (= "abc" "abd")))
(is (not (= "1" 1)))
(is= "abc" (get {:k "abc"} :k))

; --- searching and splitting (ADR-049) ---------------------------------------
(is= 2 (str-index-of "hello" "ll" 0))
(is= nil (str-index-of "hello" "z" 0))
(is= 0 (str-index-of "hello" "h" 0))
; The offset is where the search starts, so a scan does not re-read what it has
; already passed — which is what keeps `split` linear.
(is= 3 (str-index-of "aXbXc" "X" 2))
(is= nil (str-index-of "aXb" "X" 2))
; An empty needle matches everywhere, so every caller that advanced past a match
; would loop forever. Refused once here rather than guarded in each of them.
(is (throws? (str-index-of "abc" "" 0)))
; Byte offsets, and a start inside a character is an error rather than a guess.
(is (throws? (str-index-of "héllo" "l" 2)))
(is (throws? (str-index-of "abc" "b" 9)))

(is= ["a" "b" "c"] (split "," "a,b,c"))
; Multi-character separators work, which the hand-rolled scalar-comparing
; version in `notes/the-report-program.md` could not do.
(is= ["a" "b" "c"] (split ", " "a, b, c"))
(is= ["abc"] (split "|" "abc"))
(is= [""] (split "," ""))
; Empty fields are kept — a trailing separator means a trailing empty field, and
; dropping it is the caller's decision rather than the split's.
(is= ["a" ""] (split "," "a,"))
(is= ["" "a"] (split "," ",a"))
(is= ["" ""] (split "," ","))
(is= ["josé" "münchen"] (split "," "josé,münchen"))

; --- padding counts characters (ADR-049) -------------------------------------
; The whole point: a column lines up because the library counts scalars, not
; because each caller remembered which unit `str-byte-len` answers in.
(is= "dave    " (pad-right 8 "dave"))
(is= "josé    " (pad-right 8 "josé"))
(is= 8 (str-scalar-len (pad-right 8 "josé")))
(is= 8 (str-scalar-len (pad-right 8 "dave")))
(is= "      42" (pad-left 8 "42"))
(is= 8 (str-scalar-len (pad-left 8 "münchen")))
; Too narrow returns the string rather than truncating: losing data to make a
; column line up is the wrong trade to make silently.
(is= "abcdef" (pad-right 2 "abcdef"))
(is= "abcdef" (pad-left 2 "abcdef"))
