use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tower_lsp_server::jsonrpc::Result as LspResult;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::review_provider::ReviewProvider;

const LOOPBACK_BIND_HOST: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const PEERSDIFF_SERVER_NAME: &str = "peersdiff";
const LSP_BIND_ERROR: &str = "failed to bind local Peers Neovim LSP server";
const LSP_ATTACHED_MESSAGE: &str = "Peers review LSP attached";
const LSP_CONNECTION_ERROR_MESSAGE: &str = "Peers Neovim LSP connection failed";
const REVIEW_SYMBOL_DETAIL: &str = "Peers review";
const SYNTHETIC_BUFFER_SYMBOL_DETAIL: &str = "Synthetic review buffer";
const COMMAND_NOT_WIRED_SUFFIX: &str = "is not wired yet";
const REVIEW_HOVER_TITLE: &str = "Peers review";
const TARGET_LABEL: &str = "Target";
const AUTHOR_LABEL: &str = "Author";
const FILES_LABEL: &str = "Files";
const THREADS_LABEL: &str = "Threads";
const TOTAL_THREADS_LABEL: &str = "total";
const UNRESOLVED_THREADS_LABEL: &str = "unresolved";
const CURSOR_LABEL: &str = "Cursor";
const LINE_LABEL: &str = "line";
const COLUMN_LABEL: &str = "column";
const LOAD_REVIEW_STATE_ERROR: &str = "Failed to load review state";

const COMMAND_ADD_COMMENT: &str = "peers.addComment";
const COMMAND_REPLY: &str = "peers.reply";
const COMMAND_RESOLVE_THREAD: &str = "peers.resolveThread";
const COMMAND_MARK_VIEWED: &str = "peers.markViewed";
const COMMAND_SUBMIT_REVIEW: &str = "peers.submitReview";
const COMMAND_ASK_AGENT: &str = "peers.askAgent";

const ACTION_ADD_COMMENT: &str = "Peers: Add comment";
const ACTION_REPLY: &str = "Peers: Reply";
const ACTION_RESOLVE_THREAD: &str = "Peers: Resolve thread";
const ACTION_MARK_VIEWED: &str = "Peers: Mark file viewed";
const ACTION_SUBMIT_REVIEW: &str = "Peers: Submit review";
const ACTION_ASK_AGENT: &str = "Peers: Ask agent";

pub struct NvimLspServer {
    listener: TcpListener,
    addr: SocketAddr,
    provider: ReviewProvider,
}

impl NvimLspServer {
    pub async fn bind(provider: ReviewProvider) -> Result<Self> {
        let listener = TcpListener::bind((LOOPBACK_BIND_HOST, 0))
            .await
            .context(LSP_BIND_ERROR)?;
        let addr = listener.local_addr()?;

        Ok(Self {
            listener,
            addr,
            provider,
        })
    }

    pub fn url(&self) -> String {
        format!("tcp://{LOCALHOST}:{}", self.addr.port())
    }

    pub async fn run(self) -> Result<()> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let provider = self.provider.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_lsp_connection(stream, provider).await {
                    eprintln!("{LSP_CONNECTION_ERROR_MESSAGE}: {error:#}");
                }
            });
        }
    }
}

#[derive(Debug)]
struct PeersDiffLanguageServer {
    client: Client,
    provider: ReviewProvider,
}

impl PeersDiffLanguageServer {
    fn new(client: Client, provider: ReviewProvider) -> Self {
        Self { client, provider }
    }

    async fn review_summary(&self) -> String {
        match self.provider.get_review().await {
            Ok(review) => {
                let unresolved_count = review
                    .threads
                    .iter()
                    .filter(|thread| !thread.resolved)
                    .count();
                format!(
                    "{REVIEW_HOVER_TITLE} `{}`\n\n{TARGET_LABEL}: `{}`\n{AUTHOR_LABEL}: `{}`\n{FILES_LABEL}: `{}`\n{THREADS_LABEL}: `{}` {TOTAL_THREADS_LABEL}, `{}` {UNRESOLVED_THREADS_LABEL}",
                    review.review_id,
                    review.target_label,
                    self.provider.author().display_name,
                    review.files.len(),
                    review.threads.len(),
                    unresolved_count
                )
            }
            Err(error) => format!(
                "{REVIEW_HOVER_TITLE} `{}`\n\n{LOAD_REVIEW_STATE_ERROR}: {error:#}",
                self.provider.review_id()
            ),
        }
    }
}

impl LanguageServer for PeersDiffLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: PEERSDIFF_SERVER_NAME.to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        COMMAND_ADD_COMMENT.to_string(),
                        COMMAND_REPLY.to_string(),
                        COMMAND_RESOLVE_THREAD.to_string(),
                        COMMAND_MARK_VIEWED.to_string(),
                        COMMAND_SUBMIT_REVIEW.to_string(),
                        COMMAND_ASK_AGENT.to_string(),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, LSP_ATTACHED_MESSAGE)
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let summary = self.review_summary().await;
        let line = params.text_document_position_params.position.line + 1;
        let character = params.text_document_position_params.position.character + 1;
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "{summary}\n\n{CURSOR_LABEL}: {LINE_LABEL} `{line}`, {COLUMN_LABEL} `{character}`"
                ),
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        _: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        Ok(None)
    }

    async fn references(&self, _: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        Ok(None)
    }

    async fn code_action(&self, _: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        Ok(Some(vec![
            CodeActionOrCommand::Command(Command::new(
                ACTION_ADD_COMMENT.to_string(),
                COMMAND_ADD_COMMENT.to_string(),
                None,
            )),
            CodeActionOrCommand::Command(Command::new(
                ACTION_REPLY.to_string(),
                COMMAND_REPLY.to_string(),
                None,
            )),
            CodeActionOrCommand::Command(Command::new(
                ACTION_RESOLVE_THREAD.to_string(),
                COMMAND_RESOLVE_THREAD.to_string(),
                None,
            )),
            CodeActionOrCommand::Command(Command::new(
                ACTION_MARK_VIEWED.to_string(),
                COMMAND_MARK_VIEWED.to_string(),
                None,
            )),
            CodeActionOrCommand::Command(Command::new(
                ACTION_SUBMIT_REVIEW.to_string(),
                COMMAND_SUBMIT_REVIEW.to_string(),
                None,
            )),
            CodeActionOrCommand::Command(Command::new(
                ACTION_ASK_AGENT.to_string(),
                COMMAND_ASK_AGENT.to_string(),
                None,
            )),
        ]))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<LSPAny>> {
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Peers command `{}` {COMMAND_NOT_WIRED_SUFFIX}",
                    params.command
                ),
            )
            .await;
        Ok(None)
    }

    #[allow(deprecated)]
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        };

        Ok(Some(DocumentSymbolResponse::Nested(vec![DocumentSymbol {
            name: self.provider.review_id().to_string(),
            detail: Some(REVIEW_SYMBOL_DETAIL.to_string()),
            kind: SymbolKind::FILE,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: Some(vec![DocumentSymbol {
                name: uri.to_string(),
                detail: Some(SYNTHETIC_BUFFER_SYMBOL_DETAIL.to_string()),
                kind: SymbolKind::MODULE,
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            }]),
        }])))
    }
}

async fn serve_lsp_connection(stream: TcpStream, provider: ReviewProvider) -> Result<()> {
    let (read, write) = tokio::io::split(stream);
    let (service, socket) =
        LspService::new(|client| PeersDiffLanguageServer::new(client, provider));
    Server::new(read, write, socket).serve(service).await;
    Ok(())
}
