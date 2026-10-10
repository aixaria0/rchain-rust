# Rholox — an evidence-first RChain public-testnet application

**Website:** https://rholox.com/  
**Source:** https://github.com/aixaria0/Rholox  
**Review matrix:** https://github.com/aixaria0/Rholox/blob/main/docs/REVIEW.md

Rholox is an independently maintained browser-based application demonstrating public-testnet interactions with an RChain Rust node. It is a reference to an external application, **not** a component shipped or security-audited by the node repository.

## What the application currently implements

| Surface | Behavior |
| --- | --- |
| Network console | Reads node status, shard, heights, peer and validator information, and advertised capabilities. |
| Session wallet | Generates/imports secp256k1 keys in browser memory and derives REV addresses. |
| Test REV faucet and balance | Requests test funds and reads balances through the public API. |
| Proof anchoring | Hashes content locally, signs a deploy, and checks inclusion and subsequent state visibility. |
| Explorer | Queries recent blocks, block details, and deploy execution status. |
| REV transfer | Validates input, signs a test-value transfer locally, polls inclusion, and checks recipient balance observations. |

**Not implemented:** Asset Lab, token issuance, DEX/liquidity features. The project makes no mainnet-readiness or production-custody claim.

## Architecture and trust boundary

```text
Rholox browser client
  ├── local session keys and signing
  ├── node observation / block explorer
  ├── proof deployment
  └── test REV transfer
          │ public HTTP API
          ▼
https://testnet.rhobot.net
```

The client does not require node-admin endpoints. Browser-held keys are not persisted by the application; this does **not** imply that a browser session is equivalent to a hardened custody solution. Use disposable testnet keys and test-value funds only.

Deploy submission, inclusion, finalized-state observations, and transaction-specific finality are different claims. In particular, an increase in a recipient balance after inclusion is useful corroborating evidence but **does not, by itself, prove that the specific transfer was finalized**. Review the implementation and testnet behavior before treating a UI receipt as an authoritative finality certificate.

## Reproduce locally

From the [Rholox source repository](https://github.com/aixaria0/Rholox):

```sh
npm ci
npm run verify
npm audit --audit-level=high
npm run dev
```

The `verify` script runs source checks and a TypeScript/Vite production build. These checks do not replace live testnet integration testing. For the manual smoke-test sequence, implementation mapping, and explicit non-claims, see the [Rholox review matrix](https://github.com/aixaria0/Rholox/blob/main/docs/REVIEW.md).

## Relationship to rchain-rust

Rholox is developed and versioned in its own repository. This documentation entry links a concrete public-testnet application to the [application-development guide](building-apps.md) and the [public testnet guide](../node/testnet.md); it does not vendor frontend code, change consensus rules, or imply endorsement by RChain maintainers.
