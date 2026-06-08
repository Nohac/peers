use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use tokio::fs;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{Instant, sleep, timeout};

use crate::review::{ensure_storage, now_rfc3339, peers_paths};

mod codex;

const AGENT_KIND_CODEX: &str = "codex";
const WS_PREFIX: &str = "ws://";
const DEFAULT_LISTEN: &str = "ws";
const DEFAULT_AGENT_HOST: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const TEMPLATE_ADDR: &str = "%addr";
const TEMPLATE_HOST: &str = "%host";
const TEMPLATE_PORT: &str = "%port";
const TEMPLATE_REPO: &str = "%repo";
const TEMPLATE_SESSION: &str = "%session";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(8);
const RPC_TIMEOUT: Duration = Duration::from_secs(3);
const SERVER_START_ATTEMPTS: usize = 5;
const NO_AGENT_SESSION_ERROR: &str =
    "No usable Peers agent session. Run `peers agent codex` or `peers agent attach --addr <addr>`.";

#[derive(Debug, Clone)]
pub struct AgentLaunchRequest {
    pub listen: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentInvocationRequest {
    pub prompt: String,
}

#[derive(Debug, Facet)]
pub struct AgentInvocationResponse {
    pub thread_id: String,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum AgentTransport {
    Websocket,
}

#[derive(Debug, Facet)]
pub struct AgentSession {
    pub agent_kind: String,
    pub repo_root: String,
    pub address: String,
    pub transport: AgentTransport,
    pub host: String,
    pub port: u16,
    pub server_pid: Option<u32>,
    pub command: Vec<String>,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTemplate {
    pub address: String,
    pub host: String,
    pub port: u16,
    pub repo_root: PathBuf,
    pub session_path: PathBuf,
}

pub async fn launch_agent(repo_root: &Path, request: AgentLaunchRequest) -> Result<()> {
    let request = normalize_request(request);
    ensure_storage(repo_root).await?;

    let (endpoint, mut server) = start_codex_app_server(repo_root, &request.listen).await?;
    let template = AgentTemplate {
        address: endpoint.address.clone(),
        host: endpoint.host.clone(),
        port: endpoint.port,
        repo_root: repo_root.to_path_buf(),
        session_path: peers_paths(repo_root).agent_session,
    };
    let command = expand_command_template(&request.command, &template);
    if command.is_empty() {
        stop_child(&mut server).await;
        return Err(anyhow!("agent command cannot be empty"));
    }

    write_agent_session(
        repo_root,
        AgentSession {
            agent_kind: infer_agent_kind(&command),
            repo_root: repo_root.display().to_string(),
            address: endpoint.address,
            transport: AgentTransport::Websocket,
            host: endpoint.host,
            port: endpoint.port,
            server_pid: server.id(),
            command: command.clone(),
            started_at: now_rfc3339()?.to_string(),
        },
    )
    .await?;

    let status = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(repo_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("failed to launch agent command `{}`", command[0]))?;

    stop_child(&mut server).await;
    if !status.success() {
        return Err(anyhow!("agent command exited with status {status}"));
    }
    Ok(())
}

pub async fn attach_agent(repo_root: &Path, address: &str) -> Result<()> {
    let endpoint = parse_ws_endpoint(address)?;
    validate_app_server(&endpoint).await?;
    ensure_storage(repo_root).await?;
    write_agent_session(
        repo_root,
        AgentSession {
            agent_kind: AGENT_KIND_CODEX.to_string(),
            repo_root: repo_root.display().to_string(),
            address: endpoint.address,
            transport: AgentTransport::Websocket,
            host: endpoint.host,
            port: endpoint.port,
            server_pid: None,
            command: Vec::new(),
            started_at: now_rfc3339()?.to_string(),
        },
    )
    .await
}

pub async fn invoke_agent(
    repo_root: &Path,
    request: AgentInvocationRequest,
) -> Result<AgentInvocationResponse> {
    let session = load_agent_session(repo_root).await?;
    let thread_id = codex::invoke(&session.address, repo_root, &request.prompt).await?;
    Ok(AgentInvocationResponse { thread_id })
}

pub fn expand_command_template(command: &[String], template: &AgentTemplate) -> Vec<String> {
    command
        .iter()
        .map(|part| {
            part.replace(TEMPLATE_ADDR, &template.address)
                .replace(TEMPLATE_HOST, &template.host)
                .replace(TEMPLATE_PORT, &template.port.to_string())
                .replace(TEMPLATE_REPO, &template.repo_root.display().to_string())
                .replace(
                    TEMPLATE_SESSION,
                    &template.session_path.display().to_string(),
                )
        })
        .collect()
}

fn normalize_request(mut request: AgentLaunchRequest) -> AgentLaunchRequest {
    if matches!(request.command.first().map(String::as_str), Some("--")) {
        request.command.remove(0);
    }
    if request.command == [AGENT_KIND_CODEX.to_string()] {
        request.command = vec![
            AGENT_KIND_CODEX.to_string(),
            "--remote".to_string(),
            TEMPLATE_ADDR.to_string(),
        ];
    }
    request
}

fn infer_agent_kind(command: &[String]) -> String {
    command
        .first()
        .and_then(|command| Path::new(command).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| AGENT_KIND_CODEX.to_string())
}

#[derive(Debug)]
struct AgentEndpoint {
    address: String,
    host: String,
    port: u16,
}

fn allocated_ws_endpoint() -> Result<AgentEndpoint> {
    let listener = std::net::TcpListener::bind((DEFAULT_AGENT_HOST, 0))
        .context("failed to allocate a local agent port")?;
    let port = listener
        .local_addr()
        .context("failed to inspect allocated local agent port")?
        .port();
    drop(listener);
    Ok(AgentEndpoint {
        address: format!("{WS_PREFIX}{DEFAULT_AGENT_HOST}:{port}"),
        host: DEFAULT_AGENT_HOST.to_string(),
        port,
    })
}

fn parse_ws_endpoint(address: &str) -> Result<AgentEndpoint> {
    let Some(rest) = address.strip_prefix(WS_PREFIX) else {
        return Err(anyhow!(
            "only websocket agent addresses are supported for now, expected `ws://host:port`"
        ));
    };
    let host_port = rest
        .split_once('/')
        .map(|(host_port, path)| {
            if path.is_empty() {
                Ok(host_port)
            } else {
                Err(anyhow!(
                    "agent websocket address paths are not supported, expected `ws://host:port`"
                ))
            }
        })
        .transpose()?
        .unwrap_or(rest);
    let (host, port) = host_port
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("agent websocket address must include a port"))?;
    if host.is_empty() {
        return Err(anyhow!("agent websocket address must include a host"));
    }
    if host != DEFAULT_AGENT_HOST && host != LOCALHOST {
        return Err(anyhow!(
            "agent websocket address must use a loopback host (`127.0.0.1` or `localhost`)"
        ));
    }
    let port = port
        .parse::<u16>()
        .with_context(|| format!("invalid agent websocket port `{port}`"))?;
    Ok(AgentEndpoint {
        address: format!("{WS_PREFIX}{host}:{port}"),
        host: host.to_string(),
        port,
    })
}

async fn start_codex_app_server(repo_root: &Path, listen: &str) -> Result<(AgentEndpoint, Child)> {
    if listen != DEFAULT_LISTEN {
        let endpoint = parse_ws_endpoint(listen)?;
        let mut server = spawn_codex_app_server(repo_root, &endpoint.address).await?;
        if let Err(error) = wait_for_app_server(&endpoint).await {
            stop_child(&mut server).await;
            return Err(error);
        }
        return Ok((endpoint, server));
    }

    let mut last_error = None;
    for _ in 0..SERVER_START_ATTEMPTS {
        let endpoint = allocated_ws_endpoint()?;
        let mut server = spawn_codex_app_server(repo_root, &endpoint.address).await?;
        match wait_for_app_server(&endpoint).await {
            Ok(()) => return Ok((endpoint, server)),
            Err(error) => {
                stop_child(&mut server).await;
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("failed to start Codex app-server")))
}

async fn spawn_codex_app_server(repo_root: &Path, address: &str) -> Result<Child> {
    Command::new(AGENT_KIND_CODEX)
        .args(["app-server", "--listen", address])
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start `codex app-server`")
}

async fn wait_for_app_server(endpoint: &AgentEndpoint) -> Result<()> {
    let deadline = Instant::now() + SERVER_START_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for Codex app-server at {}",
                endpoint.address
            ));
        }
        if validate_app_server(endpoint).await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn validate_app_server(endpoint: &AgentEndpoint) -> Result<()> {
    timeout(
        RPC_TIMEOUT,
        TcpStream::connect((endpoint.host.as_str(), endpoint.port)),
    )
    .await
    .context("timed out validating Codex app-server")?
    .with_context(|| format!("failed to connect to {}", endpoint.address))?;
    Ok(())
}

async fn write_agent_session(repo_root: &Path, session: AgentSession) -> Result<()> {
    let paths = peers_paths(repo_root);
    fs::create_dir_all(&paths.root).await?;
    let json = facet_json::to_string(&session).context("failed to encode agent session")?;
    fs::write(paths.agent_session, format!("{json}\n"))
        .await
        .context("failed to write agent session")
}

async fn load_agent_session(repo_root: &Path) -> Result<AgentSession> {
    let path = peers_paths(repo_root).agent_session;
    let text = fs::read_to_string(&path)
        .await
        .with_context(|| format!("{NO_AGENT_SESSION_ERROR} Missing `{}`.", path.display()))?;
    let session =
        facet_json::from_str::<AgentSession>(&text).context("failed to decode agent session")?;
    if let Some(pid) = session.server_pid
        && !server_process_exists(pid).await
    {
        return Err(anyhow!(
            "{NO_AGENT_SESSION_ERROR} Session server process `{pid}` is no longer running."
        ));
    }
    validate_app_server(&AgentEndpoint {
        address: session.address.clone(),
        host: session.host.clone(),
        port: session.port,
    })
    .await
    .with_context(|| format!("{NO_AGENT_SESSION_ERROR} Session endpoint is not reachable."))?;
    Ok(session)
}

async fn server_process_exists(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        return fs::try_exists(format!("/proc/{pid}"))
            .await
            .unwrap_or(false);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

async fn stop_child(child: &mut Child) {
    if child.id().is_none() {
        return;
    }
    let _ = child.kill().await;
}
