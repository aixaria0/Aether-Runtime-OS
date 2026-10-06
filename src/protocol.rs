use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub execution_id: String,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub worker_id: String,
    pub execution_id: String,
    pub result: String,
    pub result_hash: String,
    pub public_key: String,
    pub signature: String,
}
