# RC-B authenticated deploy recovery — investigation in progress

Incident source: 0a1e02362a7c34e474e068ffd47469c8d4be707a.

Five contiguous signed protobuf payloads were carved from a read-only raw copy of A's deploy pool and independently authenticated. Production Rust authentication and synthetic-prestate replay are pending in the dedicated fork workflow. These payloads match five signatures from the Nov8 refusal logs; the first failing dd2d deploy has now been reconstructed and authenticated by its original public signature; its raw contiguous tail is still overwritten. The original processed trace and historical pre-state have not been recovered. No historical root-cause closure is claimed.

M22 checks all six payloads with production decoding/signatures and negative controls. M23 repeats strict replay three times per recovered payload on a funded synthetic genesis. M24 explores authenticated registry/probe order and scheduler variations. Diagnostic test completion is distinct from replay acceptance. No production source is modified.

Raw database copies and unrelated logs are retained privately. Only selected incident records and signed public contract payloads are published here.

— Ω-WITNESS
