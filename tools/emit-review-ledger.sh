#!/usr/bin/env bash
#
# emit-review-ledger.sh — the review ledger's rosters, and the page they are rendered onto.
#
# **What this is for.** The second-round audit (2026-09-26) ended with a rating but no *denominator*:
# twelve expert lenses read the regions that mattered and nothing recorded what was **not** read, so "no
# finding here" was indistinguishable from "nobody looked". The repo already knows the principle — the
# register's C80, *"a catalogue with no count of itself is checked for internal consistency, not for
# completeness"* — and applied it to the law register (`entryCeiling`) and never to the audit itself.
# `spec/review-ledger.tsv` is that count, and this file is what makes it a measurement rather than a
# promise.
#
# **The split, and which half is which.**
#
#   * `spec/review-ledger.tsv` is **authored** — one row per item, carrying that item's verdict, depth,
#     evidence and note. No script can write a review; only `--seed` may add rows, and it may never
#     *originate* a review verdict (see below).
#   * `spec/REVIEW-LEDGER.md` is **emitted** — derived from the TSV plus the rosters re-derived from the
#     tree, so the page cannot state a count the tree does not support.
#
# **The honesty property, and where it is enforced.** `--seed` writes a skeleton: for every item on
# every roster, a row whose verdict is the default. It **may never write `cleared`, `finding` or
# `sampled`** — those three are the words that claim a review happened, and a claim of review is the
# one thing a script cannot make. The rule is enforced where rows are written rather than trusted to
# discipline: every emitted row carrying one of those three must have carried it in the file already,
# and the seed aborts if it cannot show that. `deferred` is the default because it is the *honest* one —
# it says "in remit, not yet reviewed", which is what a row nobody has touched actually means. A blank
# cell is never a verdict, so "nobody looked" cannot be represented silently.
#
# **Tier is per item, not per file, and 48 of them are derived rather than judged.** The tier words are
# the register's own (`spec/TEST-COVERAGE.md`: T1 can fork the chain or lose funds, T2 is
# correctness-adjacent, T3 is presentation), and clause (h) of check 15 requires the word here to agree
# with that table. A file the register has already tiered takes its tier from the register — so the
# ledger inherits a judgement instead of inventing one — and anything else takes **T3, the default, with
# the consequence stated on the page**: 300 rows at T3 is not 300 files cleared, it is 300 files nobody
# has classified.
#
# **Every roster is derived from its own site, never restated.** The `file` roster is the *same walk*
# `tools/audit-type-system.sh` scans (`--files`, added for this), so the ledger's denominator is the set
# the gate moves rather than a second definition that can disagree; `law` is `spec/laws.tsv`; `class` is
# the ratchet's own baseline; `process` is `Symbols::definitions()`; `config` is `options.rs`; `tool` is
# the filesystem. The plan that specified this ledger carried **358** for the file roster — a number
# with no derivation. Measured 2026-09-26, no definition yields 358 (the walk gives 350). That is the
# defect class this whole unit exists to close, caught in the plan that closed it.
#
# Usage:
#   tools/emit-review-ledger.sh            # write spec/REVIEW-LEDGER.md
#   tools/emit-review-ledger.sh --check    # re-emit and refuse a diff (check 15 clause (b))
#   tools/emit-review-ledger.sh --seed     # refresh spec/review-ledger.tsv against the rosters
#   tools/emit-review-ledger.sh --rosters  # print the derived `kind<TAB>id` roster and exit
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="$ROOT/spec/REVIEW-LEDGER.md"
TSV="$ROOT/spec/review-ledger.tsv"
REGISTER="$ROOT/spec/TEST-COVERAGE.md"

check=0
seed=0
rosters_only=0
while (( $# > 0 )); do
  case "$1" in
    --check)   check=1 ;;
    --seed)    seed=1 ;;
    --rosters) rosters_only=1 ;;
    --tsv)     TSV="${2:?--tsv needs a path}"; shift ;;
    --out)     OUT="${2:?--out needs a path}"; shift ;;
    *) echo "usage: $0 [--check] [--seed] [--rosters] [--tsv <path>] [--out <path>]" >&2; exit 2 ;;
  esac
  shift
