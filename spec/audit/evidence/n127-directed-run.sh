#!/usr/bin/env bash
# #127's controlled comparison: `n117-after-fix-run.sh`, **unmodified**, against two trees.
#
# The protocol is `n127-directed-preregistration.md`, frozen before this ran. The two arms differ in one
# thing — the binary in the image — so the rig itself must not be edited between them: this wrapper runs
# the frozen script verbatim and does only the two things the script cannot do for itself, both of them
# about *which* binary it is measuring:
#
#   1. `rnode:local` is what the script's `docker run` and its manifest line both read, so the arm's image
#      is tagged into that name immediately before the run. The image id the manifest records is therefore
#      the arm's own, not an assumption;
#   2. the script writes to the fixed path `target/n117-audit/`, so each arm's artifacts are copied aside
#      under an arm-stamped directory as soon as it finishes. Two arms sharing one output directory is how
#      a campaign loses the ability to say which run produced which number.
#
# Everything else — the shape, the 8 GiB cap, the 3000 MiB threshold, the 300 s window, N=3 attempts, one
# devnet at a time, the clean-stop endpoint — is the frozen script's own default and is not overridden
# here on purpose. If an override is ever needed, it belongs in the preregistration first.
#
# Usage: spec/audit/evidence/n127-directed-run.sh <arm> <image-tag> [arm-tree]
#   e.g. spec/audit/evidence/n127-directed-run.sh control rnode:control-6eacc4969 6eacc4969
#
# `arm-tree` is the tree the arm's **binary** was built from, and it is a parameter because the two arms
# are not both the tree in this working directory: the control is built in a worktree at the pre-quotient
# commit, so a manifest line reading `git rev-parse HEAD` records the harness's tree and not the arm's.
# It defaults to the harness's HEAD for the arm that *is* this tree.
set -u
cd /home/patrick/RNodeRust

ARM=${1:?usage: n127-directed-run.sh <arm> <image-tag> [arm-tree]}
IMAGE=${2:?usage: n127-directed-run.sh <arm> <image-tag> [arm-tree]}
TREE=${3:-$(git rev-parse --short HEAD)}

[[ -f spec/audit/evidence/n127-directed-preregistration.md ]] || {
  echo "no preregistration — the protocol must be frozen before this runs" >&2
  exit 2
}
docker image inspect "$IMAGE" >/dev/null 2>&1 || {
  echo "no such image: $IMAGE" >&2
  exit 2
}

STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="target/n127-directed/${ARM}-${STAMP}"
mkdir -p "$OUT"

docker tag "$IMAGE" rnode:local
echo "arm=$ARM  image=$IMAGE -> rnode:local  id=$(docker inspect rnode:local --format '{{.Id}}')"
{
  echo "arm=$ARM"
  echo "image=$IMAGE"
  echo "image_id=$(docker inspect rnode:local --format '{{.Id}}')"
  echo "arm_tree=$TREE"
  echo "harness_tree=$(git rev-parse --short HEAD)"
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"

ATTEMPTS=3 spec/audit/evidence/n117-after-fix-run.sh 2>&1 | tee "$OUT/run.log"
rc=${PIPESTATUS[0]}

# The frozen script's own artifacts, filed under this arm's stamp before the next arm can overwrite them.
cp -f target/n117-audit/*.tsv "$OUT/" 2>/dev/null || true
for f in target/n117-audit/after-fix-log-*.txt; do
  [[ -f "$f" ]] && cp -f "$f" "$OUT/$(basename "$f")"
done

echo "finished=$(date -u +%Y-%m-%dT%H:%M:%SZ) rc=$rc" >> "$OUT/manifest.txt"
echo "arm=$ARM artifacts: $OUT"
