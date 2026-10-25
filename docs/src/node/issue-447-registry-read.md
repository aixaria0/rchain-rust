# Issue #447: native registry lookup at a pinned block

The public testnet is on an older binary. The disputed registry behaviour must
first be reproduced on a **source-pinned, isolated, disposable** node. A
successful arbitrary Rholang data-at-name read does not establish whether the
native registry sidecar at that state root contains a registered URI.

The acceptance probe executes the following sequence on a fresh one-validator
devnet, using only the public throwaway key already defined in devnet.sh:

1. Capture the genesis block and its pre/post-state roots.
2. Submit a **signed** insertion of integer 42 through the native system
   registry insertArbitrary channel; publish its resulting URI to a distinct
   public channel, and require a carrying block with a deploy.
3. Read the resulting URI at the carrying block through the pinned exploration
   endpoint, not through an unpinned latest-tip query.
4. Compare the same native registry lookup at the pre-registration ancestor,
   at the carrying block's **pre-state**, and at its **post-state**.
5. Require Nil, Nil, and 42 respectively. Preserve the complete endpoint
   envelopes, selected block, URI, and state hashes on failure as well.
   A same-post-state Nil is a reportable mismatch; a Nil on the ancestor
   or pre-state is the expected negative control.

Run as a dispatched GitHub-hosted workflow job:

    gh workflow run devnet-fuzz.yml -R aixaria0/rchain-rust \
      --ref verification/issue-447-registry-pinned \
      -f mode=issue447-registry

The workflow records exact source SHA, Docker image digest, startup evidence,
read response envelopes and error details. It always destroys the disposable
container and uploads raw evidence even for a failing acceptance verdict.
**No production validator, wallet, bond set, or public chain is touched.**

This is strictly a scoped native-registry read test; it does not validate
application-level Root contract resolution, same-channel merge rejection,
admission, or the complete 17-deploy r-cash installer. Such claims require
their own independent test evidence.