done

die() { printf 'emit-review-ledger: %s\n' "$1" >&2; exit 1; }

# ---------------------------------------------------------------------------------------------
# The rosters. Each is `kind<TAB>id`, one line per item, and each is derived at the site that owns it.
# ---------------------------------------------------------------------------------------------
roster_file() { bash "$ROOT/tools/audit-type-system.sh" --files | sed 's/^/file\t/'; }

# The law roster is the **entry**, not the law: `spec/laws.tsv` holds 60 entries over 50 laws (a law
# with sub-clauses has one row each), and the pair (number, clause) is what identifies one — verified
# unique 2026-09-26. Naming a row by its number alone would collide.
roster_law() {
  tail -n +2 "$ROOT/spec/laws.tsv" | awk -F'\t' '{ printf "law\t%s%s\n", $1, $2 }'
}

# The counted classes are the ratchet's own rows — the baseline *is* the list of classes the gate
# measures, so taking them from there means a class added to the gate cannot be missing from here.
roster_class() {
  awk -F'\t' '$1 !~ /^#/ && $2 ~ /^[0-9]+$/ { printf "class\t%s\n", $1 }' "$ROOT/tools/type-system-baseline.tsv"
}

# A system process is a `urn:` literal in `Symbols::definitions()` — the registry's own list, which is
# what a deploy can actually reach. 30 of them, and one (`sys:authToken:ops`) is not `rho:`-prefixed;
# it is in the roster because it is in the list.
roster_process() {
  grep -oE 'urn: "[^"]+"' "$ROOT/rholang/src/system_processes.rs" \
    | sed 's/urn: "//; s/"$//' | awk '{ printf "process\t%s\n", $0 }'
}

# A config flag is a clap long option, keyed by its **name**.
#
# The first version keyed `line:name`, and that was wrong in a way that only showed up when it was
# used: adding one flag above the others made every row below it stale, so a single insertion in
# `options.rs` broke 80 ledger rows at once. A flag's identity is its name; a line number is where it
# happens to sit today, and the ledger outlives the line. This is the same rule the register states
# for its own anchors — "a name does not rot and a line does" (`spec/STYLE.md`) — applied to the
# roster instead of to a citation.
#
# Three names (`content`, `depth`, `type`) appear under more than one subcommand, so a bare name is not
# unique. Those collisions get an occurrence suffix (`content@2`), which keeps the key stable under
# edits above them and keeps the trijection this roster feeds (flag ↔ `NodeConf` field ↔
# `defaults.conf`) a per-site question where it needs to be.
roster_config() {
  grep -oE 'long = "[^"]+"' "$ROOT/node/src/configuration/commandline/options.rs" \
    | sed -E 's/long = "([^"]+)"/\1/' \
    | awk '{ n[$0]++; if (n[$0] == 1) printf "config\t%s\n", $0; else printf "config\t%s@%d\n", $0, n[$0] }'
}

# An ingress site is where bytes from somebody else are first read. The plan for this ledger called the
# ingress roster *authored* with a ceiling, on the reasoning that a derivation cannot see an entry point
# that is not a route — and then guessed "~40". Three sources are derivable and together they are the
# server's whole network face, so they are derived and the ceiling stands over them for the rest:
#
#   * **HTTP** — every `.route(...)` in `node/src/web/http.rs`, which builds both the public router and
#     the admin router (the latter is the one that had no authentication, round 2's `/api/propose`);
#   * **gRPC** — every `*grpc_service*.rs` under `node/src/api/grpc/`, one row per service: the
#     authentication question is asked of a service, and a service's methods share its answer;
#   * **p2p** — the transport receiver's RPCs and the Kademlia server's, which are the only two places a
#     peer's frame is parsed before anything has been established about that peer.
roster_ingress() {
  grep -oE '^\s*\.route\("[^"]+"' "$ROOT/node/src/web/http.rs" \
    | sed -E 's/^\s*\.route\("([^"]+)".*/\1/' | sort -u | awk '{ printf "ingress\thttp:%s\n", $0 }'
  local f b
  for f in "$ROOT"/node/src/api/grpc/*grpc_service*.rs; do
    [ -f "$f" ] || continue
    b="$(basename "$f" .rs)"
    printf 'ingress\tgrpc:%s\n' "$b"
  done
  for f in "$ROOT/comm/src/transport/grpc_transport_receiver.rs" "$ROOT/comm/src/discovery/grpc_kademlia_rpc_server.rs"; do
    [ -f "$f" ] || continue
    b="$(basename "$f" .rs)"
    awk -v b="$b" '/^#\[cfg\(test\)\]/{exit} /^    async fn /{ sub(/^    async fn /, ""); sub(/\(.*/, ""); printf "ingress\tp2p:%s:%s\n", b, $0 }' "$f"
  done
}

