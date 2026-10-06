# Architecture

## Core invariant

No execution becomes `Completed` merely because a worker returned bytes. Completion requires a result plus a valid evidence envelope accepted by the runtime verifier.

## v0.1 process model

```text
Controller
  |-- worker-1 (OS process)
  |-- worker-2 (OS process)
  `-- worker-3 (OS process)

Controller -> SQLite/WAL execution journal
Worker     -> signed Evidence envelope
Controller -> verification -> state transition
```

Workers communicate with the controller over line-delimited JSON through process pipes. This is intentionally local for v0.1; the protocol boundary is designed to be transport-independent so a later version can move to gRPC/NATS without changing the evidence model.

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

Every transition is appended to the journal.

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

This proves a narrow property only: local crash recovery for this protocol slice. It does not yet prove network-partition tolerance, exactly-once effects, or distributed consensus.
