# Aether Runtime OS

A Rust-first, fault-tolerant execution runtime that treats **execution evidence** as a first-class artifact.

The initial `v0.1` slice is intentionally small but real: it launches three independent OS worker processes, journals state transitions in SQLite/WAL, detects a killed worker, restarts it, retries the interrupted execution, verifies Ed25519-signed evidence, and proves the verifier rejects a tampered result.

## Why this exists

Agent frameworks can generate plans and call tools. A runtime has a harder job: it must know what ran, survive partial failure, preserve execution lineage, and distinguish *claimed* success from *verified* success.

Aether is built around this pipeline:

```text
request -> schedule -> execute -> observe -> verify -> recover -> journal
```

## v0.1 capabilities

- 3 real worker processes (not simulated actors)
- explicit execution state transitions
- persistent SQLite journal with WAL enabled
- crash detection and worker restart
- stable per-worker Ed25519 key material across restart
- SHA-256 result hashing
- signed evidence envelopes
- worker identity continuity check
- negative control: tampered output must fail verification
- CI for format, lint, tests, and end-to-end demo

## Run

```bash
cargo run -- demo
```

The demo intentionally kills `worker-2` during execution. The controller records failure, enters recovery, restarts the same worker identity, retries the task, verifies the returned evidence, and completes the workflow.

Local runtime state is stored under `.aether/` and ignored by git.

## Verification levels

Aether does not collapse every kind of confidence into the word "verified". The intended ladder is:

```text
Claimed
Observed
RuntimeVerified
ReplayVerified
IndependentlyVerified
FormallyVerified
```

The current v0.1 demo reaches **RuntimeVerified** for its local evidence contract. It does not claim external or formal verification.

## Architecture

See [`docs/architecture.md`](docs/architecture.md) and [`docs/verification-model.md`](docs/verification-model.md).

## Roadmap

1. Protocol + state machine
2. Persistent single-node runtime
3. Multi-process workers
4. Recovery semantics
5. Evidence plane
6. Replay + divergence detection
7. Observability
8. Agent mesh
9. Distributed transport
10. Cluster deployment

The order is deliberate: agents are consumers of the runtime, not the runtime itself.