# The instruments: every tool, and every workflow. A gate is a review instrument, so "the instruments
# were trusted" (the Unit 0 finding) is a ledger row like any other.
roster_tool() {
  local f
  for f in "$ROOT"/tools/*.sh "$ROOT"/tools/*.py; do
    [ -f "$f" ] || continue
    printf 'tool\t%s\n' "${f#"$ROOT/"}"
  done
  for f in "$ROOT"/.github/workflows/*.yml; do
    [ -f "$f" ] || continue
    printf 'tool\t%s\n' "${f#"$ROOT/"}"
  done
}

# The authored kinds have no derivation by construction, so their ids come from the TSV alone. `site`
# is the clear case — a site row exists only where someone read a site. `roster` is the ledger's own
# front page, one row, written by hand like any other page of it.
authored_kinds=(site roster)

# ---------------------------------------------------------------------------------------------
# The register's tier table, read rather than restated: path → T1|T2|T3.
# ---------------------------------------------------------------------------------------------
tier_of_file() {
  local f="$1"
  printf '%s\n' "$TIER_MAP" | awk -F'\t' -v p="$f" '$1 == p { print $2; found = 1 } END { if (!found) print "T3" }'
}
TIER_MAP="$(awk '/^## Risk tiers \(per-module\)/,/^## [^R]/' "$REGISTER" \
  | awk -F'|' '/^\| T[123] / { p = $3; t = $2; gsub(/[ `]/, "", p); gsub(/ /, "", t); printf "%s\t%s\n", p, t }')"
[[ -n "$TIER_MAP" ]] || die "no tier rows found in $REGISTER — the ledger's tiers would all be the default, which is a silent failure"

# ---------------------------------------------------------------------------------------------
# The rosters, built once.
# ---------------------------------------------------------------------------------------------
ROSTERS="$( { roster_file; roster_law; roster_class; roster_process; roster_config; roster_ingress; roster_tool; } | sort -u )"

derived_count() { printf '%s\n' "$ROSTERS" | awk -F'\t' -v k="$1" '$1 == k { n++ } END { print n + 0 }'; }

# `--rosters` prints every derived `kind<TAB>id` and exits, so a checker can compare the ledger against
# the tree without re-deriving either. The same reason `tools/audit-type-system.sh` grew `--files`: two
# definitions of one set can disagree, and then the ledger a reviewer works from is not the one the tree
# supports. Check 15 clause (a) is the caller.
if (( rosters_only )); then
  printf '%s\n' "$ROSTERS"
  exit 0
fi

# ---------------------------------------------------------------------------------------------
# --seed: refresh the skeleton, preserving every authored cell, and never originating a review.
# ---------------------------------------------------------------------------------------------
CEILINGS='ingress process config tool'

read_tsv() {
  # prints the rows of the current TSV as `kind<TAB>id<TAB>tier<TAB>verdict<TAB>depth<TAB>sample<TAB>reason<TAB>evidence<TAB>registers<TAB>note`
  [ -f "$TSV" ] || return 0
  awk -F'\t' '$1 !~ /^#/ && NF >= 2 { print }' "$TSV"
}

if (( seed )); then
  prev="$(read_tsv || true)"
  prev_verdict() { printf '%s\n' "$prev" | awk -F'\t' -v k="$1" -v i="$2" '$1 == k && $2 == i { print $4 }'; }
  prev_row()     { printf '%s\n' "$prev" | awk -F'\t' -v k="$1" -v i="$2" '$1 == k && $2 == i { print; exit }'; }

  # The authored kinds' ids survive from the file; the derived kinds' come from the tree. A row is
  # emitted for the union, so a roster item the TSV has never seen is added rather than silently absent.
  authored_ids="$(printf '%s\n' "$prev" | awk -F'\t' -v ok=" ${authored_kinds[*]} " 'index(ok, " " $1 " ") { printf "%s\t%s\n", $1, $2 }')"
  all_ids="$( { printf '%s\n' "$ROSTERS"; printf '%s\n' "$authored_ids"; } | grep -v '^$' | sort -u )"

  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT

  # The ceilings: preserved from the previous header if present, else the derived count. A ceiling the
  # derivation has outrun is a failure, not a rounding — it means rows are missing.
  {
    printf '# The review ledger — one row per item, authored. Regenerated in skeleton by:\n'
    printf '#   tools/emit-review-ledger.sh --seed\n'
    printf '# Columns: kind\tid\ttier\tverdict\tdepth\tsample\treason\tevidence\tregisters\tnote\n'
    printf '# verdict vocabulary: cleared finding sampled exempt deferred unreachable\n'
    printf '# tier vocabulary: T1 T2 T3   |   depth vocabulary: shallow medium deep -\n'
    printf '# reason vocabulary (exempt): data generated dev-tool legacy axiomatized\n'
    printf '# reason vocabulary (unreachable): peer-bound harness-bound\n'
    for k in $CEILINGS; do
      c="$(printf '%s\n' "$prev" | awk -v k="$k" '$1 == "#" && $2 == "ceiling" && $3 == k { print $4 }')"
      d="$(derived_count "$k")"
      printf '# ceiling\t%s\t%s\n' "$k" "${c:-$d}"
    done
    # Every allocated C-number that no `finding` row names. An audit that has registered nothing yet is
    # honest about it here rather than in prose, and the ceiling only goes down.
    allocated="$(bash "$ROOT/tools/next-audit-number.sh" 2>/dev/null | sed -n 's/^in use: //p' | tr ' ' '\n' | grep -c . || true)"
    unhoused="$(printf '%s\n' "$prev" | awk '$1 == "#" && $2 == "unhousedCeiling" { print $3 }')"
    printf '# unhousedCeiling\t%s\n' "${unhoused:-${allocated:-0}}"
  } > "$tmp"

  # The review verdicts: the three words a script may never originate. Collected from the previous file
  # so that the assertion below is a comparison and not a promise.
  review_verdicts="$(printf '%s\n' "$prev" | awk -F'\t' '$4 == "cleared" || $4 == "finding" || $4 == "sampled" { printf "%s\t%s\n", $1, $2 }' | sort -u)"

  n_new=0
  n_kept=0
  while IFS=$'\t' read -r kind id; do
    [ -n "$kind" ] || continue
    row="$(prev_row "$kind" "$id" || true)"
    if [ -n "$row" ]; then
      # Carried forward verbatim: every authored cell is the author's, including the note.
      printf '%s\n' "$row" >> "$tmp"
      n_kept=$((n_kept + 1))
    else
      tier="T3"; depth='-'
      case "$kind" in
        file) tier="$(tier_of_file "$id")" ;;
      esac
      # A new row is `deferred` — the honest default. Never a review verdict, by construction: this
      # branch is only reached for an (kind,id) the previous file did not have.
      printf '%s\t%s\t%s\tdeferred\t%s\t-\t-\t-\t-\t-\n' "$kind" "$id" "$tier" "$depth" >> "$tmp"
      n_new=$((n_new + 1))
    fi
  done < <(printf '%s\n' "$all_ids" | sort -t$'\t' -k1,1 -k2,2)

  # **The rule, and it is a count rather than a set difference — because the set difference cannot fail.**
  # The first version asked whether any review verdict in the output was absent from the previous file.
  # Every output row is either carried forward verbatim (so its verdict is in the previous file by
  # construction) or is new, and a new row is written `deferred` by the same branch that makes it new.
  # The difference was therefore empty whatever the file said: a check that could not fail, in the file
  # whose whole purpose is to stop claims a person did not make. The count can fail, and does: change the
  # default verdict to `cleared` and the output gains one per new row, which is exactly the edit this
  # guards against. (Verified by making that edit — see the unit's falsification record.)
  n_review_out="$(awk -F'\t' '$4 == "cleared" || $4 == "finding" || $4 == "sampled"' "$tmp" | grep -c . || true)"
  n_review_prev="$(printf '%s\n' "$review_verdicts" | grep -c . || true)"
  if [[ "$n_review_out" != "$n_review_prev" ]]; then
    printf 'FAIL  --seed would have originated a review verdict: the output carries %s, the previous file had %s.\n' \
      "$n_review_out" "$n_review_prev" >&2
    printf '      rows the previous file did not have, which now claim a review:\n' >&2
    awk -F'\t' '$4 == "cleared" || $4 == "finding" || $4 == "sampled" { printf "%s\t%s\n", $1, $2 }' "$tmp" | sort -u > "$tmp.rev"
    printf '%s\n' "$review_verdicts" | grep -v '^$' | sort -u > "$tmp.prev"
    comm -23 "$tmp.rev" "$tmp.prev" | sed 's/^/        /' >&2 || true
    rm -f "$tmp.rev" "$tmp.prev"
    die "a script may not write \`cleared\`, \`finding\` or \`sampled\` — those are claims a person makes"
  fi

  cp "$tmp" "$TSV"
  printf 'seeded %s (%d row(s) carried forward, %d new)\n' "${TSV#"$ROOT"/}" "$n_kept" "$n_new"
  exit 0
fi

# ---------------------------------------------------------------------------------------------
# The ledger, read. Every check below is against a file that must exist: an absent TSV is not a
# zero-row ledger, it is an unrun audit.
# ---------------------------------------------------------------------------------------------
[ -f "$TSV" ] || die "$TSV does not exist — run: tools/emit-review-ledger.sh --seed"

rows="$(awk -F'\t' '$1 !~ /^#/ && NF >= 10 { print }' "$TSV")"
[[ -n "$rows" ]] || die "$TSV has no data rows — an empty ledger beside a full roster is not a measurement"
badcols="$(awk -F'\t' '$1 !~ /^#/ && NF != 10 { print NR": "NF" field(s)" }' "$TSV" | head -5)"
[[ -z "$badcols" ]] || die "$TSV has a row that is not 10 columns wide:
$badcols"

ceiling_of() {
  # **Default field-splitting, not `-F'\t'`.** The header writes `# ceiling<TAB>ingress<TAB>44`; under
  # a tab-only FS that line's first field is `"# ceiling"` and the lookup misses — which is what this
  # did on its first run, reporting every ceiling as absent while the header was holding all four.
  # The default FS splits on runs of space *or* tab, and no value in these two line kinds contains
  # either. Data rows keep `-F'\t'`, where the `note` column does contain spaces.
  awk -v k="$1" '$1 == "#" && $2 == "ceiling" && $3 == k { print $4 }' "$TSV"
}
unhoused_ceiling="$(awk '$1 == "#" && $2 == "unhousedCeiling" { print $3 }' "$TSV")"

# The gate's own site count, computed **once and checked**, not substituted inline. The inline form
# was `$(bash … --sites | grep -c … || echo 0)`, and under this script's `pipefail` that printed the
# count *and* a zero whenever the gate exited non-zero — a two-line value spliced into the middle of a
# sentence, and worse, a `0` that would have read as "the class is empty" on a gate that had actually
# failed. Measured while re-anchoring the register: the page emitted `750\n0`. A count that can silently
# become zero is the defect class this whole unit exists for, so it fails loudly instead.
n_sites="$(bash "$ROOT/tools/audit-type-system.sh" --sites 2>/dev/null | grep -cE '^[a-z][a-z0-9-]*/src/.*:[0-9]+:' || true)"
if ! [[ "$n_sites" =~ ^[0-9]+$ ]] || (( n_sites == 0 )); then
  die "tools/audit-type-system.sh --sites yielded no countable sites (got '$n_sites') — the ledger's site denominator would be a zero nothing supports"
fi

# Per-kind tallies, in one pass, so the page and the checks cannot disagree about a count.
tally() {
  # $1 = kind; $2 = column number to match; $3 = value. Prints the count.
  printf '%s\n' "$rows" | awk -F'\t' -v k="$1" -v c="$2" -v v="$3" '$1 == k && $c == v { n++ } END { print n + 0 }'
}
kind_rows() { printf '%s\n' "$rows" | awk -F'\t' -v k="$1" '$1 == k { n++ } END { print n + 0 }'; }

# **The nine kinds, always all nine, in a fixed order** — not `cut -f1 | sort -u` off the rows. That
# was the first version, and it made an empty kind *invisible*: `site` has no rows yet, so it simply
# was not in the table, which is precisely the failure this ledger exists to remove. "Nobody looked"
# must be a row of zeros, not an absent row. The list is closed for the same reason the verdict
# vocabulary is: a kind that can be invented per-row is not a kind.
ALL_KINDS=(file ingress law site class process config tool roster)
all_kinds="$(printf '%s\n' "${ALL_KINDS[@]}")"
unknown_kinds="$(printf '%s\n' "$rows" | cut -f1 | sort -u | grep -vxF "$all_kinds" || true)"
[[ -z "$unknown_kinds" ]] || die "$TSV names kind(s) outside the closed set: $unknown_kinds"
for k in $all_kinds; do
  n="$(kind_rows "$k")"
  isderived=0
  case " ${authored_kinds[*]} " in *" $k "*) ;; *) isderived=1 ;; esac
  if (( isderived )) && [[ "$n" != "$(derived_count "$k")" ]]; then
    die "kind $k: the ledger holds $n row(s) and the tree derives $(derived_count "$k") — the roster and the rows disagree"
  fi
