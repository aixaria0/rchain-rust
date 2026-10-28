#!/usr/bin/env python3
"""Read-only live-testnet fresh-join and restart qualification for a passive node."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request

BASE = "https://testnet.rhobot.net"
PINNED_GENESIS = "c596bef4bb678b3ac0cdb931b5b6968653209ca40bc35b6fec0050254d479b26"
PINNED_FINALITY_HEIGHT = 1725
PINNED_FINALITY_HASH = "393b9ed4d90d74677be2e935a38ac2945287dc228325ad873421028abe3d7d6a"
BOOTSTRAP = "rnode://6b70152e0dfe887101428710a5c5a14c9933a3d6@164.90.140.144?protocol=40400&discovery=40404"
MULTISIG = "042d721140c51d83d87503d86fa85e0c835c1b80dcb086251b7c59da497c6ed508625a5c0624741278af944e74f701e4d0f5c2b447b561a1e52f164e895c24b15d"
BONDS = """0410b8c59d04df768160eb97f8773db56a9e861f3b8c53ba4ef80a1d4f71bb2c6c9cc977b3c93d5cc3dbe9465fa7f501014be4238a5a74b2d2256f95321a0c3c73 250
04d7707c7d31abc15f961da732f759311a24d5ff7431af2a25f75abd82a188be5e367ccce681e444cc8b671e27ca544d2e4a824d21363667e76c1d4c937ca25b92 250
04dce59b572da605b9eaa554deca8d364ef541a36ff4386d7ba49087d86866fd3c2d14db6b237aeee46cf732e8880420fe5af5734f01e8a715fc7f7c8da08b045c 250
041ed2a25819ada35a65dbcce3b42c6a03a414ce6c13cd908aa169876294bf12100698876b4ea5dca09afe23ad7ce27f6bbe7ad0ec4330ca2759645dc4c8b31266 250
"""
WALLETS = """1111pJu4TJaJDNJDTinnftr2fcHvMfnDeTRXRzwgPfwuKmGMa5juj,1000000000000
11116Xmisa9auibAV5GvqfVe1pqB71wHU55ZWVsaYYbwRX7QtGF1X,1000000000000
1111RMGCXTSVYKRi6eRyzNLxjJFHUFD7ctJxL3K1jFt4G2eGY6t5g,1000000000000
1111icKQ4mh226WBfHvWfhjEdDqBU5npSuWANBhnQUmBh37NmCuuk,1000000000000
"""

def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def get_json(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "rholo-n457-live-validation"}), timeout=8) as response:
        return json.load(response)

def maybe_json(url):
    try:
        return get_json(url)
    except Exception as exc:
        return {"_error": str(exc)[:200]}

def resident_mb(pid):
    try:
        status = Path(f"/proc/{pid}/status").read_text()
        for ln in status.splitlines():
            if ln.startswith("VmRSS:"):
                return int(ln.split()[1]) // 1024
    except OSError:
        pass
    return 0

def host_available_mb():
    for ln in Path("/proc/meminfo").read_text().splitlines():
        if ln.startswith("MemAvailable:"):
            return int(ln.split()[1]) // 1024
    return 0

def terminate(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

def genesis_check(port):
    blocks = maybe_json(f"http://127.0.0.1:{port}/api/blocks/0/0")
    if not isinstance(blocks, list):
        return False
    return any(x.get("blockHash") == PINNED_GENESIS for x in blocks)

def run_one(label, data, ip, deadline_secs, minimum_height):
    port = 43403
    log_path = Path(f"{label}.log")
    command = [
        str(Path("bin/rnode").resolve()), "--profile", "docker", "run",
        "--dev-mode", "--no-upnp", "--no-attest-on-new-blocks",
        "--thread-pool-size", "4",
        "--network-id", "testnet", "--shard-name", "testnet", "--parent-shard-id", "/",
        "--host", ip,
        "--protocol-port", "43400", "--api-port-grpc-external", "43401",
        "--api-port-grpc-internal", "43402", "--api-port-http", str(port),
        "--discovery-port", "43404", "--api-port-admin-http", "43405",
        "--data-dir", str(data.resolve()),
        "--bonds-file", str(Path("genesis/bonds.txt").resolve()),
        "--wallets-file", str(Path("genesis/wallets.txt").resolve()),
        "--bootstrap", BOOTSTRAP,
        "--pos-multi-sig-public-keys", MULTISIG, "--pos-multi-sig-quorum", "1",
        "--epoch-length", "10", "--quarantine-length", "10",
        "--bond-minimum", "1", "--bond-maximum", "250",
        "--number-of-active-validators", "4",
    ]
    start = time.monotonic()
    peak_rss = 0
    results = {"label": label, "start": stamp(), "peakRSSMiB": 0, "status": "INCOMPLETE"}
    log = open(log_path, "wb")
    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        while time.monotonic() - start < deadline_secs:
            if process.poll() is not None:
                results["status"] = "NODE_EXITED"
                results["exitCode"] = process.returncode
                break
            peak_rss = max(peak_rss, resident_mb(process.pid))
            elapsed = round(time.monotonic() - start)
            status = maybe_json(f"http://127.0.0.1:{port}/api/v1/status")
            height = status.get("latestBlockNumber", 0)
            if not isinstance(height, int):
                height = 0
            fin = maybe_json(f"http://127.0.0.1:{port}/api/last-finalized-block").get("blockInfo", {})
            if not isinstance(fin, dict):
                fin = {}
            last_finalized = fin.get("blockNumber", -1)
            final_hash = fin.get("blockHash", "")
            results.update({"elapsedSeconds": elapsed, "peakRSSMiB": peak_rss, "height": height,
                            "finalityHeight": last_finalized, "finalityHash": final_hash})
            print(json.dumps({"phase": label, "seconds": elapsed, "height": height,
                              "finalized": last_finalized, "rssMiB": peak_rss,
                              "hostAvailableMiB": host_available_mb(),
                              "peers": status.get("peers"), "status": results["status"]}), flush=True)
            # The pinned finalized hash prevents declaring a different chain successful.
            if height >= minimum_height and last_finalized >= PINNED_FINALITY_HEIGHT:
                if final_hash == PINNED_FINALITY_HASH and genesis_check(port):
                    results["status"] = "PASS"
                    break
                # If finality has advanced beyond the anchor, the finality hash
                # won't equal the anchor. Confirm the anchor via block API instead.
                at_height = maybe_json(f"http://127.0.0.1:{port}/api/blocks/{PINNED_FINALITY_HEIGHT}/{PINNED_FINALITY_HEIGHT}")
                if genesis_check(port) and isinstance(at_height, list) and any(x.get("blockHash") == PINNED_FINALITY_HASH for x in at_height):
                    results["status"] = "PASS"
                    break
            time.sleep(30)
        else:
            results["status"] = "TIMEBOX_INCOMPLETE"
    finally:
        terminate(process)
        log.close()
        results["end"] = stamp()
        results["peakRSSMiB"] = peak_rss
        results["processExitAfterStop"] = process.returncode
        Path(f"{label}.json").write_text(json.dumps(results, indent=2))
    return results

def main():
    Path("genesis").mkdir(exist_ok=True)
    Path("store").mkdir(exist_ok=True)
    Path("genesis/bonds.txt").write_text(BONDS)
    Path("genesis/wallets.txt").write_text(WALLETS)
    import subprocess as sp
    sp.run(["chmod", "644", "genesis/bonds.txt", "genesis/wallets.txt"], check=True)
    version = sp.check_output(["bin/rnode", "--version"], text=True).strip()
    expected_sha = "d1af45d1db3f5fe2fa6047ad6c8a72b654d041df"
    if expected_sha not in version:
        raise RuntimeError(f"Unexpected binary source: {version}")
    bootstrap = get_json(BASE + "/api/v1/status")
    finalized = get_json(BASE + "/api/last-finalized-block")["blockInfo"]
    print(json.dumps({"source": version, "bootstrap": bootstrap, "finality": finalized}), flush=True)
    if bootstrap.get("networkId") != "testnet" or bootstrap.get("shardId") != "/testnet":
        raise RuntimeError("Bootstrap is not testnet")
    ip = urllib.request.urlopen("https://api.ipify.org", timeout=8).read().decode().strip()
    print(f"RUNNER_PUBLIC_IP={ip}", flush=True)
    first = run_one("fresh-join", Path("store"), ip, deadline_secs=3600, minimum_height=PINNED_FINALITY_HEIGHT)
    if first["status"] != "PASS":
        print("VERDICT: INCOMPLETE; no fresh-join PASS", flush=True)
        return 1
    print("VERDICT: LIVE_FRESH_JOIN_PASS", flush=True)
    second = run_one("restart", Path("store"), ip, deadline_secs=780, minimum_height=PINNED_FINALITY_HEIGHT)
    if second["status"] == "PASS":
        print("VERDICT: LIVE_RESTART_PASS", flush=True)
        return 0
    print("VERDICT: FRESH_JOIN_PASS_RESTART_INCOMPLETE", flush=True)
    return 1

if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        Path("unexpected-error.txt").write_text(str(exc))
        print("UNEXPECTED_ERROR", repr(exc), flush=True)
        raise
