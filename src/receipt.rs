use crate::evidence::sha256_hex;
use crate::journal::{ExecutionEvent, Journal};
use crate::replay::audit_execution;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const RECEIPT_SCHEMA: &str = "aether-execution-receipt/v1";
pub const RECEIPT_SCOPE: &str = "LOCAL_PERSISTED_EXECUTION_JOURNAL_ONLY";
pub const STATE_MACHINE: &str = "aether.execution-state-machine/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEvent {
    pub index: usize,
    pub from_state: Option<String>,
    pub to_state: String,
    pub worker_id: Option<String>,
    pub result_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub schema: String,
    pub scope: String,
    pub runtime: String,
    pub runtime_version: String,
    pub state_machine: String,
    pub execution_id: String,
    pub terminal_state: String,
    pub payload_hash: String,
    pub result_hash: String,
    pub worker_id: String,
    pub events: Vec<ReceiptEvent>,
    pub limitations: Vec<String>,
}

pub fn export_receipt(journal: &Journal, execution_id: &str) -> Result<ExecutionReceipt> {
    let report = audit_execution(journal, execution_id)?;
    if !report.valid {
        let reason = report
            .first_divergence
            .as_ref()
            .map(|d| d.reason.as_str())
            .unwrap_or("unknown replay divergence");
        bail!("cannot export divergent execution {execution_id}: {reason}");
    }
    if report.terminal_state.as_deref() != Some("Completed") {
        bail!("receipt v1 requires a Completed execution");
    }

    let snapshot = journal
        .snapshot(execution_id)?
        .context("execution snapshot is missing")?;
    let result_hash = snapshot
        .result_hash
        .clone()
        .context("Completed execution is missing result_hash")?;
    let worker_id = snapshot
        .worker_id
        .clone()
        .context("Completed execution is missing worker_id")?;
    let events = journal.events_for_execution(execution_id)?;
    let last = events.last().context("execution has no journal events")?;
    if last.to_state != "Completed" || last.result_hash.as_deref() != Some(result_hash.as_str()) {
        bail!("Completed snapshot is not bound to the terminal event result_hash");
    }

    Ok(ExecutionReceipt {
        schema: RECEIPT_SCHEMA.to_string(),
        scope: RECEIPT_SCOPE.to_string(),
        runtime: "aether-runtime-os".to_string(),
        runtime_version: env!("CARGO_PKG_VERSION").to_string(),
        state_machine: STATE_MACHINE.to_string(),
        execution_id: execution_id.to_string(),
        terminal_state: snapshot.state,
        payload_hash: sha256_hex(snapshot.payload.as_bytes()),
        result_hash,
        worker_id,
        events: events
            .iter()
            .enumerate()
            .map(|(index, event)| normalize_event(index, event))
            .collect(),
        limitations: vec![
            "Receipt normalizes away SQLite global sequence numbers and timestamps.".into(),
            "Payload bytes are not exported; payload_hash is SHA-256 of the persisted payload.".into(),
            "Receipt does not carry worker Ed25519 signatures; Aether verified them before completion.".into(),
            "Receipt establishes local persisted lineage consistency only, not independent observation.".into(),
        ],
    })
}

pub fn export_receipt_from_db(path: &Path, selector: &str) -> Result<ExecutionReceipt> {
    let journal = Journal::open(path)?;
    let execution_id = if selector == "latest" {
        journal
            .execution_ids()?
            .pop()
            .context("journal contains no executions")?
    } else {
        selector.to_string()
    };
    export_receipt(&journal, &execution_id)
}

fn normalize_event(index: usize, event: &ExecutionEvent) -> ReceiptEvent {
    ReceiptEvent {
        index,
        from_state: event.from_state.clone(),
        to_state: event.to_state.clone(),
        worker_id: event.worker_id.clone(),
        result_hash: event.result_hash.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_db() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("aether-receipt-{}.db", Uuid::new_v4()))
    }

    #[test]
    fn completed_execution_exports_deterministic_receipt_shape() {
        let path = temp_db();
        let journal = Journal::open(&path).expect("open journal");
        let id = "exec-receipt";
        let hash = sha256_hex(b"result");
        journal.create(id, "secret-payload").unwrap();
        journal
            .transition(id, "Created", "Scheduled", Some("worker-1"), None)
            .unwrap();
        journal
            .transition(id, "Scheduled", "Running", Some("worker-1"), None)
            .unwrap();
        journal
            .transition(id, "Running", "Verifying", Some("worker-1"), Some(&hash))
            .unwrap();
        journal
            .transition(
                id,
                "Verifying",
                "Completed",
                Some("worker-1"),
                Some(&hash),
            )
            .unwrap();

        let receipt = export_receipt(&journal, id).expect("export receipt");
        assert_eq!(receipt.schema, RECEIPT_SCHEMA);
        assert_eq!(receipt.scope, RECEIPT_SCOPE);
        assert_eq!(receipt.terminal_state, "Completed");
        assert_eq!(receipt.result_hash, hash);
        assert_eq!(receipt.payload_hash, sha256_hex(b"secret-payload"));
        assert_eq!(receipt.events.len(), 5);
        assert_eq!(receipt.events[0].index, 0);
        assert_eq!(receipt.events[0].from_state, None);
        assert_eq!(receipt.events[4].to_state, "Completed");

        drop(journal);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }

    #[test]
    fn incomplete_execution_cannot_be_exported() {
        let path = temp_db();
        let journal = Journal::open(&path).expect("open journal");
        journal.create("incomplete", "payload").unwrap();
        assert!(export_receipt(&journal, "incomplete").is_err());
        drop(journal);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }
}
