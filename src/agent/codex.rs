use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use futures::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const AGENT_INVOKE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_INITIALIZE: &str = "initialize";
const REQUEST_THREAD_LIST: &str = "thread/list";
const REQUEST_TURN_START: &str = "turn/start";
const NOTIFICATION_INITIALIZED: &str = "initialized";
const CLIENT_NAME: &str = "peers";
const CLIENT_TITLE: &str = "Peers";
const SORT_UPDATED_AT: &str = "updated_at";
const SORT_DESC: &str = "desc";

pub async fn invoke(address: &str, repo_root: &Path, prompt: &str) -> Result<String> {
    let mut client = CodexAppClient::connect(address).await?;
    client.initialize().await?;
    let thread_id = client
        .repo_thread_id(repo_root)
        .await?
        .ok_or_else(|| anyhow!("no loaded Codex thread found for `{}`", repo_root.display()))?;
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
        let id = self.next_request_id();
        self.send_json(&CodexThreadListRequest {
            id,
            method: REQUEST_THREAD_LIST,
            params: CodexThreadListParams {
                limit: 10,
                sort_key: SORT_UPDATED_AT,
                sort_direction: SORT_DESC,
                cwd: repo_root.display().to_string(),
                archived: false,
                use_state_db_only: false,
            },
        })
        .await?;
        let response: CodexResponse<CodexThreadListResponse> = self.response(id).await?;
        let result = response.ok_result()?;
        Ok(result
            .data
            .into_iter()
            .find(|thread| thread.cwd == repo_root.display().to_string())
            .map(|thread| thread.id))
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
                return facet_json::from_str::<CodexResponse<T>>(&text)
                    .context("failed to decode Codex app-server response");
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
                error.message.unwrap_or(error.kind)
            ));
        }
        self.result
            .ok_or_else(|| anyhow!("Codex app-server response did not include a result"))
    }
}

#[derive(Debug, Facet)]
struct CodexError {
    #[facet(rename = "type")]
    kind: String,
    #[facet(default)]
    message: Option<String>,
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
struct CodexThreadListRequest {
    id: u32,
    method: &'static str,
    params: CodexThreadListParams,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
struct CodexThreadListParams {
    limit: u32,
    sort_key: &'static str,
    sort_direction: &'static str,
    cwd: String,
    archived: bool,
    use_state_db_only: bool,
}

#[derive(Debug, Facet)]
struct CodexThreadListResponse {
    data: Vec<CodexThread>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_initialize_response_with_unused_fields() {
        let response = r#"{
            "result": {
                "userAgent": "codex/0.137.0",
                "codexHome": "/home/jonas/.codex",
                "platformFamily": "unix",
                "platformOs": "linux"
            }
        }"#;

        let response: CodexResponse<CodexInitializeResponse> =
            facet_json::from_str(response).unwrap();

        assert_eq!(response.ok_result().unwrap().user_agent, "codex/0.137.0");
    }

    #[test]
    fn encodes_turn_start_request_with_codex_field_names() {
        let request = CodexTurnStartRequest {
            id: 3,
            method: REQUEST_TURN_START,
            params: CodexTurnStartParams {
                thread_id: "thread-1".to_string(),
                input: vec![CodexUserInput::text("hello".to_string())],
                cwd: "/repo".to_string(),
            },
        };

        let encoded = facet_json::to_string(&request).unwrap();

        assert!(encoded.contains(r#""method":"turn/start""#), "{encoded}");
        assert!(encoded.contains(r#""threadId":"thread-1""#), "{encoded}");
        assert!(encoded.contains(r#""text_elements":[]"#), "{encoded}");
    }
}
