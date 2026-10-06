use aether_runtime_os::runtime::{audit, demo};
use aether_runtime_os::worker::worker_main;
use anyhow::{Context, Result, bail};
use std::env;
use std::path::PathBuf;

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
        Some("audit") => {
            let path = args
                .windows(2)
                .find(|pair| pair[0] == "--db")
                .map(|pair| PathBuf::from(&pair[1]))
                .unwrap_or_else(|| PathBuf::from(".aether/runtime.db"));
            audit(&path)
        }
        Some("demo") | None => demo(),
        Some(other) => bail!("unknown command: {other}. Use `demo`, `audit`, or `worker`."),
    }
}
