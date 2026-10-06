use crate::protocol::Evidence;
use anyhow::{Result, anyhow, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn signing_message(execution_id: &str, result_hash: &str) -> String {
    format!("{execution_id}:{result_hash}")
}

pub fn verify_evidence(evidence: &Evidence) -> Result<()> {
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

pub fn load_or_create_signing_key(path: &Path) -> Result<SigningKey> {
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

pub fn sign_result(
    worker_id: &str,
    execution_id: String,
    result: String,
    key: &SigningKey,
) -> Evidence {
    let result_hash = sha256_hex(result.as_bytes());
    let signature = key.sign(signing_message(&execution_id, &result_hash).as_bytes());
    Evidence {
        worker_id: worker_id.to_string(),
        execution_id,
        result,
        result_hash,
        public_key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(signature.to_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_fixture() -> Evidence {
        let key = SigningKey::generate(&mut OsRng);
        sign_result(
            "worker-test",
            "test-execution".to_string(),
            "deterministic-result".to_string(),
            &key,
        )
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
