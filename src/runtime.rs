use crate::evidence::verify_evidence;
use crate::journal::Journal;
use crate::protocol::{Evidence, TaskRequest};
use crate::replay::{audit_all, replay_events};
use crate::worker::WorkerHandle;
use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn run_one(journal: &Journal, worker: &mut WorkerHandle, payload: &str) -> Result<Evidence> {
    let execution_id = Uuid::new_v4().to_string();
    let request = TaskRequest {
        execution_id: execution_id.clone(),
        payload: payload.to_string(),
    };
    journal.create(&execution_id, payload)?;
    journal.transition(
        &execution_id,
        "Created",
        "Scheduled",
        Some(&worker.id),
        None,
    )?;
    journal.transition(
        &execution_id,
        "Scheduled",
        "Running",
        Some(&worker.id),
        None,
    )?;

    match worker.send(&request) {
        Ok(evidence) => {
            journal.transition(
                &execution_id,
                "Running",
                "Verifying",
                Some(&worker.id),
                Some(&evidence.result_hash),
            )?;
            verify_evidence(&evidence)?;
            journal.transition(
                &execution_id,
                "Verifying",
                "Completed",
                Some(&worker.id),
                Some(&evidence.result_hash),
            )?;
            Ok(evidence)
        }
        Err(first_error) => {
            eprintln!("{}: execution failed: {first_error}", worker.id);
            journal.transition(&execution_id, "Running", "Failed", Some(&worker.id), None)?;
            journal.transition(
                &execution_id,
                "Failed",
                "Recovering",
                Some(&worker.id),
                None,
            )?;
            worker.restart()?;
            let evidence = worker
                .send(&request)
                .context("retry after worker restart failed")?;
            journal.transition(
                &execution_id,
                "Recovering",
                "Verifying",
                Some(&worker.id),
                Some(&evidence.result_hash),
            )?;
            verify_evidence(&evidence)?;
            journal.transition(
                &execution_id,
                "Verifying",
                "Completed",
                Some(&worker.id),
                Some(&evidence.result_hash),
            )?;
            Ok(evidence)
        }
    }
}

pub fn audit(journal_path: &Path) -> Result<()> {
    let journal = Journal::open(journal_path)?;
    let reports = audit_all(&journal)?;
    if reports.is_empty() {
        println!("no executions found");
        return Ok(());
    }

    let mut failed = 0usize;
    for report in &reports {
        if report.valid {
            println!(
                "PASS {} events={} terminal={}",
                report.execution_id,
                report.event_count,
                report.terminal_state.as_deref().unwrap_or("<none>")
            );
        } else {
            failed += 1;
            let divergence = report
                .first_divergence
                .as_ref()
                .expect("invalid report has divergence");
            println!(
                "FAIL {} seq={:?} index={} reason={}",
                report.execution_id, divergence.seq, divergence.event_index, divergence.reason
            );
        }
    }

    if failed > 0 {
        bail!("replay audit found {failed} divergent execution(s)");
    }
    println!("AETHER REPLAY AUDIT PASS: {} execution(s)", reports.len());
    Ok(())
}

pub fn demo() -> Result<()> {
    let root = env::var_os("AETHER_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".aether"));
    fs::create_dir_all(root.join("keys"))?;
    let journal_path = root.join("runtime.db");
    let journal = Journal::open(&journal_path)?;

    let mut workers = [
        WorkerHandle::spawn("worker-1", root.join("keys/worker-1.key"))?,
        WorkerHandle::spawn("worker-2", root.join("keys/worker-2.key"))?,
        WorkerHandle::spawn("worker-3", root.join("keys/worker-3.key"))?,
    ];

    let mut evidence = Vec::new();
    evidence.push(run_one(&journal, &mut workers[0], "alpha")?);
    evidence.push(run_one(&journal, &mut workers[1], "beta")?);
    evidence.push(run_one(&journal, &mut workers[2], "gamma")?);

    eprintln!("fault injection: killing worker-2");
    workers[1].kill()?;
    evidence.push(run_one(&journal, &mut workers[1], "delta-after-crash")?);
    evidence.push(run_one(&journal, &mut workers[0], "epsilon")?);
    evidence.push(run_one(&journal, &mut workers[2], "zeta")?);

    let mut tampered = evidence.last().context("missing evidence")?.clone();
    tampered.result.push_str(":tampered");
    if verify_evidence(&tampered).is_ok() {
        bail!("negative control failed: tampered evidence was accepted");
    }

    let reports = audit_all(&journal)?;
    if let Some(bad) = reports.iter().find(|report| !report.valid) {
        bail!("journal replay diverged for {}", bad.execution_id);
    }

    let first_execution = evidence.first().context("missing first evidence")?;
    let mut synthetic = journal.events_for_execution(&first_execution.execution_id)?;
    let mutation_index = synthetic
        .iter()
        .position(|event| event.to_state == "Running")
        .context("expected Running event")?;
    synthetic[mutation_index].from_state = Some("GhostState".to_string());
    let negative_replay = replay_events(&first_execution.execution_id, &synthetic);
    if negative_replay.valid || negative_replay.first_divergence.is_none() {
        bail!("negative control failed: replay divergence was not detected");
    }

    println!("AETHER v0.2 DEMO PASS");
    println!("workers: 3 real OS processes");
    println!("completed executions: {}", journal.completed_count()?);
    println!("crash recovery: PASS");
    println!("signed evidence verification: PASS");
    println!("negative control (tampered evidence): REJECTED");
    println!(
        "journal replay audit: PASS ({} execution(s))",
        reports.len()
    );
    println!("first-divergence negative control: DETECTED");
    println!("verification boundary: RuntimeVerified + ReplayVerified");
    Ok(())
}
