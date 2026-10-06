# Verification Model

## Integrity is not authenticity

A SHA-256 hash demonstrates integrity relative to bytes. It does not identify who produced those bytes.

Aether therefore separates:

- **integrity**: the result re-hashes to the recorded digest;
- **authenticity**: the digest binding is signed by the worker's Ed25519 key;
- **identity continuity**: a worker ID must keep the same public key across restart;
- **lineage consistency**: persisted transitions must replay through the allowed state machine;
- **materialization consistency**: the replayed terminal state must equal the current execution snapshot.

## Evidence acceptance

For an evidence envelope `E`, the verifier accepts only when all conditions hold:

1. `SHA256(E.result) == E.result_hash`
2. the public key matches the runtime's known identity for `E.worker_id`
3. the Ed25519 signature verifies over `E.execution_id:E.result_hash`

Only then may the execution move from `Verifying` to `Completed`.

## Replay acceptance

For a journal lineage `J`, the replay verifier accepts only when:

1. the first event is `None -> Created`;
2. every later event belongs to the same execution;
3. every `from_state` equals the state reconstructed from prior events;
4. every state edge belongs to the explicit transition contract;
5. the replayed terminal state equals the materialized execution snapshot.

Replay is deterministic: it does not call workers or regenerate outputs. It evaluates the persisted causal trace that the runtime claims happened.

## First-divergence verdict

A replay failure returns a structured first-divergence record containing:

- journal sequence (`seq`) when available;
- zero-based event index;
- expected replay state;
- observed `from_state`;
- reason for rejection.

The verifier stops at the earliest inconsistency rather than allowing later events to hide it.

## Negative controls

The end-to-end demo carries two independent negative controls.

### Evidence negative control

A valid evidence envelope is copied, its `result` is mutated, and the original hash/signature are retained. Verification must reject it.

### Replay negative control

A valid journal lineage is copied and the `from_state` of its `Running` event is replaced with `GhostState`. Replay must reject that lineage at the mutated edge.

If either negative control is accepted, the corresponding verifier is invalid.

## Current boundary

v0.2 can support the local verdict:

```text
RuntimeVerified + ReplayVerified
```

This means the runtime has verified its local evidence contract and its persisted state-transition lineage under the implemented replay rules.

It cannot yet support:

- `IndependentlyVerified`
- `FormallyVerified`

It also does not claim distributed safety, external side-effect determinism, or Byzantine fault tolerance.
