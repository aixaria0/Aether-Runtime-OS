use crate::evidence::{load_or_create_signing_key, sign_result, verify_evidence};
use crate::protocol::{Evidence, TaskRequest};
use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct WorkerHandle {
    pub id: String,
    key_path: PathBuf,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    expected_public_key: Option<String>,
}

impl WorkerHandle {
    pub fn spawn(id: &str, key_path: PathBuf) -> Result<Self> {
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

    pub fn send(&mut self, request: &TaskRequest) -> Result<Evidence> {
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

    pub fn kill(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }

    pub fn restart(&mut self) -> Result<()> {
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

pub fn worker_main(worker_id: &str, key_path: &Path) -> Result<()> {
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
        let evidence = sign_result(worker_id, request.execution_id, result, &signing_key);
        serde_json::to_writer(&mut stdout, &evidence)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}
