use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct TaskRequest {
    execution_id: String,
    payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Evidence {
    worker_id: String,
    execution_id: String,
    result: String,
    result_hash: String,
    public_key: String,
    signature: String,
}

struct Journal {
    conn: Connection,
}

impl Journal {
    fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                state TEXT NOT NULL,
                worker_id TEXT,
                result_hash TEXT,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                execution_id TEXT NOT NULL,
                from_state TEXT,
                to_state TEXT NOT NULL,
                worker_id TEXT,
                ts INTEGER NOT NULL
            );
            ",
        )?;
        Ok(Self { conn })
    }

    fn create(&self, execution_id: &str, payload: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO executions (id, payload, state, updated_at) VALUES (?1, ?2, 'Created', ?3)",
            params![execution_id, payload, now_unix()],
        )?;
        self.conn.execute(
            "INSERT INTO events (execution_id, from_state, to_state, ts) VALUES (?1, NULL, 'Created', ?2)",
            params![execution_id, now_unix()],
        )?;
        Ok(())
    }

    fn transition(
        &self,
        execution_id: &str,
        from: &str,
        to: &str,
        worker_id: Option<&str>,
        result_hash: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE executions
             SET state=?2, worker_id=COALESCE(?3, worker_id), result_hash=COALESCE(?4, result_hash), updated_at=?5
             WHERE id=?1 AND state=?6",
            params![execution_id, to, worker_id, result_hash, now_unix(), from],
        )?;
        if self.conn.changes() != 1 {
            bail!("invalid transition for {execution_id}: expected current state {from}");
        }
        self.conn.execute(
            "INSERT INTO events (execution_id, from_state, to_state, worker_id, ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![execution_id, from, to, worker_id, now_unix()],
        )?;
        Ok(())
    }

    fn completed_count(&self) -> Result<u64> {
        let value: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM executions WHERE state='Completed'",
            [],
            |row| row.get(0),
        )?;
        Ok(value as u64)
    }
}

struct WorkerHandle {
    id: String,
    key_path: PathBuf,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    expected_public_key: Option<String>,
}

impl WorkerHandle {
    fn spawn(id: &str, key_path: PathBuf) -> Result<Self> {
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let exe = env::current_exe()?;
        let mut child = Command::new(exe)
            .arg("worker")
            .arg("--id")
            .arg(id)
            .arg("--key")
            .arg(&key_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn {id}"))?;
        let stdin = BufWriter::new(child.stdin.take().context("worker stdin unavailable")?);
        let stdout = BufReader::new(child.stdout.take().context("worker stdout unavailable")?);
        Ok(Self {
            id: id.to_string(),
            key_path,
            child,
            stdin,
            stdout,
            expected_public_key: None,
        })
    }

    fn send(&mut self, request: &TaskRequest) -> Result<Evidence> {
        serde_json::to_writer(&mut self.stdin, request)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            bail!("{} exited before returning evidence", self.id);
        }
        let evidence: Evidence = serde_json::from_str(line.trim())?;
        if evidence.worker_id != self.id {
            bail!(
                "worker identity mismatch: expected {}, got {}",
                self.id,
                evidence.worker_id
            );
        }
        match &self.expected_public_key {
            Some(expected) if expected != &evidence.public_key => {
                bail!("public key changed for {}", self.id)
            }
            None => self.expected_public_key = Some(evidence.public_key.clone()),
            _ => {}
        }
        verify_evidence(&evidence)?;
        Ok(evidence)
    }

