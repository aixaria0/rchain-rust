#!/usr/bin/env bash
# check-wargame — the wargame section's completeness claim, checked against the page.
#
# Why this exists. The wargame section in `docs/src/spec/adversary.md` claims that every study node
# dispositioned all eleven guide words, that every Table A row carries a decided verdict, a persona and
# a falsifier, and that the referee's calibration is reported. Those are claims *about a document*, and
# this repository's rule is that prose is not a check — a claim about a document belongs in a check that
# reads it. `tools/check-hazop-worksheet.sh` is the sibling this mirrors.
#
# The format contract:
#
#   ### W-N<n> — <name>
#   **Design intent.** <one sentence>
#   **Census row tested.** <the adversary.md census class, or "— (new surface)">
#   **Guide words.** row: NO, MORE · folded: LATE -> NO · vacuous: REVERSE (a reason) · owed: EARLY
#   | ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
#   | **W-N4-01** | Mallory | NO | ... | unauth-actuation | inside | local · default | CONFIRMED | — | casper/src/x.rs::a_test |
#
#   **Calibration.** controls caught: C of P · register-leaks: L · vacuity: V%
#
# **The header is the contract, and it is read rather than assumed.** Every cell below is looked up by
# *column name* against the page's own Table A header, and a row whose cell count disagrees with that
# header is refused. Both halves are repairs (2026-10-14): the falsifier is the header's tenth column,
# and this check used to read position 11 and begin with `if (NF != 12) next` — which **skipped a
# malformed row in silence instead of refusing it**. Three rows of this page sat in that blind spot
# (W-N2-06, W-N2-09, W-N4-02, each carrying a "not armed" note as an undeclared eleventh cell): their
# falsifiers were never read, their guide words registered only because a neighbour carried the same
# word, and either row could have been emptied without the gate saying a word.
#
# Checks:
#   1  every node dispositions all eleven guide words exactly once (row / folded / vacuous / owed)
#   2  every word declared a `row` carries at least one Table A row, and no row carries a word the node
#      did not declare as a row
#      **`py` is in the extension list because this repository's falsifiers are not all Rust or shell.**
#      `tests/reconcile-regression.py` drives the reconcile tool's sections offline, and two of the wargame
#      rows name a test in it; a list that could not resolve those would push a citation to the *guard* the
#      test drives instead, which is a weaker claim than the one that exists. The rule the list serves —
#      the path exists and the symbol after `::` occurs in it — is unchanged.
#   3  the page has a Table A header naming the ten columns, and every Table A row has exactly the cell
#      count that header declares — a row of any other shape is a violation, never a skip (see above)
#   4  every Table A row names a known persona, an in-model value (inside/outside), a decided verdict
#      (CONFIRMED / NOT-RE-ESTABLISHED / REFUTED / UNSTATABLE), a disposition that is `—` or R1..R4, and a
#      falsifier that is either an `owed:` marker or a path **and symbol** that exist in this tree
#   5  the page declares its referee calibration and the declaration is well-formed (caught <= planted,
#      planted >= 1) — a wargame that reports no calibration reports no verdict column
#   6  the scan is not vacuous: at least one node, one Table A header and one Table A row were found
#
# Usage: tools/check-wargame.sh [--gate] [<page>]
#        --gate exits non-zero on a violation (for CI). Without it the report is printed and the exit is 0.
#        The default page is docs/src/spec/adversary.md.
set -euo pipefail

gate=0
page=""
for arg in "$@"; do
  case "$arg" in
    --gate) gate=1 ;;
    -h|--help) sed -n '2,49p' "$0"; exit 0 ;;
    *) page="$arg" ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
page="${page:-$root/docs/src/spec/adversary.md}"
[[ -r "$page" ]] || { echo "check-wargame: no page at $page" >&2; exit 2; }

cal="$(grep -m1 '\*\*Calibration\.\*\*' "$page" || true)"
if [[ -z "$cal" ]]; then
  echo "check-wargame: $page declares no calibration line." >&2
  echo "  expected: **Calibration.** controls caught: C of P · register-leaks: L · vacuity: V%" >&2
  exit 1
fi
declared_caught="$(printf '%s' "$cal" | sed -n 's/.*controls caught: \([0-9]\+\).*/\1/p')"
declared_planted="$(printf '%s' "$cal" | sed -n 's/.*of \([0-9]\+\).*/\1/p')"
declared_caught="${declared_caught:--1}"
declared_planted="${declared_planted:--1}"

echo "check-wargame: $page"

