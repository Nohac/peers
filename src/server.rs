use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use tokio::net::TcpListener;

use crate::comments::Author;
use crate::rpc::{PeersReviewDispatcher, ReviewApi};

pub struct LocalServer {
    listener: TcpListener,
    addr: SocketAddr,
    token: String,
    repo_root: PathBuf,
    review_id: String,
    author: Author,
}

impl LocalServer {
    pub async fn bind(repo_root: PathBuf, review_id: String, author: Author) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("failed to bind local Peers Vox server")?;
        let addr = listener.local_addr()?;

        Ok(Self {
            listener,
            addr,
            token: new_token(),
            repo_root,
            review_id,
            author,
        })
    }

    pub fn vox_url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    pub fn frontend_url(&self) -> String {
        format!(
            "http://localhost:3000/?vox={}&token={}",
            self.vox_url(),
            self.token
        )
    }

    pub async fn run_until_shutdown(self) -> Result<()> {
        let ws_listener = vox::WsListener::from_tcp(self.listener);
        let api = ReviewApi::new(self.repo_root, self.review_id, self.author, self.token);
        let server = vox::serve_listener(ws_listener, PeersReviewDispatcher::new(api));

        tokio::select! {
            result = server => {
                result.map_err(|error| anyhow!("{error}"))?;
            }
            _ = tokio::signal::ctrl_c() => {}
        }

        Ok(())
    }
}

fn new_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}{:x}", std::process::id(), nanos)
}
