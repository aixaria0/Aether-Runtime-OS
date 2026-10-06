use crate::journal::{ExecutionEvent, ExecutionSnapshot, Journal};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Divergence {
    pub seq: Option<i64>,
    pub event_index: usize,
    pub expected_state: Option<String>,
    pub observed_from: Option<String>,
    pub observed_to: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayReport {
    pub execution_id: String,
    pub valid: bool,
    pub terminal_state: Option<String>,
    pub event_count: usize,
    pub first_divergence: Option<Divergence>,
}

pub fn allowed_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("Created", "Scheduled")
            | ("Scheduled", "Running")
            | ("Running", "Verifying")
            | ("Verifying", "Completed")
            | ("Running", "Failed")
            | ("Failed", "Recovering")
            | ("Recovering", "Verifying")
    )
}

pub fn replay_events(execution_id: &str, events: &[ExecutionEvent]) -> ReplayReport {
    if events.is_empty() {
        return invalid_report(
            execution_id,
            0,
            None,
            0,
            None,
            None,
            "execution has no journal events",
        );
    }

    let first = &events[0];
    if first.execution_id != execution_id {
        return invalid_report(
            execution_id,
            events.len(),
            first.seq.into(),
            0,
            None,
            first.from_state.clone(),
            "first event belongs to a different execution",
        );
    }
    if first.from_state.is_some() || first.to_state != "Created" {
        return invalid_report(
            execution_id,
            events.len(),
            Some(first.seq),
            0,
            None,
            first.from_state.clone(),
            "first event must materialize Created from no prior state",
        );
    }

    let mut current = "Created".to_string();
    for (index, event) in events.iter().enumerate().skip(1) {
        if event.execution_id != execution_id {
            return invalid_report(
                execution_id,
                events.len(),
                Some(event.seq),
                index,
                Some(current),
                event.from_state.clone(),
                "event belongs to a different execution",
            );
        }

        if event.from_state.as_deref() != Some(current.as_str()) {
            return invalid_report(
                execution_id,
                events.len(),
                Some(event.seq),
                index,
                Some(current),
                event.from_state.clone(),
                "event from_state does not match replayed state",
            );
        }

        if !allowed_transition(&current, &event.to_state) {
            return invalid_report(
                execution_id,
                events.len(),
                Some(event.seq),
                index,
                Some(current.clone()),
                event.from_state.clone(),
                &format!(
                    "transition {} -> {} is not allowed",
                    current, event.to_state
                ),
            );
        }

        current = event.to_state.clone();
    }

    ReplayReport {
        execution_id: execution_id.to_string(),
        valid: true,
        terminal_state: Some(current),
        event_count: events.len(),
        first_divergence: None,
    }
}

pub fn verify_snapshot(report: ReplayReport, snapshot: Option<&ExecutionSnapshot>) -> ReplayReport {
    if !report.valid {
        return report;
    }

    let Some(snapshot) = snapshot else {
        return invalid_report(
            &report.execution_id,
            report.event_count,
            None,
            report.event_count,
            report.terminal_state.clone(),
            None,
            "materialized execution snapshot is missing",
        );
    };

    if report.terminal_state.as_deref() != Some(snapshot.state.as_str()) {
        return invalid_report(
            &report.execution_id,
            report.event_count,
            None,
            report.event_count,
            report.terminal_state.clone(),
            Some(snapshot.state.clone()),
            "materialized state diverges from replayed terminal state",
        );
    }

    report
}

pub fn audit_execution(journal: &Journal, execution_id: &str) -> Result<ReplayReport> {
    let events = journal.events_for_execution(execution_id)?;
    let snapshot = journal.snapshot(execution_id)?;
    Ok(verify_snapshot(
        replay_events(execution_id, &events),
        snapshot.as_ref(),
    ))
}

pub fn audit_all(journal: &Journal) -> Result<Vec<ReplayReport>> {
    let mut reports = Vec::new();
    for execution_id in journal.execution_ids()? {
        reports.push(audit_execution(journal, &execution_id)?);
    }
    Ok(reports)
}

fn invalid_report(
    execution_id: &str,
    event_count: usize,
    seq: Option<i64>,
    event_index: usize,
    expected_state: Option<String>,
    observed_from: Option<String>,
    reason: &str,
) -> ReplayReport {
    ReplayReport {
        execution_id: execution_id.to_string(),
        valid: false,
        terminal_state: expected_state.clone(),
        event_count,
        first_divergence: Some(Divergence {
            seq,
            event_index,
            expected_state,
            observed_from,
            observed_to: None,
            reason: reason.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: i64, id: &str, from: Option<&str>, to: &str) -> ExecutionEvent {
        ExecutionEvent {
            seq,
            execution_id: id.to_string(),
            from_state: from.map(str::to_string),
            to_state: to.to_string(),
            worker_id: Some("worker-1".to_string()),
            result_hash: None,
            ts: 0,
        }
    }

    #[test]
    fn valid_replay_reaches_completed() {
        let events = vec![
            event(1, "x", None, "Created"),
            event(2, "x", Some("Created"), "Scheduled"),
            event(3, "x", Some("Scheduled"), "Running"),
            event(4, "x", Some("Running"), "Verifying"),
            event(5, "x", Some("Verifying"), "Completed"),
        ];
        let report = replay_events("x", &events);
        assert!(report.valid);
        assert_eq!(report.terminal_state.as_deref(), Some("Completed"));
        assert!(report.first_divergence.is_none());
    }

    #[test]
    fn first_divergence_is_precise() {
        let events = vec![
            event(1, "x", None, "Created"),
            event(2, "x", Some("Created"), "Scheduled"),
            event(3, "x", Some("Ghost"), "Running"),
            event(4, "x", Some("Running"), "Completed"),
        ];
        let report = replay_events("x", &events);
        assert!(!report.valid);
        let divergence = report.first_divergence.expect("divergence expected");
        assert_eq!(divergence.seq, Some(3));
        assert_eq!(divergence.event_index, 2);
        assert_eq!(divergence.expected_state.as_deref(), Some("Scheduled"));
        assert_eq!(divergence.observed_from.as_deref(), Some("Ghost"));
    }

    #[test]
    fn illegal_edge_is_rejected() {
        let events = vec![
            event(1, "x", None, "Created"),
            event(2, "x", Some("Created"), "Completed"),
        ];
        let report = replay_events("x", &events);
        assert!(!report.valid);
        assert!(
            report
                .first_divergence
                .as_ref()
                .expect("divergence expected")
                .reason
                .contains("not allowed")
        );
    }
}
