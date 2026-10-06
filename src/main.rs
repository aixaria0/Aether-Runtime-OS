use aether_runtime_os::receipt::export_receipt_from_db;
use aether_runtime_os::runtime::{audit, demo};
use aether_runtime_os::worker::worker_main;
use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::path::PathBuf;

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("worker") => {
            let id = arg_value(&args, "--id").context("worker requires --id")?;
            let key = arg_value(&args, "--key")
                .map(PathBuf::from)
                .context("worker requires --key")?;
            worker_main(&id, &key)
        }
        Some("audit") => {
            let path = arg_value(&args, "--db")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".aether/runtime.db"));
            audit(&path)
        }
        Some("receipt") => {
            let selector = args
                .get(2)
                .filter(|value| !value.starts_with("--"))
                .context("receipt requires an execution id or `latest`")?;
            let db = arg_value(&args, "--db")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".aether/runtime.db"));
            let receipt = export_receipt_from_db(&db, selector)?;
            let json = serde_json::to_string_pretty(&receipt)?;
            if let Some(output) = arg_value(&args, "--out").map(PathBuf::from) {
                if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&output, format!("{json}\n"))?;
                println!("AETHER RECEIPT EXPORTED: {}", output.display());
            } else {
                println!("{json}");
            }
            Ok(())
        }
        Some("demo") | None => demo(),
        Some(other) => {
            bail!("unknown command: {other}. Use `demo`, `audit`, `receipt`, or `worker`.")
        }
    }
}
