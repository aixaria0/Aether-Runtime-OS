# Aether Runtime OS

A Rust-first, fault-tolerant execution runtime that treats **execution evidence** and **replayable execution lineage** as first-class artifacts.

The current `v0.2` slice is intentionally narrow but real: it launches three independent OS worker processes, journals state transitions in SQLite/WAL, detects a killed worker, restarts it, retries the interrupted execution, verifies Ed25519-signed evidence, replays persisted execution history against an explicit transition contract, and reports the first point where a trace diverges from that contract.

## Why this exists

Agent frameworks can generate plans and call tools. A runtime has a harder job: it must know what ran, survive partial failure, preserve execution lineage, distinguish *claimed* success from *verified* success, and later prove that the recorded history itself is internally coherent.

Aether is built around this pipeline:

```text
request -> schedule -> execute -> observe -> verify -> recover -> journal -> replay
```

## v0.2 capabilities

- 3 real worker processes (not simulated actors)
- explicit execution state transitions
- persistent SQLite journal with WAL enabled
- backward-compatible journal migration from v0.1
- crash detection and worker restart
- stable per-worker Ed25519 key material across restart
- SHA-256 result hashing
- signed evidence envelopes
- worker identity continuity check
- negative control: tampered output must fail verification
- deterministic journal replay against the transition contract
- first-divergence reporting with journal sequence and event index
- negative control: mutated execution history must be detected
- standalone `audit` command for persisted journals
- CI for format, lint, tests, end-to-end recovery, evidence verification, and replay audit

## Run

```bash
cargo run -- demo
```

The demo intentionally kills `worker-2` during execution. The controller records failure, enters recovery, restarts the same worker identity, retries the task, verifies the returned evidence, completes the workflow, then replays the persisted history.

Use an isolated runtime directory when desired:

```bash
AETHER_ROOT=.aether-test cargo run -- demo
cargo run -- audit --db .aether-test/runtime.db
```

The default local runtime state is stored under `.aether/` and ignored by git.

## Replay audit

```bash
cargo run -- audit
```

The replay engine does not re-run worker code. It reconstructs state from the append-only event lineage and checks each edge against the runtime transition contract. The first inconsistent edge becomes a machine-inspectable divergence rather than being hidden by a later terminal state.

## Verification levels

Aether does not collapse every kind of confidence into the word "verified". The ladder is:

```text
Claimed
Observed
RuntimeVerified
ReplayVerified
IndependentlyVerified
FormallyVerified
```

The current v0.2 demo reaches **RuntimeVerified + ReplayVerified** for its local evidence and journal contracts. It does not claim external, distributed, or formal verification.

## Architecture

See [`docs/architecture.md`](docs/architecture.md) and [`docs/verification-model.md`](docs/verification-model.md).

## Roadmap

1. Protocol + state machine
2. Persistent single-node runtime
3. Multi-process workers
4. Recovery semantics
5. Evidence plane
6. Replay + first-divergence detection
7. Evidence DAG + replay fingerprints
8. Observability
9. Agent mesh
10. Distributed transport
11. Cluster deployment

The order is deliberate: agents are consumers of the runtime, not the runtime itself.
