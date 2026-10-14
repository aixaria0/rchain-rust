# The wargame arms, on one devnet boot

- commit: `f9521edd7`
- image: `ca63473797b6`
- validators: 3 · 2026-10-14T15:17:58Z
- **this run was cut short**: the table below is what it observed before it stopped

| row | recorded verdict | observed | arm | detail |
|---|---|---|---|---|
| W-N12-06 | CONFIRMED | held | `arm_deploy_admission` | the second client is admitted while the first floods: the burst drew 89 429 and 111 200; the same identity was then admitted over gRPC (rc=0) and the second identity too (rc=0) |
| W-N2-01 | CONFIRMED | held | `arm_deploy_admission` | the bound has a subject: the burst drew 89 429 and 111 200; the same identity was then admitted over gRPC (rc=0) and the second identity too (rc=0) |
| W-N2-02 | CONFIRMED | held | `arm_deploy_admission` | the surfaces are not one pool: the burst drew 89 429 and 111 200; the same identity was then admitted over gRPC (rc=0) and the second identity too (rc=0) |
| W-N2-06 | CONFIRMED | inapplicable | `arm_deploy_admission` | decode-before-429 is not observable from the response alone; it needs a CPU or a counter, and this arm has neither |
| W-N2-08 | CONFIRMED | held | `arm_deploy_admission` | a second identity is not denied by the first's flood: the burst drew 89 429 and 111 200; the same identity was then admitted over gRPC (rc=0) and the second identity too (rc=0) |
| W-N2-09 | CONFIRMED | inapplicable | `arm_deploy_admission` | an invalid-signature flood needs a signed-but-wrong deploy, which this runner cannot mint |
| W-N3-01 | CONFIRMED | held | `arm_public_reads` | the public status carries the public key set only (HTTP 200) |
| W-N3-02 | CONFIRMED | violated | `arm_public_reads` | the public API is plaintext: http returns 200 and https returns 000 |
| W-N3-03 | CONFIRMED | held | `arm_public_reads` | the delegator read is not on the public port (404); the admin port answers it (200) |
| W-N3-05 | CONFIRMED | violated | `arm_public_reads` | an unsigned term executed over the unauthenticated public port (200) |
| W-N4-01 | CONFIRMED | violated | `arm_admin_actuator` | an unidentified caller made the node propose (HTTP 200: "Success! Block 4c4419bdb5f8eb749cbca042be0d04291622a76d4f9c5deae9754ed53561d6e9 created a) |
| W-N4-02 | CONFIRMED | inapplicable | `arm_admin_actuator` | the devnet is single-shard and answers 422 before any caller identity is considered; a multi-shard boot is the precondition this row names |
| W-N4-04 | CONFIRMED | violated | `arm_steady_state` | the chain advanced unattended, height 2 -> 26: block arrival is itself the proposal trigger |
| W-N4-06 | CONFIRMED | violated | `arm_admin_actuator` | a forged Origin/Host pair passed the guard and the node acted (HTTP 200) |
| W-N6-02 | CONFIRMED | violated | `arm_admin_actuator` | this host reached the admin actuator over its published port and the node acted (HTTP 200: "Success! Block 4c4419bdb5f8eb749cbca042be0d04291622a76d4f9c5deae9754ed53561d6e9 created a) |
| W-N9-02 | CONFIRMED | violated | `arm_admin_actuator` | 5 of 5 explicit proposes produced a block (height 3 -> 8): a caller-driven propose is not paced, which is C196's deliberate exemption |
