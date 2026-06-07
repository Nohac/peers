use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use futures::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const AGENT_INVOKE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_INITIALIZE: &str = "initialize";
const REQUEST_THREAD_LOADED_LIST: &str = "thread/loaded/list";
const REQUEST_THREAD_READ: &str = "thread/read";
const REQUEST_TURN_START: &str = "turn/start";
const NOTIFICATION_INITIALIZED: &str = "initialized";
const CLIENT_NAME: &str = "peers";
const CLIENT_TITLE: &str = "Peers";
const RESPONSE_SNIPPET_LIMIT: usize = 600;

pub async fn invoke(address: &str, repo_root: &Path, prompt: &str) -> Result<String> {
    let mut client = CodexAppClient::connect(address).await?;
    client.initialize().await?;
    let thread_id = client
        .repo_thread_id(repo_root)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "no loaded Codex thread found for `{}`. Start or resume a Codex conversation in this repository first, then retry the Peers agent action.",
                repo_root.display()
            )
        })?;
    client.start_turn(&thread_id, prompt, repo_root).await?;
    Ok(thread_id)
}

type CodexWs =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct CodexAppClient {
    ws: CodexWs,
    next_id: u32,
}

impl CodexAppClient {
    async fn connect(address: &str) -> Result<Self> {
        let (ws, _) = timeout(AGENT_INVOKE_TIMEOUT, connect_async(address))
            .await
            .context("timed out connecting to Codex app-server")?
            .with_context(|| format!("failed to connect to Codex app-server at `{address}`"))?;
        Ok(Self { ws, next_id: 1 })
    }

    async fn initialize(&mut self) -> Result<()> {
        let id = self.next_request_id();
        self.send_json(&CodexInitializeRequest {
            id,
            method: REQUEST_INITIALIZE,
            params: CodexInitializeParams {
                client_info: CodexClientInfo {
                    name: CLIENT_NAME,
                    title: CLIENT_TITLE,
                    version: env!("CARGO_PKG_VERSION"),
                },
                capabilities: CodexInitializeCapabilities {
                    experimental_api: true,
                    request_attestation: false,
                },
            },
        })
        .await?;
        let _: CodexResponse<CodexInitializeResponse> = self.response(id).await?;
        self.send_json(&CodexInitializedNotification {
            method: NOTIFICATION_INITIALIZED,
        })
        .await?;
        Ok(())
    }

    async fn repo_thread_id(&mut self, repo_root: &Path) -> Result<Option<String>> {
        let loaded_ids = self.loaded_thread_ids().await?;
        for thread_id in loaded_ids {
            let thread = self.read_thread(&thread_id).await?;
            if thread.cwd == repo_root.display().to_string() {
                return Ok(Some(thread.id));
            }
        }
        Ok(None)
    }

    async fn loaded_thread_ids(&mut self) -> Result<Vec<String>> {
        let id = self.next_request_id();
        self.send_json(&CodexThreadLoadedListRequest {
            id,
            method: REQUEST_THREAD_LOADED_LIST,
            params: CodexThreadLoadedListParams { limit: 10 },
        })
        .await?;
        let response: CodexResponse<CodexThreadLoadedListResponse> = self.response(id).await?;
        Ok(response.ok_result()?.data)
    }

    async fn read_thread(&mut self, thread_id: &str) -> Result<CodexThread> {
        let id = self.next_request_id();
        self.send_json(&CodexThreadReadRequest {
            id,
            method: REQUEST_THREAD_READ,
            params: CodexThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
        .await?;
        let response: CodexResponse<CodexThreadReadResponse> = self.response(id).await?;
        Ok(response.ok_result()?.thread)
    }

    async fn start_turn(&mut self, thread_id: &str, prompt: &str, repo_root: &Path) -> Result<()> {
        let id = self.next_request_id();
        self.send_json(&CodexTurnStartRequest {
            id,
            method: REQUEST_TURN_START,
            params: CodexTurnStartParams {
                thread_id: thread_id.to_string(),
                input: vec![CodexUserInput::text(prompt.to_string())],
                cwd: repo_root.display().to_string(),
            },
        })
        .await?;
        let _: CodexResponse<CodexTurnStartResponse> = self.response(id).await?;
        Ok(())
    }

    async fn send_json<T: Facet<'static>>(&mut self, value: &T) -> Result<()> {
        let text = facet_json::to_string(value).context("failed to encode Codex app-server RPC")?;
        self.ws
            .send(Message::Text(text.into()))
            .await
            .context("failed to send Codex app-server RPC")
    }

    async fn response<T: Facet<'static>>(&mut self, id: u32) -> Result<CodexResponse<T>> {
        timeout(AGENT_INVOKE_TIMEOUT, async {
            while let Some(message) = self.ws.next().await {
                let message = message.context("failed to read Codex app-server message")?;
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                        .context("Codex app-server sent non-UTF8 binary JSON")?,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(frame) => {
                        return Err(anyhow!("Codex app-server closed connection: {frame:?}"));
                    }
                    Message::Frame(_) => continue,
                };
                let envelope = match facet_json::from_str::<CodexEnvelope>(&text) {
                    Ok(envelope) => envelope,
                    Err(_) => continue,
                };
                if envelope.id != Some(id) {
                    continue;
                }
                return decode_response(id, &text);
            }
            Err(anyhow!(
                "Codex app-server connection closed before response"
            ))
        })
        .await
        .context("timed out waiting for Codex app-server response")?
    }

    fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn decode_response<T: Facet<'static>>(id: u32, text: &str) -> Result<CodexResponse<T>> {
    facet_json::from_str::<CodexResponse<T>>(text).with_context(|| {
        format!(
            "failed to decode Codex app-server response for request {id}: {}",
            response_snippet(text)
        )
    })
}

