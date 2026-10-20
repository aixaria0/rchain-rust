#!/usr/bin/env python3
"""Bounded same-volume partition/rejoin under explicit supporting deploy workload."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("acceptance",ROOT/"tools/consensus-acceptance-drill.py")
a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)

class WorkloadDrill(a.Drill):
    def wait(self, description, condition, drive=True):
        # This arm explicitly supplies work; it cannot establish quiet-chain liveness.
        return super().wait(description,condition,drive=True)

def cmd(*args):
    return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=25).strip()


def durable_state_volume(container):
    mounts=[m for m in container["Mounts"] if m["Destination"]=="/var/lib/rnode"]
    if len(mounts)!=1 or mounts[0]["Type"]!="volume":
        raise AssertionError("exactly one durable state volume required")
    m=mounts[0]
    return (m["Type"],m["Name"],m["Source"],m["Destination"],m["RW"])


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--validators",type=int,choices=[4,8],required=True)
    ap.add_argument("--out",required=True);args=ap.parse_args();args.window=150
    if os.environ.get("GITHUB_ACTIONS")!="true" or os.environ.get("RUNNER_ENVIRONMENT")!="github-hosted":
        raise SystemExit("disposable hosted runner only")
    if os.environ.get("DEVNET_PREFIX")!="acceptance":
        raise SystemExit("unexpected namespace")
    d=WorkloadDrill(args);d.run()
    original=json.loads((d.out/"result.json").read_text())
    victim=a.node_name(args.validators-1)
    volume_before=durable_state_volume(json.loads(cmd("docker","inspect",victim))[0])
    rounds=[]
    try:
        for cycle in range(2):
            before=a.finalized_identity(a.get(0,"/api/last-finalized-block"))
            cmd("docker","network","disconnect","acceptance",victim)
            networks=json.loads(cmd("docker","inspect","-f","{{json .NetworkSettings.Networks}}",victim))
            if "acceptance" in networks:raise AssertionError("partition was not installed")
            # Two 16-second escape/cadence cycles per N=8 sender plus margin:
            # a pre-registered 300-second window, not an unbounded retry.
            deadline=time.monotonic()+300;advanced=False;observed=[]
            while time.monotonic()<deadline:
                d.drive()
                identities=[]
                for i in range(args.validators-1):
                    height,hash_=a.finalized_identity(a.get(i,"/api/last-finalized-block"))
                    if not a.marker_present(a.probe(i,d.marker,hash_),d.value):
                        raise AssertionError("online validator lost finalized marker")
                    identities.append({"node":i,"height":height,"hash":hash_})
                observed.append(identities)
                (d.out/f"partition-{cycle+1}-live.json").write_text(json.dumps({
                    "cycle":cycle+1,"before_height":before[0],"observations":observed,
                    "remaining_seconds":max(0,deadline-time.monotonic())},indent=2)+"\n")
                if all(x["height"]>=before[0]+3 for x in identities):
                    advanced=True;break
                time.sleep(2)
            cmd("docker","network","connect","acceptance",victim)
            target=max(x["height"] for x in identities)
            recovered=d.wait("all validators retain marker after partition healing",
                             lambda:d.finalized_marker(target,required=True))
            d.fixed_snapshots(original["initial_finalized"])
            if durable_state_volume(json.loads(cmd("docker","inspect",victim))[0])!=volume_before:
                raise AssertionError("rejoin replaced the durable mounts")
            rounds.append({"cycle":cycle+1,"isolated_validator":victim,
                "before_height":before[0],"online_observations":observed,"rejoined":recovered,
                "same_volume":True,"original_finalized_snapshots_retained":True,
                "partition_progress_observed":advanced})
            if not advanced:
                raise AssertionError("online supermajority did not advance in 300s; healed recovery was checked separately")
    finally:
        subprocess.run(["docker","network","connect","acceptance",victim],capture_output=True)
        (d.out/"partition-observations.json").write_text(json.dumps(rounds,indent=2)+"\n")
    memory=cmd("docker","stats","--no-stream","--format","{{.Name}} {{.MemUsage}}")
    result={"result":"PASS_BOUNDED_WORKLOAD_PARTITION_REJOIN","source_sha":cmd("git","rev-parse","HEAD"),
        "validators":args.validators,"partition_cycles":len(rounds),"rounds":rounds,
        "resource_sample":memory,"quiet_chain_liveness":"NOT_ESTABLISHED",
        "scope":"retention and finality progression in these executions; not universal CBC safety"}
    (d.out/"partition-result.json").write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps(result),flush=True)

if __name__=="__main__":main()
