"""Archive actual isolated-node DAG summaries and hash preimages for replay."""
import concurrent.futures
import json
from pathlib import Path
import sys
import urllib.request

out = Path(sys.argv[1]) / "dag"
out.mkdir(parents=True, exist_ok=True)
errors = []
blocks = {}
for i in range(8):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{40403+1000*i}/api/blocks/0/1000", timeout=10) as r:
            data = json.load(r)
        (out / f"validator-{i}.json").write_text(json.dumps(data, indent=2)+"\n")
        def visit(x):
            if isinstance(x,dict):
                if "blockHash" in x and "justifications" in x:
                    blocks.setdefault(x["blockHash"], i)
                for v in x.values():
                    visit(v)
            elif isinstance(x,list):
                for v in x:
                    visit(v)
        visit(data)
    except Exception as e:
        errors.append({"node":i,"error":repr(e)})

def fetch(item):
    h,i=item
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{40403+1000*i}/api/block/{h}/raw", timeout=10) as r:
            data=json.load(r)
        (out / f"{h}.json").write_text(json.dumps(data)+"\n")
    except Exception as e:
        return {"hash":h,"error":repr(e)}
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    errors.extend(x for x in pool.map(fetch,blocks.items()) if x)
(out / "capture.json").write_text(json.dumps({"blocks":len(blocks),"errors":errors},indent=2)+"\n")
print(json.dumps({"captured_blocks":len(blocks),"errors":len(errors)}))
