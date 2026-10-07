#!/usr/bin/env python3
"""Dial a peer's OCapN listener through this node's own admin route.

Reads the target node's Ed25519 seed out of its identity file (via `docker exec`), derives the
public key with openssl, and POSTs a dial to the *dialling* node's admin surface. Used by `run.sh`
in place of curl because the workstation's conda curl has a broken libcurl and resets the
connection.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

BOOTSTRAP = "devnet-bootstrap"
ADMIN = "http://127.0.0.1:41405/api/v1/ocapn/dial"
SWISS = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w".hex()


def verifying_key(container: str, path: str) -> str:
    seed = subprocess.run(
        ["docker", "exec", container, "od", "-An", "-tx1", "-v", "-N", "32", path],
        capture_output=True, text=True, check=True,
    ).stdout
    seed_hex = seed.replace(" ", "").replace("\n", "")
    assert len(seed_hex) == 64, f"not an Ed25519 seed: {len(seed_hex)} hex chars"
    der = bytes.fromhex("302e020100300506032b657004220420") + bytes.fromhex(seed_hex)
    # Written outside the tree: it is a private key, even a throwaway one, and nothing should have to
    # remember not to commit it.
    path = os.path.join(tempfile.gettempdir(), "ocapn-devnet-pk8.der")
    open(path, "wb").write(der)
    try:
        pub = subprocess.run(
            ["openssl", "pkey", "-inform", "DER", "-in", path, "-pubout", "-outform", "DER"],
            capture_output=True, check=True,
        ).stdout
    finally:
        os.unlink(path)
    return pub[-32:].hex()


def main() -> int:
    verify = verifying_key(BOOTSTRAP, "/var/lib/rnode/ocapn-identity.key")
    print(f"the bootstrap verifies as {verify}")
    body = json.dumps({
        "designator": BOOTSTRAP,
        "transport": "noise",
        "hints": {"host": BOOTSTRAP, "port": "22045", "verify": verify},
        "swiss": SWISS,
    }).encode()

    # **Retried, because a TCP connect to a published docker port proves nothing.** The port's userland
    # proxy accepts the connection before the container's listener exists and then resets it — measured
    # here: an open of 127.0.0.1:41405 succeeded while every HTTP request to it was refused. The gate
    # that means something is an HTTP *response*, from either the route or its `enable-ocapn-dial`
    # guard, so a reset is retried and a status is not.
    for attempt in range(1, 121):
        request = urllib.request.Request(
            ADMIN, data=body, headers={"content-type": "application/json"}
        )
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                print(f"HTTP {response.status}")
                print(response.read().decode())
                return 0
        except urllib.error.HTTPError as error:
            # A status is an answer: the route is up and it declined. Report it, do not retry.
            print(f"HTTP {error.code}")
            print(error.read().decode())
            return 1
        except (ConnectionResetError, urllib.error.URLError) as error:
            if attempt == 120:
                print(f"!! the admin route never answered: {error}")
                return 1
            time.sleep(1)
    return 1


if __name__ == "__main__":
    sys.exit(main())
