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
(is= 3 (str-len "abc"))
(is= 6 (str-len "héllo"))
(is= 5 (count (str-scalars "héllo")))

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
; that matters and the one a wrong `str-len` unit would break.
(is= "héllo, world" (bytes-str (str-bytes "héllo, world")))
(is= "héllo" (scalars-str (str-scalars "héllo")))

; --- strings are values ------------------------------------------------------
(is (= "abc" "abc"))
(is (not (= "abc" "abd")))
(is (not (= "1" 1)))
(is= "abc" (get {:k "abc"} :k))