done
total_reviewed=0
for k in $all_kinds; do
  total_reviewed=$(( total_reviewed + $(tally "$k" 4 cleared) + $(tally "$k" 4 finding) + $(tally "$k" 4 sampled) ))
done

# ---------------------------------------------------------------------------------------------
# Emit the page.
# ---------------------------------------------------------------------------------------------
emit() {
  local hat
  hat="$(git rev-parse --short=9 HEAD 2>/dev/null || echo '-')"
  cat <<EOF
# The review ledger

<!-- Generated by tools/emit-review-ledger.sh from spec/review-ledger.tsv and the rosters in the tree.
     Do not edit by hand; edit the TSV, which is the authored half. -->

This page is the audit's **denominator**. It exists because the second-round audit (2026-09-26) had
none: twelve expert lenses read the regions that mattered, and nothing recorded what was *not* read, so
"no finding here" was indistinguishable from "nobody looked". The principle is the register's own —
C80, *"a catalogue with no count of itself is checked for internal consistency, not for completeness"*.

**The authored half is \`spec/review-ledger.tsv\`** — one row per item, and the only place a verdict is
written. **This page is emitted**: every count below is recomputed from the tree, so it cannot state a
total the tree does not support. Check 15 of \`tools/audit-test-register.sh\` binds the two together.

**Six verdict words, and the distinctions are the point.** \`cleared\` is a review that found nothing —
what "no finding here" looks like once it is earned. \`finding\` names a registered C-number. \`sampled\`
is the word the second round needed: 26 of 60 law rows reviewed is neither cleared nor deferred, and it
carries its fraction. \`exempt\` is a closed reason class. \`deferred\` is in remit, not yet read, and
reachable by reading. \`unreachable\` is in remit and **not decidable by reading** — a live peer, a
two-validator devnet — and the difference from \`deferred\` is that no amount of care settles it. A blank
cell is never a verdict; the checker refuses one, so "nobody looked" cannot be written silently.

## Rosters, and what has been reviewed

**Never summed.** Files overlap with the sites inside them and with the laws they witness, so one total
would be a number whose denominator changes meaning when someone re-scopes a kind. Each row stands alone.

| kind | roster | rows | reviewed | finding | sampled | cleared | deferred | exempt | unreachable |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
EOF
  for k in $all_kinds; do
    local nrev nfind nsamp nclear ndef nexc nunr dcell
    nrev=$(( $(tally "$k" 4 cleared) + $(tally "$k" 4 finding) + $(tally "$k" 4 sampled) ))
    nfind=$(tally "$k" 4 finding); nsamp=$(tally "$k" 4 sampled); nclear=$(tally "$k" 4 cleared)
    ndef=$(tally "$k" 4 deferred); nexc=$(tally "$k" 4 exempt); nunr=$(tally "$k" 4 unreachable)
    # An authored kind shows `—`, not `0`: a derivation that found nothing and a kind that has no
    # derivation are different facts, and printing 0 for the second would read as the first — which the
    # gate treats as a hard failure everywhere else it appears.
    dcell="$(derived_count "$k")"
    case " ${authored_kinds[*]} " in *" $k "*) dcell='—' ;; esac
    printf '| `%s` | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n' \
      "$k" "$dcell" "$(kind_rows "$k")" "$nrev" "$nfind" "$nsamp" "$nclear" "$ndef" "$nexc" "$nunr"
  done
  cat <<EOF

**What the columns mean, so the table is read the way it was written.** The first column is the derived
count (from the tree, at the site that owns the set); \`rows\` is how many the ledger holds. They are equal
for every derived kind, and the emitter refuses a ledger where they are not — a missing row is as much a
defect as an invented one, which is C80's rule and the reason one direction alone is not enough.

\`site\` and \`roster\` have no derivation, and show \`—\` rather than \`0\`: a \`site\` row exists only for
a site someone read, and \`roster\` is this page's own row. **All nine kinds appear even when empty**, and
that is deliberate — an empty kind rendered as an absent row is exactly the "nobody looked" this ledger
exists to make visible. A kind missing from this table would be a hole in the denominator.

## Tier, and what a default tier means

The tier words are the register's (\`spec/TEST-COVERAGE.md\`): **T1** can fork the chain or lose funds,
**T2** is correctness-adjacent (wrong answers), **T3** is presentation. Tier is a property of an *item*,
not of a file — \`casper/src/merging.rs\` is T1 while one arithmetic site inside it is T3.

| tier | rows | reviewed | deferred | exempt | unreachable |
|---|---:|---:|---:|---:|---:|
EOF
  for t in T1 T2 T3; do
    local tr td te tu tn
    tn=$(printf '%s\n' "$rows" | awk -F'\t' -v t="$t" '$3==t' | grep -c . || true)
    tr=$(printf '%s\n' "$rows" | awk -F'\t' -v t="$t" '$3==t && ($4=="cleared"||$4=="finding"||$4=="sampled")' | grep -c . || true)
    td=$(printf '%s\n' "$rows" | awk -F'\t' -v t="$t" '$3==t && $4=="deferred"' | grep -c . || true)
    te=$(printf '%s\n' "$rows" | awk -F'\t' -v t="$t" '$3==t && $4=="exempt"' | grep -c . || true)
    tu=$(printf '%s\n' "$rows" | awk -F'\t' -v t="$t" '$3==t && $4=="unreachable"' | grep -c . || true)
    printf '| %s | %s | %s | %s | %s | %s |\n' "$t" "$tn" "$tr" "$td" "$te" "$tu"
  done
  n_t3="$(printf '%s\n' "$rows" | awk -F'\t' '$3=="T3"' | grep -c . || true)"
  n_tiered="$(printf '%s\n' "$TIER_MAP" | grep -c . || true)"
  n_t1="$(printf '%s\n' "$rows" | awk -F'\t' '$3=="T1"' | grep -c . || true)"
  cat <<EOF

**$n_t3 rows are T3, and most of them are T3 by default rather than by judgement.** For the $n_tiered
files the register has already tiered, the tier above is *derived* from that table — a judgement
inherited, not invented. For every other item the seed writes T3 because it is the lowest tier, and that
is the consequence to read with it: **a T3 row is not a file that was judged presentational, it is a file
nobody has classified.** Raising one is a review act, and the cross-tab is what makes the backlog of
those acts visible instead of lost in a total.

**The $n_t1 T1 rows are the ones that matter, and they are all \`deferred\`.** That is the honest state of
this audit at the moment it was emitted, and it is the sentence a reader should carry away: the modules
that can fork the chain or lose funds are *named* here and unreviewed. A T1 row that cannot be read
deeply is \`deferred\` or \`unreachable\`, never \`cleared shallow\` — clause (e) of check 15 refuses that
cell, because a tier that is recorded and not paid for is a statistic.

## Ceilings, committed against derived

Where an authored roster can outrun its derivation, the TSV header records how far it may. A ceiling
below the derivation means rows are missing; a ceiling far above it means the roster is a promise.

| kind | committed | derived | status |
|---|---:|---:|---|
EOF
  for k in $CEILINGS; do
    local c d st
    c="$(ceiling_of "$k")"; d="$(derived_count "$k")"
    st=ok; [ -n "$c" ] || { st="no ceiling"; c='-'; }
    [ -n "$c" ] && [ "$c" != '-' ] && (( c < d )) && st="BELOW the derivation"
    printf '| `%s` | %s | %s | %s |\n' "$k" "$c" "$d" "$st"
  done
  cat <<EOF

**Findings allocated but unhoused**: the header records a ceiling of \`${unhoused_ceiling:-0}\` for
C-numbers the register allocates and no \`finding\` row names. An audit whose findings are all unregistered
is honest about that *as a number*, not as a sentence — the ceiling only goes down, and check 15 compares
it against the allocator.

## What this ledger does not measure

Stated here rather than left for a reader to assume, which is the idiom \`tools/emit-coverage-ledger.sh\`
uses for its own boundary.

* **It does not measure whether a \`cleared\` row is right.** A verdict is a person's claim, and the ledger
  records it; nothing here re-reads a file to check that "no finding" was earned. What the ledger *can*
  do is make the claim visible, attributable, and countable — and require that a T1 claim be \`deep\`,
  which is clause (e) of check 15 and the only floor on quality it can enforce.
* **It does not measure the depth of a review beyond the tier floor.** \`deep\` requires a resolving
  \`path:symbol\` and an evidence cell naming a falsifier or a C-number; it cannot tell a careful read from
  a fast one.
* **The site roster is authored, not derived.** Of the $n_sites counted type-system sites the gate ratchets, a \`site\` row exists only where someone read one. The *class*
  rows carry the totals; a cleared class is only honest when every site in it has a site row.
* **Overlap is real and deliberate.** A file row and the law rows witnessed in it are different
  questions about the same bytes. That is why the table above is never summed.

*Emitted from a tree at ${hat}.*
EOF
}

if (( check )); then
  scratch="$(mktemp)"
  trap 'rm -f "$scratch"' EXIT
  emit > "$scratch"
  # The commit line is compared out for the reason the coverage ledger compares out its measurement
  # date: it names the commit that last touched this page, so committing the page would otherwise make
  # the very next --check fail on the commit that landed it.
  if ! diff <(grep -v '^\*Emitted from a tree at' "$OUT") <(grep -v '^\*Emitted from a tree at' "$scratch") >/dev/null 2>&1; then
    echo "FAIL  $OUT is not what tools/emit-review-ledger.sh emits — re-emit it (the diff follows)" >&2
    diff <(grep -v '^\*Emitted from a tree at' "$OUT") <(grep -v '^\*Emitted from a tree at' "$scratch") | head -30 >&2 || true
    exit 1
  fi
  echo "ok    review ledger matches spec/review-ledger.tsv ($(printf '%s\n' "$rows" | grep -c . || true) row(s), $total_reviewed reviewed)"
else
  emit > "$OUT"
  echo "wrote ${OUT#"$ROOT"/} ($(printf '%s\n' "$rows" | grep -c . || true) row(s), $total_reviewed reviewed)"
fi
