# Checks on prose fail the same way checks on code do

**Not normative.** A record of four errors made on 2026-08-03 while running a
voice pass over the write-ups (ADR-063) and writing `GUIDE.md` (ADR-064). Three
are the same defect this project has been cataloguing since milestone 8, found
somewhere it had not been looked for: the apparatus that reviews prose, rather
than the apparatus that tests code.

`wrong-subject.html` states the pattern — *a true assertion applied to a subject
that quietly moved, where review reads the assertion and nothing reads the
subject.* Every instance below is that, and the fourth is its cousin.

## 1. A pattern is not a rule (35 candidates, 0 real)

The voice rubric prohibits restating, after a colon, the noun a pronoun just
stood for. The detector searched for a pronoun, then a colon, then an article,
and reported **35 violations across 12 pages**. That number was published in a
summary before anyone read the hits.

Reading all 35 found **zero**. A colon introducing the *content* of a claim, or
a list, or an explanation is ordinary punctuation and the rule has no quarrel
with it. The assertion "these are colons preceded by a pronoun" was true of
every hit. The subject — "these are restatements" — was never checked.

The tell was available and ignored: the same detector fired three times on one
page, and all three were obviously fine on sight. One page's worth of reading
would have killed the finding before it was reported.

## 2. The corpus included things nobody wrote

Counts were taken over every `<p>` in the thirteen pages. That set contains a
footer that repeats on all thirteen, and blockquotes holding verbatim `ETHOS.md`
and `ADR.md`. Neither is authored prose, and both were being counted as if a
voice pass could revise them.

Excluding them moved the numbers by up to half — *land* 13→6, *surface* 10→5,
*actually* 19→12 — which turned three "findings" into noise at the rationing
threshold. The measurement was correct about the file. The claim was about the
writing.

**The dangerous half is the quotations**, and it is worse than a counting error:
a revision inside one makes this repository assert that a source said something
it did not, with nothing downstream able to detect it. Six candidates sat inside
quotations. One sat inside inline quotation marks, which a filter that only
knows `<blockquote>` walks straight past — the second subject the first fix did
not cover.

## 3. A name check that could not see macros

`GUIDE.md` claims a list of names exists. The check ran `(println NAME)` for
each and looked for `is not bound`. It reported `when`, `and`, and `or` as
missing.

They are not missing. They are **macros**, so they are not values, so printing
one fails for a reason unrelated to whether it exists — and a minute earlier the
same session had run `(when true :w)` successfully and written it down. The
assertion "this name is not a printable value" was true. The question was "does
this name exist", and the check was the wrong shape for a third of its inputs.

This one was caught in thirty seconds because the output was *implausible*, and
that is the only reason. A check that had reported those three as present would
have been believed without inspection.

## 4. The one that is not a wrong subject

`GUIDE.md` also listed a `hash` native. There is no `hash` native. Nothing
measured it wrongly; the sentence was written from expectation and never run.
That is the ordinary failure the other three dress up as rigor, and it is worth
naming separately: **a measurement applied to the wrong subject and an assertion
nobody measured are different mistakes, and the first one feels like work.**

## What it costs to avoid

Nothing that is not already the rule here.

- **Read the hits before reporting the count.** Reading 35 candidates took a few
  minutes; the count was wrong and had already been stated as a finding.
- **Print the subject, not only the verdict.** ADR-045's rule for the budget
  exclusion is the same rule: the report says *what it measured over*, so a
  reader can see the subject drift. A count with no manifest is a number with no
  referent.
- **Run every example.** Two of four errors would have survived any amount of
  re-reading and died instantly to execution.

The uncomfortable part is that all four happened in one session, by an author
who had just finished reading `wrong-subject.html` closely enough to revise it.
Knowing the pattern is not a defence against it — the defence is the thirty
seconds of looking at what the check actually saw.
