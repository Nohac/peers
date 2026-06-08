use std::path::PathBuf;

use anyhow::{Context, Result};
use facet::Facet;
use tokio::fs;

use crate::comments::Author;
use crate::diff::ReviewTarget;
use crate::nvim::NvimLspServer;
use crate::realtime::{ReviewUpdateBroadcaster, run_realtime_watcher};
use crate::review::{ensure_storage, now_rfc3339, peers_paths};
use crate::review_provider::ReviewProvider;

const SESSION_ENCODE_ERROR: &str = "failed to encode Peers session info";
const SIGTERM_HANDLER_ERROR: &str = "failed to install SIGTERM handler";
const CTRL_C_LISTEN_ERROR: &str = "failed to listen for Ctrl-C";
const REALTIME_WATCHER_ERROR: &str = "Peers realtime watcher stopped";

pub struct LocalServer {
    nvim_lsp: NvimLspServer,
    nvim_listen: Option<String>,
    provider: ReviewProvider,
}

impl LocalServer {
    pub async fn bind(
        repo_root: PathBuf,
        target: ReviewTarget,
        author: Author,
        nvim_listen: Option<String>,
    ) -> Result<Self> {
        ensure_storage(&repo_root).await?;
        let updates = ReviewUpdateBroadcaster::new();
        let provider = ReviewProvider::new(repo_root, target, author, updates);
        let nvim_lsp = NvimLspServer::bind(provider.clone()).await?;

        Ok(Self {
            nvim_lsp,
            nvim_listen,
            provider,
        })
    }

    pub fn nvim_lsp_url(&self) -> String {
        self.nvim_lsp.url()
    }

    pub async fn run_until_shutdown(self) -> Result<()> {
        let Self {
            nvim_lsp,
            nvim_listen,
            provider,
        } = self;
        let session_path = peers_paths(provider.repo_root()).session;
        let session_info = ReviewSessionInfo {
            pid: std::process::id(),
            repo_root: provider.repo_root().display().to_string(),
            target_label: provider.target().label(),
            view_kind: session_view_kind(provider.target()).to_string(),
            nvim_lsp_url: nvim_lsp.url(),
            realtime: true,
            nvim_listen: nvim_listen.clone(),
            started_at: now_rfc3339()?.to_string(),
        };
        write_session_info(&session_path, &session_info).await?;

        let lsp_server = nvim_lsp.run();
        spawn_realtime_watcher(provider.repo_root().to_path_buf(), provider.updates());

        let result = tokio::select! {
            result = lsp_server => {
                result
            }
            result = shutdown_signal() => result,
        };

        let _ = fs::remove_file(session_path).await;
        result
    }
}

fn spawn_realtime_watcher(repo_root: PathBuf, updates: ReviewUpdateBroadcaster) {
    tokio::spawn(async move {
        if let Err(error) = run_realtime_watcher(repo_root, updates).await {
            eprintln!("{REALTIME_WATCHER_ERROR}: {error:#}");
        }
    });
}

#[derive(Debug, Facet)]
struct ReviewSessionInfo {
    pid: u32,
    repo_root: String,
    target_label: String,
    view_kind: String,
    nvim_lsp_url: String,
    realtime: bool,
    nvim_listen: Option<String>,
    started_at: String,
}

fn session_view_kind(target: &ReviewTarget) -> &'static str {
    match target {
        ReviewTarget::WorkingTree => "diff",
        ReviewTarget::Cached => "diff_cached",
        ReviewTarget::All => "diff_all",
        ReviewTarget::Branch { .. } => "review",
    }
}

async fn write_session_info(path: &std::path::Path, info: &ReviewSessionInfo) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let json = facet_json::to_string(info).context(SESSION_ENCODE_ERROR)?;
    fs::write(path, format!("{json}\n")).await?;
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context(SIGTERM_HANDLER_ERROR)?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.context(CTRL_C_LISTEN_ERROR)?;
            }
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context(CTRL_C_LISTEN_ERROR)?;
    }

    Ok(())
}
