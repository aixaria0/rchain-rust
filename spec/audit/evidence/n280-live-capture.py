#!/usr/bin/env python3
"""Capture issue #280's live forensic evidence from the wedged public testnet.

Why this is a committed instrument rather than a transcript. The node it reads was **still wedged**
when this was written, and the DAG read it produces changes the moment the operator restarts it — so
the reading is perishable and had to be taken before the fix, not after. Every file it writes under
`n280-merge-loses-a-write/raw/` is the *reply* the node gave, not a summary of it; a summary would
repeat the mistake #280's own reporter was careful to avoid.

Usage:  python3 spec/audit/evidence/n280-live-capture.py [--endpoint https://testnet.rhobot.net]
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "n280-merge-loses-a-write", "raw")

# The tip the node is stuck at, and the walk it justifies. `seq_num`/`blockNumber` are the node's;
# the hashes are the ones in the issue's table plus every justification reached below height 100.
TIP = "ae973c3c006a5ada36eff70254e5ff675007d254331dc4610c5054d49eddc9e8"

# The three deploys the issue names, plus the #105 faucet transfer. Full signatures, from /api/block.
DEPLOY_SIGS = {
    "100-open": "304402200f7c03fd5ce4e4db619e7a2ad62f651fefdba6c77bcad5d242922094d4fc973102204fad840da330bdde5b7d826362dad9eb5ab853e1d40f5b936f3989a7151de26d",
    "103-cast": "304402201c15617a0bcc9d803b4adf679180f113aa9025f1b0c78d603d1648ef0895f85202200e1028ea1cd5ae0018978099ad5233afcfb2584dee4c3bc7366c0632050d6ed0",
    "105-faucet": "304402206045e2598f18e575b0a38b5a7a498b232d569ec2bf3046e357d26a9e013c033102201ccdbc0717945f67f1af007d8af2e974f38512fe5aae9b64868794fe73d98fe9",
}

# The Issue contract's registry id, from the issue's own "How to check it".
ISSUE_URI = "rho:id:hcgk7d95dif366pzpks6p8hbpgt9xg9oo5erzyu39z6eeqscum4y"

# A read of one facet through the registry, then one verb on it. The term is the issue's, with the
# facet and verb substituted, so the reading is comparable with the reporter's own.
FACET_TERM = (
    "new return, lookup(`rho:registry:lookup`), st, capsCh in {"
    "lookup!(`%s`, *st) |"
    "for (@r <- st) { match r { (_, c) => { capsCh!(c) } c => { capsCh!(c) } } |"
    'for (@caps <- capsCh) { match caps { {"read": f, ..._} => { @f!("%s", [], *return) }'
    ' _ => { return!("no read facet") } } } } }'
)

# The blocks whose post-state the read is made against: 100 (A, carrying the deploy), 101 (the
# finalised block), 102, 103 (A, carrying the cast), 104 (A only), 105 (the tip).
READ_BLOCKS = {
    "100-A": "357b06f84bd79147d72a4e0bf907aa6dd2899da7602e8e1e50b8ffb3e63f4918",
    "101-D-finalized": "e3a8e5fcf4b30c47cbede2fb1f4023861a0aa24e28a0e52233edf1431ad4f7f3",
    "102-A": "2f46520fd9798482b07069466cb4680d003808ef4c33fe1b640f178cc2b5153e",
    "103-A-cast": "1dc55474d997411a0ecef99bf0657678fd55749698d721cd5709d1ea0be667d8",
    "104-A": "0723f71f2849c983031f5b9cf9fc6c4fa682084c35ae6065912b0000c52dce0b",
    "105-A-tip": TIP,
}


def get(endpoint, path, timeout=30):
    """One GET, returning (text, error). The error is recorded, never raised: a probe that failed has
    to read as a failure in the artefact, or the artefact claims more than the node said."""
    try:
        with urllib.request.urlopen(endpoint + path, timeout=timeout) as r:
            return r.read().decode(), None
    except urllib.error.HTTPError as e:
        return "", f"HTTP {e.code}: {e.read().decode(errors='replace')[:400]}"
    except Exception as e:  # noqa: BLE001 - recorded, not handled
        return "", repr(e)


def post(endpoint, path, body, timeout=40):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        endpoint + path, data=data, headers={"content-type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read().decode(), None
    except urllib.error.HTTPError as e:
        return "", f"HTTP {e.code}: {e.read().decode(errors='replace')[:400]}"
    except Exception as e:  # noqa: BLE001
        return "", repr(e)


def write(name, text):
    path = os.path.join(OUT, name)
    with open(path, "w") as f:
        f.write(text if text.endswith("\n") else text + "\n")
    return path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint", default="https://testnet.rhobot.net")
    args = ap.parse_args()
    endpoint = args.endpoint.rstrip("/")
    os.makedirs(OUT, exist_ok=True)

    written, refused = [], []

    # --- the node's own account of itself -------------------------------------------------------
    for name, path in [
        ("status.json", "/api/status"),
        ("health.json", "/health"),
        ("last-finalized-block.json", "/api/last-finalized-block"),
        ("blocks-depth1.json", "/api/blocks"),
    ]:
        text, err = get(endpoint, path)
        write(name, text if err is None else json.dumps({"ERROR": err}, indent=2))
        (written if err is None else refused).append(f"{name} <- {path}")

    # --- every block of the walk, and the DAG edge list it came from -----------------------------
    seen, frontier, edges = set(), [TIP], []
    while frontier:
        h = frontier.pop(0)
        if h in seen:
            continue
        seen.add(h)
        text, err = get(endpoint, f"/api/block/{h}")
        write(f"block-{h[:12]}.json", text if err is None else json.dumps({"ERROR": err}, indent=2))
        (written if err is None else refused).append(f"block-{h[:12]}.json")
        try:
            bi = json.loads(text)["blockInfo"]
        except Exception:  # noqa: BLE001 - a refused block contributes no edges
            continue
        if bi["blockNumber"] > 92:  # the boundary round and two rounds either side
            for j in bi["justifications"]:
                edges.append((bi["blockNumber"], h, j))
                frontier.append(j)

    write(
        "dag-edges.tsv",
        "height\tblock\tjustification\n"
        + "\n".join("\t".join(str(x) for x in e) for e in edges),
    )

    # --- what the node says each named deploy did -----------------------------------------------
    for name, sig in DEPLOY_SIGS.items():
        text, err = get(endpoint, f"/api/v1/deploy-status/{sig}")
        write(f"deploy-status-{name}.json", text if err is None else json.dumps({"ERROR": err}, indent=2))
        (written if err is None else refused).append(f"deploy-status-{name}.json")

    # --- the contract read, at each block's post-state -------------------------------------------
    # This is the reading the issue got `[]` from and the reason the whole report exists: it is the
    # *same* term at six different states, so the table is comparable down the column.
    for name, h in READ_BLOCKS.items():
        text, err = post(
            endpoint,
            "/api/explore-deploy-by-block-hash",
            {"term": FACET_TERM % (ISSUE_URI, "issues"), "blockHash": h, "usePreStateHash": False},
        )
        write(f"explore-issues-{name}.json", text if err is None else json.dumps({"ERROR": err}, indent=2))
        (written if err is None else refused).append(f"explore-issues-{name}.json")

    print(f"wrote {len(written)} files to {OUT}")
    for line in refused:
        print(f"  REFUSED: {line}", file=sys.stderr)
    if refused:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
