#!/usr/bin/env python3
"""Qualify restart and persisted-state replay of a keyless real testnet snapshot."""
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request

BINARY_SHA="d1af45d1db3f5fe2fa6047ad6c8a72b654d041df"
GENESIS="c596bef4bb678b3ac0cdb931b5b6968653209ca40bc35b6fec0050254d479b26"
BOOTSTRAP="rnode://6b70152e0dfe887101428710a5c5a14c9933a3d6@164.90.140.144?protocol=40400&discovery=40404"
MULTISIG="042d721140c51d83d87503d86fa85e0c835c1b80dcb086251b7c59da497c6ed508625a5c0624741278af944e74f701e4d0f5c2b447b561a1e52f164e895c24b15d"

def json_get(url):
    try:
        with urllib.request.urlopen(url,timeout=6) as response:
            return json.load(response)
    except Exception as e:
        return {"_error":str(e)[:180]}

def rss_mb(pid):
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])//1024
    except OSError:
        return 0
    return 0

def terminate(p):
    if p.poll() is None:
        p.send_signal(signal.SIGTERM)
        try:
            p.wait(timeout=20)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait(timeout=6)

def main():
    ver=subprocess.check_output(["bin/rnode","--version"],text=True).strip()
    assert BINARY_SHA in ver, ver
    data=Path("stored-real-testnet").resolve()
    assert (data/"dagstorage/data.mdb").is_file()
    assert (data/"blockstorage/data.mdb").is_file()
    assert (data/"genesis/bonds.txt").is_file()
    assert not any(data.glob("**/*.pem")), "identity file unexpectedly found"
    assert not list(data.glob("**/*private*")), "private key unexpectedly found"
    ip=urllib.request.urlopen("https://api.ipify.org",timeout=8).read().decode().strip()
    latest=get_anchor()
    print("SOURCE",ver,"RUNNER",ip,"BASELINE",json.dumps(latest),flush=True)
    ret_first=run("stored-replay",data,ip,1800,None)
    if ret_first.get("status") != "PASS":
        return 1
    ret_restart=run("stored-restart",data,ip,780,ret_first)
    return 0 if ret_restart.get("status")=="PASS" else 1

def get_anchor():
    data=json_get("https://testnet.rhobot.net/api/v1/status")
    assert data.get("networkId")=="testnet"
    return {"height":data["latestBlockNumber"],"version":data["version"]["node"]}

def verify_chain(port,minimum_height):
    base=f"http://127.0.0.1:{port}"
    status=json_get(base+"/api/v1/status")
    height=status.get("latestBlockNumber",0)
    if not isinstance(height,int) or height<minimum_height:
        return False,status
    blocks=json_get(base+"/api/blocks/0/0")
    if not isinstance(blocks,list) or not any(x.get("blockHash")==GENESIS for x in blocks):
        return False,status
    # Compare a historical height that must exist in the stopped D data.
    target=1500
    ours=json_get(base+f"/api/blocks/{target}/{target}")
    remote=json_get("https://testnet.rhobot.net"+f"/api/blocks/{target}/{target}")
    if not isinstance(ours,list) or not isinstance(remote,list):
        return False,status
    aset={x.get("blockHash") for x in ours}
    bset={x.get("blockHash") for x in remote}
    if not aset or aset!=bset:
        return False,status
    return True,status

def run(label,data,ip,seconds,previous):
    port=44403
    args=[
        str(Path("bin/rnode").resolve()),"--profile","docker","run",
        "--dev-mode","--disable-lfs","--no-upnp","--no-attest-on-new-blocks",
        "--thread-pool-size","4",
        "--network-id","testnet","--shard-name","testnet","--parent-shard-id","/",
        "--host",ip,
        "--protocol-port","44400","--api-port-grpc-external","44401",
        "--api-port-grpc-internal","44402","--api-port-http",str(port),
        "--discovery-port","44404","--api-port-admin-http","44405",
        "--data-dir",str(data),
        "--bonds-file",str(data/"genesis/bonds.txt"),
        "--wallets-file",str(data/"genesis/wallets.txt"),
        "--bootstrap",BOOTSTRAP,
        "--expected-genesis-hash",GENESIS,
        "--pos-multi-sig-public-keys",MULTISIG,"--pos-multi-sig-quorum","1",
        "--epoch-length","10","--quarantine-length","10","--bond-minimum","1",
        "--bond-maximum","250","--number-of-active-validators","4"
    ]
    start=time.monotonic()
    result={"phase":label,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "status":"INCOMPLETE","peakRSSMiB":0}
    minimum=1500 if previous is None else max(1500,previous.get("height",0))
    f=open(label+".log","wb")
    p=subprocess.Popen(args,stdout=f,stderr=subprocess.STDOUT)
    try:
        while time.monotonic()-start<seconds:
            if p.poll() is not None:
                result.update(status="NODE_EXITED",exitCode=p.returncode)
                break
            result["peakRSSMiB"]=max(result["peakRSSMiB"],rss_mb(p.pid))
            status=json_get(f"http://127.0.0.1:{port}/api/v1/status")
            height=status.get("latestBlockNumber",0)
            if not isinstance(height,int):height=0
            result.update(height=height,elapsedSeconds=round(time.monotonic()-start))
            print(json.dumps({"phase":label,"seconds":result["elapsedSeconds"],
                              "height":height,"rssPeakMiB":result["peakRSSMiB"],
                              "peers":status.get("peers")}),flush=True)
            if height>=minimum:
                if verify_chain(port,minimum)[0]:
                    result["status"]="PASS"
                    result["finality"]=json_get(f"http://127.0.0.1:{port}/api/last-finalized-block").get("blockInfo",{})
                    break
            time.sleep(20)
        else:
            result["status"]="TIMEBOX_INCOMPLETE"
    finally:
        terminate(p)
        f.close()
        result["utcEnd"]=datetime.datetime.now(datetime.timezone.utc).isoformat()
        Path(label+".json").write_text(json.dumps(result,indent=2))
        print("VERDICT",label,result["status"],flush=True)
    return result

if __name__=="__main__":
    try:
        sys.exit(main())
    except BaseException as exc:
        Path("stored-error.txt").write_text(repr(exc))
        raise
