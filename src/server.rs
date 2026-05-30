use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use tokio::fs;
use tokio::net::TcpListener;

use crate::comments::Author;
use crate::nvim::NvimLspServer;
use crate::nvim_rpc::spawn_nvim_refresh_notifier;
use crate::realtime::{ReviewUpdateBroadcaster, run_realtime_watcher};
use crate::review::{now_rfc3339, review_paths};
use crate::review_provider::ReviewProvider;
use crate::rpc::{PeersReviewDispatcher, ReviewApi};

const LOOPBACK_BIND_HOST: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const FRONTEND_PORT: u16 = 3000;
const VOX_SCHEME: &str = "ws";
const HTTP_SCHEME: &str = "http";
const VOX_BIND_ERROR: &str = "failed to bind local Peers Vox server";
const SESSION_ENCODE_ERROR: &str = "failed to encode Peers session info";
const SIGTERM_HANDLER_ERROR: &str = "failed to install SIGTERM handler";
const CTRL_C_LISTEN_ERROR: &str = "failed to listen for Ctrl-C";
const REALTIME_WATCHER_ERROR: &str = "Peers realtime watcher stopped";

pub struct LocalServer {
    listener: TcpListener,
    addr: SocketAddr,
    nvim_lsp: NvimLspServer,
    token: String,
    nvim_listen: Option<String>,
    provider: ReviewProvider,
}

impl LocalServer {
    pub async fn bind(
        repo_root: PathBuf,
        review_id: String,
        author: Author,
        nvim_listen: Option<String>,
    ) -> Result<Self> {
        let listener = TcpListener::bind((LOOPBACK_BIND_HOST, 0))
            .await
            .context(VOX_BIND_ERROR)?;
        let addr = listener.local_addr()?;
        let updates = ReviewUpdateBroadcaster::new();
        let provider = ReviewProvider::new(repo_root, review_id, author, updates);
        let nvim_lsp = NvimLspServer::bind(provider.clone()).await?;

        Ok(Self {
            listener,
            addr,
            nvim_lsp,
            token: new_token(),
            nvim_listen,
            provider,
        })
    }

    pub fn vox_url(&self) -> String {
        format!("{VOX_SCHEME}://{LOCALHOST}:{}", self.addr.port())
    }

    pub fn nvim_lsp_url(&self) -> String {
        self.nvim_lsp.url()
    }

    pub fn frontend_url(&self) -> String {
        format!(
            "{HTTP_SCHEME}://{LOCALHOST}:{FRONTEND_PORT}/?vox={}&token={}",
            self.vox_url(),
            self.token
        )
    }

    pub async fn run_until_shutdown(self) -> Result<()> {
        let Self {
            listener,
            nvim_lsp,
            token,
            nvim_listen,
            provider,
            ..
        } = self;
        let session_path = review_paths(provider.repo_root(), provider.review_id()).session;
        let session_info = ReviewSessionInfo {
            pid: std::process::id(),
            review_id: provider.review_id().to_string(),
            vox_url: format!(
                "{VOX_SCHEME}://{LOCALHOST}:{}",
                listener.local_addr()?.port()
            ),
            nvim_lsp_url: nvim_lsp.url(),
            frontend_url: format!(
                "{HTTP_SCHEME}://{LOCALHOST}:{FRONTEND_PORT}/?vox={VOX_SCHEME}://{LOCALHOST}:{}&token={}",
                listener.local_addr()?.port(),
                token
            ),
            token: token.clone(),
            realtime: true,
            nvim_listen: nvim_listen.clone(),
            started_at: now_rfc3339()?,
        };
        write_session_info(&session_path, &session_info).await?;

        let ws_listener = vox::WsListener::from_tcp(listener);
        let api = ReviewApi::new(provider.clone(), token);
        let server = vox::serve_listener(ws_listener, PeersReviewDispatcher::new(api));
        let lsp_server = nvim_lsp.run();
        spawn_realtime_watcher(
            provider.repo_root().to_path_buf(),
            provider.review_id().to_string(),
            provider.updates(),
        );
        spawn_nvim_refresh_notifier(provider.updates(), nvim_listen);

        let result = tokio::select! {
            result = server => {
                result.map_err(|error| anyhow!("{error}"))
            }
            result = lsp_server => {
                result
            }
            result = shutdown_signal() => result,
        };

        let _ = fs::remove_file(session_path).await;
        result
    }
}

fn spawn_realtime_watcher(repo_root: PathBuf, review_id: String, updates: ReviewUpdateBroadcaster) {
    tokio::spawn(async move {
        if let Err(error) = run_realtime_watcher(repo_root, review_id, updates).await {
            eprintln!("{REALTIME_WATCHER_ERROR}: {error:#}");
        }
    });
}

#[derive(Debug, Facet)]
struct ReviewSessionInfo {
    pid: u32,
    review_id: String,
    vox_url: String,
    nvim_lsp_url: String,
    frontend_url: String,
    token: String,
    realtime: bool,
    nvim_listen: Option<String>,
    started_at: String,
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

fn new_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}{:x}", std::process::id(), nanos)
}