fn response_snippet(text: &str) -> String {
    if text.len() <= RESPONSE_SNIPPET_LIMIT {
        return text.to_string();
    }
    let snippet: String = text.chars().take(RESPONSE_SNIPPET_LIMIT).collect();
    format!("{snippet}...")
}

#[derive(Debug, Facet)]
struct CodexEnvelope {
    #[facet(default)]
    id: Option<u32>,
}

#[derive(Debug, Facet)]
struct CodexResponse<T> {
    #[facet(default)]
    result: Option<T>,
    #[facet(default)]
    error: Option<CodexError>,
}

impl<T> CodexResponse<T> {
    fn ok_result(self) -> Result<T> {
        if let Some(error) = self.error {
            return Err(anyhow!(
                "Codex app-server error: {}",
                error.display_message()
            ));
        }
        self.result
            .ok_or_else(|| anyhow!("Codex app-server response did not include a result"))
    }
}

#[derive(Debug, Facet)]
struct CodexError {
    #[facet(rename = "type")]
    #[facet(default)]
    kind: Option<String>,
    #[facet(default)]
    message: Option<String>,
}

impl CodexError {
    fn display_message(self) -> String {
        self.message
            .or(self.kind)
            .unwrap_or_else(|| "unknown Codex app-server error".to_string())
    }
}

#[derive(Debug, Facet)]
struct CodexInitializedNotification {
    method: &'static str,
}

#[derive(Debug, Facet)]
struct CodexInitializeRequest {
    id: u32,
    method: &'static str,
    params: CodexInitializeParams,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexInitializeParams {
    client_info: CodexClientInfo,
    capabilities: CodexInitializeCapabilities,
}

#[derive(Debug, Facet)]
struct CodexClientInfo {
    name: &'static str,
    title: &'static str,
    version: &'static str,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexInitializeCapabilities {
    experimental_api: bool,
    request_attestation: bool,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexInitializeResponse {
    user_agent: String,
}

#[derive(Debug, Facet)]
struct CodexThreadLoadedListRequest {
    id: u32,
    method: &'static str,
    params: CodexThreadLoadedListParams,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexThreadLoadedListParams {
    limit: u32,
}

#[derive(Debug, Facet)]
struct CodexThreadLoadedListResponse {
    data: Vec<String>,
}

#[derive(Debug, Facet)]
struct CodexThreadReadRequest {
    id: u32,
    method: &'static str,
    params: CodexThreadReadParams,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexThreadReadParams {
    thread_id: String,
    include_turns: bool,
}

#[derive(Debug, Facet)]
struct CodexThreadReadResponse {
    thread: CodexThread,
}

#[derive(Debug, Facet)]
struct CodexThread {
    id: String,
    cwd: String,
}

#[derive(Debug, Facet)]
struct CodexTurnStartRequest {
    id: u32,
    method: &'static str,
    params: CodexTurnStartParams,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexTurnStartParams {
    thread_id: String,
    input: Vec<CodexUserInput>,
    cwd: String,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "type")]
#[allow(dead_code)]
enum CodexUserInput {
    #[facet(rename = "text")]
    Text {
        text: String,
        text_elements: Vec<CodexTextElement>,
    },
}

impl CodexUserInput {
    fn text(text: String) -> Self {
        Self::Text {
            text,
            text_elements: Vec::new(),
        }
    }
}

#[derive(Debug, Facet)]
struct CodexTextElement {
    #[facet(rename = "byteRange")]
    byte_range: CodexByteRange,
    placeholder: Option<String>,
}

#[derive(Debug, Facet)]
struct CodexByteRange {
    start: u32,
    end: u32,
}

#[derive(Debug, Facet)]
struct CodexTurnStartResponse {
    turn: CodexTurn,
}

#[derive(Debug, Facet)]
struct CodexTurn {
    id: String,
}
