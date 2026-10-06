# CHIMERA-OMEGA Receipt Boundary

Aether exports a deterministic, normalized execution receipt for consumption by `aixaria0/CHIMERA-OMEGA` without taking a code dependency on CHIMERA.

## Producer boundary

Command:

```bash
cargo run -- receipt latest --db .aether/runtime.db --out receipt.json
```

Schema: `aether-execution-receipt/v1`

The receipt is exportable only when the selected execution:

1. replays successfully against Aether's implemented state machine;
2. terminates in `Completed`;
3. has a materialized worker identity and result hash;
4. has a terminal journal event bound to the same result hash.

## Deterministic normalization

The cross-repository receipt deliberately excludes SQLite global sequence numbers and timestamps. Those values describe storage position and wall-clock observation, not the normalized execution lineage.

It exports:

- execution id;
- runtime name and version;
- state-machine contract id;
- terminal state;
- SHA-256 payload hash, not payload bytes;
- result hash;
- worker id;
- ordered local event indexes and normalized state transitions.

## Verification boundary

Aether verifies worker Ed25519 evidence before an execution becomes `Completed`, but the v1 cross-repository receipt does **not** carry worker public keys or signatures. A downstream consumer therefore must not claim to have independently authenticated the original worker signature from this receipt alone.

The receipt supports the narrower statement:

> Aether exported a normalized record of a locally completed execution whose persisted lineage passed Aether's replay contract at export time.

CHIMERA-OMEGA is expected to independently validate the receipt schema and transition sequence, content-address the imported receipt under its own encoding rules, and state its own evidence boundary explicitly.

## Repository ownership rule

The project-level integration is intentionally limited to repositories owned by `aixaria0`:

- `aixaria0/Aether-Runtime-OS` — receipt producer;
- `aixaria0/CHIMERA-OMEGA` — evidence adapter and verifier.

No third-party project repository is required by this integration contract.