set +e
sed -e 's/→/->/g' -e 's/·/@SEP@/g' "$page" \
  | awk -v root="$root" -v dc="$declared_caught" -v dp="$declared_planted" '
    BEGIN {
      FS = "|"
      split("NO,MORE,LESS,AS WELL AS,PART OF,OTHER THAN,REVERSE,EARLY,LATE,BEFORE,AFTER", K, ",")
      for (i = 1; i <= length(K); i++) known[K[i]] = 1
      split("Alice,Bob,Eve,Mallory,Oscar,Root,Operator", P, ",")
      for (i = 1; i <= length(P); i++) persona[P[i]] = 1
      split("CONFIRMED,NOT-RE-ESTABLISHED,REFUTED,UNSTATABLE", V, ",")
      for (i = 1; i <= length(V); i++) verdict[V[i]] = 1
      # The columns the checks below read, by name. `Deviation` and `Class` are not validated here
      # (they are prose and a census class the page owns in its own prose) but they are part of the
      # header this check insists on, because a header that lost one of them is a table this check
      # is no longer reading what it thinks it is.
      n_required = split("ID,Persona,Word,Deviation,Class,In-model,Precondition,Verdict,Disposition,Falsifier", required, ",")
      nodes = 0; rows = 0; unread = 0; tables = 0; hdr_nf = 0; header_names = ""
      bad = 0; node = ""; have_gw = 0
    }
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    function norm(s,   t) {
      t = s; gsub(/\([^)]*\)/, "", t); t = toupper(t)
      gsub(/\/NOT/, "", t); gsub(/\*/, "", t); gsub(/[ \t]+/, " ", t); return trim(t)
    }
    function problem(msg) { printf "  VIOLATION  %s\n", msg; bad++ }
    function addwords(payload, kind,   n, it, w, i) {
      gsub(/\([^)]*\)/, "", payload)
      n = split(payload, it, ",")
      for (i = 1; i <= n; i++) {
        w = it[i]
        if (kind == "folded") sub(/->.*$/, "", w)
        w = norm(w); if (w == "") continue
        if (!(w in known)) { problem(node ": unknown guide word [" w "] under " kind); continue }
        seen[w]++
        if (kind == "row") rowset[w] = 1
      }
    }
    function flush(   i, w, c) {
      if (node == "") return
      nodes++
      if (!have_gw) problem(node ": no **Guide words.** line")
      else for (i = 1; i <= length(K); i++) {
        w = K[i]; c = (w in seen) ? seen[w] : 0
        if (c == 0) problem(node ": guide word " w " is dispositioned nowhere (row / folded / vacuous / owed)")
        else if (c > 1) problem(node ": guide word " w " is dispositioned " c " times")
      }
      for (w in rowset) if (!(w in rowseen)) problem(node ": word " w " is declared a row but no Table A row carries it")
      delete seen; delete rowset; delete rowseen; have_gw = 0
    }
    /^### W-N[0-9]+/ { flush(); s = $0; sub(/^### /, "", s); split(s, a, " "); node = a[1]; next }
    /\*\*Guide words\.\*\*/ {
      have_gw = 1
      s = $0; sub(/^.*\*\*Guide words\.\*\*[ \t]*/, "", s)
      n = split(s, seg, "@SEP@")
      for (i = 1; i <= n; i++) {
        t = trim(seg[i]); if (t == "") continue
        if (match(t, /^(row|folded|vacuous|owed):/)) addwords(substr(t, RSTART + RLENGTH), substr(t, RSTART, RLENGTH - 1))
        else problem(node ": guide-word segment is not row:/folded:/vacuous:/owed: -- [" t "]")
      }
      next
    }
    # **The header the page writes is the authority on its columns, and every field below is read by
    # name rather than by position.** Both halves of that are repairs (2026-10-14). The position is
    # exactly what drifts: the falsifier is the tenth column of the header, and this block used to
    # begin `if (NF != 12) next` -- so a row carrying an eleventh cell was **skipped in silence**,
    # not refused. Three rows of this page were in that blind spot (W-N2-06, W-N2-09, W-N4-02, each
    # with a "not armed" note as an undeclared cell): their falsifiers were never read, their guide
    # word counted only because a neighbouring row carried it, and the rows could have been emptied
    # entirely without the gate saying a word. A wrong cell count is a violation now, and the count
    # it is compared against comes from the header rather than from a constant here.
    /^\|/ {
      if (trim($2) == "ID" && trim($3) == "Persona") {
        tables++; hdr_cells = NF - (trim($NF) == "" ? 2 : 1); delete col
        for (i = 2; i < NF; i++) { c = trim($i); if (c != "") col[c] = i }
        for (i = 1; i <= n_required; i++) {
          c = required[i]
          if (!(c in col)) problem("the Table A header has no `" c "` column (found: " header_names ")")
        }
        header_names = ""
        for (i = 2; i < NF; i++) header_names = header_names (i > 2 ? ", " : "") trim($i)
        next
      }
      if ($0 !~ /^\| *\*\*W-N/) next
      id = $2; gsub(/[^A-Za-z0-9-]/, "", id)
      if (id !~ /^W-N[0-9]+-[0-9]+$/) {
        problem("a Table A row is named [" trim($2) "], which is not a W-N<n>-<n> id")
        next
      }
      if (node == "") { problem("Table A row " id " appears before any node heading"); next }
      rows++
      if (tables == 0) { problem(node " " id ": a Table A row appears before any Table A header"); next }
      # **Cells, not fields, and the difference is not pedantry.** A row written `| … | owed: … | — not
      # armed: … ` — one cell too many, closing delimiter dropped — has the *same* field count as the
      # header, because the missing delimiter and the extra cell cancel. That is exactly how the three
      # rows this check could not see were written, so a count of `$NF - 2` would have passed them
      # again; the cell count is the field count less the delimiters the row actually carries.
      cells = NF - (trim($NF) == "" ? 2 : 1)
      if (cells != hdr_cells) {
        unread++
        problem(node " " id ": " cells " cells where the header declares " hdr_cells " -- an extra \
or missing cell makes the row unreadable, so it is refused rather than skipped")
        next
      }
      pr = trim($(col["Persona"])); if (!(pr in persona)) problem(node " " id ": unknown persona [" pr "]")
      w = norm($(col["Word"]))
      if (w == "") problem(node " " id ": empty guide word")
      else if (!(w in known)) problem(node " " id ": unknown guide word [" w "]")
      else if (w in rowset) rowseen[w] = 1
      else problem(node " " id ": guide word " w " is not declared a `row` for this node")
      im = trim($(col["In-model"])); if (im != "inside" && im != "outside") problem(node " " id ": in-model [" im "] is not inside/outside")
      prec = trim($(col["Precondition"])); if (prec == "" || prec == "—") problem(node " " id ": no precondition -- a row without a context is not a finding")
      vd = trim($(col["Verdict"])); if (!(vd in verdict)) problem(node " " id ": verdict [" vd "] is not CONFIRMED/NOT-RE-ESTABLISHED/REFUTED/UNSTATABLE")
      disp = trim($(col["Disposition"])); dtok = disp; sub(/[ \t(].*$/, "", dtok)
      if (dtok != "—" && dtok != "-" && dtok !~ /^R[1-4]$/) problem(node " " id ": disposition [" disp "] is not `—` or a rung R1..R4")
      fals = trim($(col["Falsifier"]))
      if (fals == "") { problem(node " " id ": empty falsifier"); next }
      if (fals ~ /^owed:/) next
      okp = 0; oksym = 0; anycol = 0; lastp = ""; lastsym = ""
      m = fals
      while (match(m, /[A-Za-z0-9_.\/-]+\.(rs|py|lean|md|sh|tsv|txt|json|toml|v)/)) {
        p = substr(m, RSTART, RLENGTH); m = substr(m, RSTART + RLENGTH); sub(/:.*$/, "", p)
        if (system("test -e \"" root "/" p "\"") != 0) continue
        okp = 1; lastp = p
        if (substr(m, 1, 2) == "::") {
          anycol = 1
          if (match(substr(m, 3), /^[A-Za-z0-9_]+/)) {
            sym = substr(substr(m, 3), RSTART, RLENGTH); lastsym = sym
            if (system("grep -qF -- \"" sym "\" \"" root "/" p "\"") == 0) oksym = 1
          }
        }
      }
      if (!okp) problem(node " " id ": falsifier [" fals "] names no path that exists in this tree")
      else if (anycol && !oksym) problem(node " " id ": falsifier names symbol [" lastsym "] which does not occur in " lastp)
      next
    }
    END {
      flush()
      if (nodes == 0) problem("no `### W-N<n>` node headings found -- the scan would pass vacuously")
      if (rows == 0) problem("no Table A rows found -- the scan would pass vacuously")
      if (tables == 0) problem("no Table A header row found -- the column count and the column names would go unchecked")
      if (dc < 0 || dp < 0) problem("calibration line is not well-formed (need `controls caught: C of P`)")
      else { if (dp < 1) problem("calibration declares 0 controls planted"); if (dc > dp) problem("calibration caught " dc " of " dp " -- more than planted") }
      # **`unread` is printed, and that is the point of it.** A row whose shape is wrong is refused
      # above; this counter is how a reader sees that the refused count is zero rather than taking
      # the totals line at its word -- the failure mode this check had was silence, not a wrong
      # number, so the number it printed could not have caught it.
      printf "\n  totals: %d nodes, %d rows (%d unread), %d tables, %d/%d controls caught\n", nodes, rows, unread, tables, dc, dp
      exit (bad > 0)
    }
  '
status=$?
set -e

if (( status == 0 )); then
  echo "check-wargame: every node dispositions all eleven guide words, every row has the header's shape and is decided and evidenced, calibration is declared."
  exit 0
fi
if (( gate == 1 )); then
  echo "check-wargame: the wargame section does not hold." >&2
  exit 1
fi
echo "check-wargame: report only (use --gate to fail)."
exit 0