    fn kill(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        let expected = self.expected_public_key.clone();
        let id = self.id.clone();
        let key_path = self.key_path.clone();
        let mut fresh = Self::spawn(&id, key_path)?;
        fresh.expected_public_key = expected;
        *self = fresh;
        Ok(())
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn signing_message(execution_id: &str, result_hash: &str) -> String {
    format!("{execution_id}:{result_hash}")
}

fn verify_evidence(evidence: &Evidence) -> Result<()> {
    let computed = sha256_hex(evidence.result.as_bytes());
    if computed != evidence.result_hash {
        bail!("result hash mismatch");
    }

    let key_bytes: [u8; 32] = hex::decode(&evidence.public_key)?
        .try_into()
        .map_err(|_| anyhow!("invalid Ed25519 public key length"))?;
    let sig_bytes: [u8; 64] = hex::decode(&evidence.signature)?
        .try_into()
        .map_err(|_| anyhow!("invalid Ed25519 signature length"))?;
    let verifying_key = VerifyingKey::from_bytes(&key_bytes)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verifying_key.verify(
        signing_message(&evidence.execution_id, &evidence.result_hash).as_bytes(),
        &signature,
    )?;
    Ok(())
}

fn load_or_create_signing_key(path: &Path) -> Result<SigningKey> {
    if path.exists() {
        let bytes: [u8; 32] = fs::read(path)?
            .try_into()
            .map_err(|_| anyhow!("invalid worker key length at {}", path.display()))?;
        return Ok(SigningKey::from_bytes(&bytes));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let key = SigningKey::generate(&mut OsRng);
    fs::write(path, key.to_bytes())?;
    Ok(key)
}

fn worker_main(worker_id: &str, key_path: &Path) -> Result<()> {
    let signing_key = load_or_create_signing_key(key_path)?;
    eprintln!("{worker_id}: online");

    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for line in BufReader::new(stdin.lock()).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: TaskRequest = serde_json::from_str(&line)?;
        let result = format!("processed:{}:{}", worker_id, request.payload);
        let result_hash = sha256_hex(result.as_bytes());
        let message = signing_message(&request.execution_id, &result_hash);
        let signature = signing_key.sign(message.as_bytes());
        let evidence = Evidence {
            worker_id: worker_id.to_string(),
            execution_id: request.execution_id,
            result,
            result_hash,
            public_key: hex::encode(signing_key.verifying_key().to_bytes()),
            signature: hex::encode(signature.to_bytes()),
        };
        serde_json::to_writer(&mut stdout, &evidence)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

fn run_one(journal: &Journal, worker: &mut WorkerHandle, payload: &str) -> Result<Evidence> {
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

fn demo() -> Result<()> {
    let root = PathBuf::from(".aether");
    fs::create_dir_all(root.join("keys"))?;
    let journal = Journal::open(&root.join("runtime.db"))?;

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

    println!("AETHER v0.1 DEMO PASS");
    println!("workers: 3 real OS processes");
    println!("completed executions: {}", journal.completed_count()?);
    println!("crash recovery: PASS");
    println!("signed evidence verification: PASS");
    println!("negative control (tampered result): REJECTED");
    println!("verification boundary: RuntimeVerified");
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("worker") => {
            let id = args
                .windows(2)
                .find(|pair| pair[0] == "--id")
                .map(|pair| pair[1].clone())
                .context("worker requires --id")?;
            let key = args
                .windows(2)
                .find(|pair| pair[0] == "--key")
                .map(|pair| PathBuf::from(&pair[1]))
                .context("worker requires --key")?;
            worker_main(&id, &key)
        }
        Some("demo") | None => demo(),
        Some(other) => bail!("unknown command: {other}. Use `demo` or `worker`."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_fixture() -> Evidence {
        let key = SigningKey::generate(&mut OsRng);
        let execution_id = "test-execution".to_string();
        let result = "deterministic-result".to_string();
        let result_hash = sha256_hex(result.as_bytes());
        let signature = key.sign(signing_message(&execution_id, &result_hash).as_bytes());
        Evidence {
            worker_id: "worker-test".to_string(),
            execution_id,
            result,
            result_hash,
            public_key: hex::encode(key.verifying_key().to_bytes()),
            signature: hex::encode(signature.to_bytes()),
        }
    }

    #[test]
    fn valid_evidence_is_accepted() {
        verify_evidence(&signed_fixture()).expect("valid evidence must verify");
    }

    #[test]
    fn tampered_result_is_rejected() {
        let mut evidence = signed_fixture();
        evidence.result.push_str("-tampered");
        assert!(verify_evidence(&evidence).is_err());
    }
}
