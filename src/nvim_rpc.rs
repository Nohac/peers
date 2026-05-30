use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::realtime::ReviewUpdateBroadcaster;

const NVIM_BINARY: &str = "nvim";
const SERVER_ARG: &str = "--server";
const REMOTE_EXPR_ARG: &str = "--remote-expr";
const REFRESH_ALL_EXPR: &str = "luaeval(\"require('peers.buffer').refresh_all()\")";
const NVIM_RPC_REFRESH_ERROR: &str = "Peers Neovim RPC refresh failed";
const NVIM_RPC_STATUS_ERROR: &str = "Neovim RPC refresh command failed";
const NVIM_RPC_EXEC_ERROR: &str = "failed to execute Neovim RPC refresh command";

pub fn spawn_nvim_refresh_notifier(updates: ReviewUpdateBroadcaster, listen: Option<String>) {
    let Some(listen) = listen.filter(|value| !value.is_empty()) else {
        return;
    };

    tokio::spawn(async move {
        let mut receiver = updates.subscribe();
        while receiver.recv().await.is_ok() {
            if let Err(error) = send_refresh(&listen).await {
                eprintln!("{NVIM_RPC_REFRESH_ERROR}: {error:#}");
            }
        }
    });
}

async fn send_refresh(listen: &str) -> Result<()> {
    let status = Command::new(NVIM_BINARY)
        .arg(SERVER_ARG)
        .arg(listen)
        .arg(REMOTE_EXPR_ARG)
        .arg(REFRESH_ALL_EXPR)
        .status()
        .await
        .context(NVIM_RPC_EXEC_ERROR)?;
    if !status.success() {
        bail!("{NVIM_RPC_STATUS_ERROR}: {status}");
    }
    Ok(())
}
