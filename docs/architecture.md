# Architecture

## Core invariant

No execution becomes `Completed` merely because a worker returned bytes. Completion requires a result plus a valid evidence envelope accepted by the runtime verifier.

A second v0.2 invariant is now explicit: a materialized execution state is not trusted merely because it exists in the `executions` table. The append-only event lineage must replay to the same terminal state through allowed transitions.

## v0.2 module boundaries

```text
src/
  protocol.rs   wire/domain envelopes
  evidence.rs   hashing, signing, verification
  journal.rs    SQLite/WAL persistence + migration
  worker.rs     OS process lifecycle + IPC
  runtime.rs    orchestration + recovery
  replay.rs     deterministic replay + divergence detection
  lib.rs        library surface
  main.rs       CLI only
```

The split is intentional: replay and evidence verification are pure enough to test independently from process orchestration.

## Process model

```text
Controller
  |-- worker-1 (OS process)
  |-- worker-2 (OS process)
  `-- worker-3 (OS process)

Controller -> SQLite/WAL execution journal
Worker     -> signed Evidence envelope
Controller -> verification -> state transition
Journal    -> replay engine -> replay verdict
```

Workers communicate with the controller over line-delimited JSON through process pipes. This is intentionally local; the protocol and evidence boundaries remain separate from transport so later versions can move to a network transport without redefining evidence semantics.

## Execution states

```text
Created
  -> Scheduled
  -> Running
  -> Verifying
  -> Completed

Running
  -> Failed
  -> Recovering
  -> Verifying
  -> Completed
```

Every transition is appended to the journal with sequence order. v0.2 also stores `result_hash` on transition events when available. Existing v0.1 databases are migrated in place by adding that event column if missing.

## Evidence envelope

Each worker response carries:

- `worker_id`
- `execution_id`
- `result`
- `result_hash` (SHA-256)
- `public_key` (Ed25519)
- `signature`

The signature covers `execution_id:result_hash`. The controller recomputes the result hash, checks worker-key continuity, and verifies the signature before accepting the result.

## Recovery semantics

The demo injects a real process failure by terminating `worker-2`. The next execution routed to that worker fails, the runtime records `Failed -> Recovering`, restarts the worker using its persisted key material, retries the same execution, then verifies and commits the result.

## Replay and first divergence

Replay consumes the ordered event lineage for one execution and reconstructs state from `Created` forward. For every event it checks:

1. the event belongs to the same execution;
2. `from_state` equals the state reconstructed so far;
3. the edge is allowed by the transition contract;
4. the replayed terminal state matches the materialized execution snapshot.

The verifier stops at the first violated condition and returns its event index and journal sequence. This is deliberately different from merely asking whether the last row says `Completed`.

The end-to-end demo includes a negative replay control: it clones a valid lineage, mutates the `from_state` of its `Running` event to `GhostState`, and requires the replay verifier to reject it at that exact edge.

## Current limits

The implementation proves a narrow local property set only. It does not yet prove:

- network-partition tolerance;
- exactly-once external side effects;
- distributed consensus;
- Byzantine worker tolerance;
- independent reproduction on a second runtime;
- formal correctness of the transition system.
