== the anchored sync drill: the restore works, the catch-up does not ==

rig: four validators, epoch-length 10, no-autopropose. The chain ran to height 157 with finality 153.
Three joiners were wiped (devnet reset): V1 with --sync-anchor, V2 on the ordinary path, V3 untouched.

== V1's own log: the anchor became the sync's root ==
2026-10-09T18:45:05.070Z INFO  [casper.engine.NodeLaunch] Syncing to the operator-named anchor 199c289122510a632a1ec40a5dfb7ca7156a926714006e863be70ad2db8e7098 rather tha
2026-10-09T18:45:05.115Z INFO  [casper.engine.NodeLaunch] the anchor block 199c289122510a632a1ec40a5dfb7ca7156a926714006e863be70ad2db8e7098 is the sync's root
2026-10-09T18:45:35.391Z INFO  [casper.engine.NodeSyncing] LFS state is successfully restored.

== heights ==
master           h 157
V1-anchor150     h 151
V2-control       h 157
V3-untouched     h 157

== the blocks above the anchor, master vs the anchored joiner ==
h=150  master: 199c2891,37842303,5b7c105b   V1: 199c2891
h=152  master: 667d6644,bdcfbc6c,e4abe91a   V1: -
h=154  master: 45fcaf4c,4e69326b,a2199b59   V1: -
h=156  master: 2abb2b81,38b4e563,5c52663d   V1: -

== the earlier attempt at an OLD anchor (block 30), which fails differently ==
validateBlockCheckpoint failed: regenerated merge (many) — restoring to an old root leaves a long
suffix to validate, which is the C188 class, not the anchor.
