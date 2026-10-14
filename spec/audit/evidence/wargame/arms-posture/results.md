# The wargame arms, on one devnet boot

- commit: `5fa5c3fe7`
- image: `ca63473797b6`
- validators: 3 · 2026-10-14T17:25:04Z
- **this run was cut short**: the table below is what it observed before it stopped

| row | recorded verdict | observed | arm | detail |
|---|---|---|---|---|
| W-N6-01 | NOT-RE-ESTABLISHED | violated | `arm_container_posture` | nothing contains a compromise inside the node: CapDrop=null ReadonlyRootfs=false SecurityOpt=null (docker_opts passes no --cap-drop, --read-only or --security-opt, so the container keeps every default capability and a writable root) |
