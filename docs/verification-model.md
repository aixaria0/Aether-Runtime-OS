# Verification Model

## Integrity is not authenticity

A SHA-256 hash demonstrates integrity relative to bytes. It does not identify who produced those bytes.

Aether therefore separates:

- **integrity**: the result re-hashes to the recorded digest;
- **authenticity**: the digest binding is signed by the worker's Ed25519 key;
- **identity continuity**: a worker ID must keep the same public key across restart.

## Evidence acceptance

For an evidence envelope `E`, the verifier accepts only when all conditions hold:

1. `SHA256(E.result) == E.result_hash`
2. the public key matches the runtime's known identity for `E.worker_id`
3. the Ed25519 signature verifies over `E.execution_id:E.result_hash`

Only then may the execution move from `Verifying` to `Completed`.

## Negative control

The end-to-end demo copies a valid evidence envelope, mutates the result, and keeps the original hash/signature. Verification must reject it.

A verifier that accepts this negative control is considered invalid.

## Current boundary

v0.1 can support the verdict:

```text
RuntimeVerified
```

for the local evidence contract.

It cannot yet support:

- `ReplayVerified`
- `IndependentlyVerified`
- `FormallyVerified`

Those require additional machinery and evidence.
