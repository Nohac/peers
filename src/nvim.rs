use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use dprint_plugin_markdown::configuration::{
    ConfigurationBuilder, EmphasisKind, StrongKind, TextWrap,
};
use dprint_plugin_markdown::format_text as format_markdown_text;
use facet::Facet;
use std::collections::{BTreeMap, BTreeSet};
use tokio::net::{TcpListener, TcpStream};
use tower_lsp_server::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};
use tracing::instrument;

use crate::agent::AgentInvocationRequest;
use crate::anchors::{AnchorLinePlacement, AnchorPlacement};
use crate::comments::{Author, AuthorKind, CommentThread};
use crate::diff::{DiffSection, FileSide, FileStatus, LineRange};
use crate::review_provider::ReviewProvider;
use crate::review_provider::{
    CommentRequest, CreateThreadRequest, EditCommentRequest, ReviewComment, ReviewProjection,
    ReviewThread, ReviewThreadAnchor, ThreadBodyRequest, ThreadRequest,
    thread_visible_in_default_projection,
};

const LOOPBACK_BIND_HOST: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const PEERSDIFF_SERVER_NAME: &str = "peersdiff";
const LSP_BIND_ERROR: &str = "failed to bind local Peers Neovim LSP server";
const LSP_ATTACHED_MESSAGE: &str = "Peers review LSP attached";
const LSP_CONNECTION_ERROR_MESSAGE: &str = "Peers Neovim LSP connection failed";
const SYNTHETIC_BUFFER_SYMBOL_DETAIL: &str = "Synthetic review buffer";
const COMMAND_NOT_WIRED_SUFFIX: &str = "is not wired yet";
const REVIEW_HOVER_TITLE: &str = "Peers review";
const TARGET_LABEL: &str = "Target";
const AUTHOR_LABEL: &str = "Author";
const FILES_LABEL: &str = "Files";
const THREADS_LABEL: &str = "Threads";
const TOTAL_THREADS_LABEL: &str = "total";
const UNRESOLVED_THREADS_LABEL: &str = "unresolved";
const THREAD_SCOPE_LINE: &str = "line";
const THREAD_SCOPE_FILE: &str = "file";
const COMMENT_AGENT_MARKER: &str = " [agent]";
const COMMENT_EDITED_LABEL: &str = "edited";
const COMMENT_META_SEPARATOR: &str = " · ";
const CURSOR_LABEL: &str = "Cursor";
const LINE_LABEL: &str = "line";
const COLUMN_LABEL: &str = "column";
const LOAD_REVIEW_STATE_ERROR: &str = "Failed to load review state";
const SYMBOL_FILE_DETAIL: &str = "changed file";
const SYMBOL_HUNK_DETAIL: &str = "changed lines";
const SYMBOL_REVIEW_DETAIL: &str = "Peers review";
const LINES_LABEL: &str = "lines";
const OLD_LINES_LABEL: &str = "old lines";

const COMMAND_ADD_COMMENT: &str = "peers.addComment";
const COMMAND_REPLY: &str = "peers.reply";
const COMMAND_EDIT_COMMENT: &str = "peers.editComment";
const COMMAND_DELETE_COMMENT: &str = "peers.deleteComment";
const COMMAND_DELETE_THREAD: &str = "peers.deleteThread";
const COMMAND_RESOLVE_THREAD: &str = "peers.resolveThread";
const COMMAND_REOPEN_THREAD: &str = "peers.reopenThread";
const COMMAND_TOGGLE_THREAD_COLLAPSED: &str = "peers.toggleThreadCollapsed";
const COMMAND_RESPOND_TO_THREAD: &str = "peers.respondToThread";

const ACTION_ADD_LINE_COMMENT: &str = "Peers: Add line comment";
const ACTION_ADD_RANGE_COMMENT_PREFIX: &str = "Peers: Add comment on lines";
const ACTION_ADD_FILE_COMMENT: &str = "Peers: Add comment on file";
const ACTION_REPLY: &str = "Peers: Reply";
const ACTION_EDIT_COMMENT: &str = "Peers: Edit comment";
const ACTION_DELETE_COMMENT: &str = "Peers: Delete comment";
const ACTION_DELETE_THREAD: &str = "Peers: Delete thread";
const ACTION_RESOLVE_THREAD: &str = "Peers: Resolve thread";
const ACTION_REOPEN_THREAD: &str = "Peers: Reopen thread";
const ACTION_TOGGLE_THREAD_COLLAPSED: &str = "Peers: Toggle collapse";
const ACTION_RESPOND_TO_THREAD: &str = "Peers: Respond to thread";
const NOTIFICATION_REVIEW_UPDATED: &str = "peers/reviewUpdated";
const METHOD_RENDER_REVIEW: &str = "peers/renderReview";
const METHOD_CREATE_THREAD: &str = "peers/createThread";
const METHOD_REPLY_TO_THREAD: &str = "peers/replyToThread";
const METHOD_EDIT_COMMENT: &str = "peers/editComment";
const METHOD_DELETE_COMMENT: &str = "peers/deleteComment";
const METHOD_DELETE_THREAD: &str = "peers/deleteThread";
const METHOD_RESOLVE_THREAD: &str = "peers/resolveThread";
const METHOD_REOPEN_THREAD: &str = "peers/reopenThread";
const METHOD_TOGGLE_THREAD_COLLAPSED: &str = "peers/toggleThreadCollapsed";
const METHOD_ASK_AGENT: &str = "peers/askAgent";
const PARAM_SCOPE: &str = "scope";
const PARAM_PATH: &str = "path";
const PARAM_SIDE: &str = "side";
const PARAM_START_LINE: &str = "start_line";
const PARAM_END_LINE: &str = "end_line";
const PARAM_BODY: &str = "body";
const PARAM_THREAD_ID: &str = "thread_id";
const PARAM_COMMENT_ID: &str = "comment_id";
const PARAM_CONTEXT: &str = "context";
const PARAM_LINE_LABEL: &str = "line_label";
const PARAM_ANCHOR_PLACEMENT: &str = "anchor_placement";
const PARAM_PROMPT: &str = "prompt";
const LSP_INVALID_PARAMS: &str = "invalid Peers request params";
const LSP_MISSING_FIELD: &str = "missing field";
const LSP_INVALID_FIELD: &str = "invalid field";
const LSP_PAYLOAD_ENCODE_ERROR: &str = "failed to encode Peers LSP payload";
const LSP_PAYLOAD_DECODE_ERROR: &str = "failed to decode Peers LSP payload";

const ROW_KIND_FILE_HEADER: &str = "file_header";
const ROW_KIND_HUNK_HEADER: &str = "hunk_header";
const ROW_KIND_CONTEXT: &str = "context";
const ROW_KIND_ADD: &str = "add";
const ROW_KIND_DELETE: &str = "delete";
const ROW_KIND_COMMENT: &str = "comment";
const ROW_KIND_EMPTY: &str = "empty";
const SIDE_NEW: &str = "new";
const SIDE_OLD: &str = "old";
const HIGHLIGHT_FILE_HEADER: &str = "PeersDiffFileHeader";
const HIGHLIGHT_HUNK_HEADER: &str = "PeersDiffHunkHeader";
const HIGHLIGHT_ADD_GUTTER: &str = "PeersDiffAddGutter";
const HIGHLIGHT_DELETE_GUTTER: &str = "PeersDiffDeleteGutter";
const HIGHLIGHT_ADD_GUTTER_BACKGROUND: &str = "PeersDiffAddGutterBackground";
const HIGHLIGHT_DELETE_GUTTER_BACKGROUND: &str = "PeersDiffDeleteGutterBackground";
const HIGHLIGHT_ADD_LINE_BACKGROUND: &str = "PeersDiffAddLineBackground";
const HIGHLIGHT_DELETE_LINE_BACKGROUND: &str = "PeersDiffDeleteLineBackground";
const HIGHLIGHT_LINE_NUMBER: &str = "PeersDiffLineNumber";
const HIGHLIGHT_THREAD_ATTACHMENT: &str = "PeersDiffThreadAttachment";
const HIGHLIGHT_THREAD_BODY: &str = "PeersDiffThreadBody";
const HIGHLIGHT_THREAD_BORDER: &str = "PeersDiffThreadBorder";
const HIGHLIGHT_THREAD_BORDER_CONTEXT: &str = "PeersDiffThreadBorderContext";
const HIGHLIGHT_THREAD_BORDER_STALE: &str = "PeersDiffThreadBorderStale";
const HIGHLIGHT_THREAD_BORDER_DETACHED: &str = "PeersDiffThreadBorderDetached";
const HIGHLIGHT_THREAD_RESOLVED: &str = "PeersDiffThreadResolved";
const HIGHLIGHT_THREAD_HEADER: &str = "PeersDiffThreadHeader";
const HIGHLIGHT_THREAD_META: &str = "PeersDiffThreadMeta";
const HIGHLIGHT_THREAD_RAIL: &str = "PeersDiffThreadRail";
const HIGHLIGHT_THREAD_RAIL_CONTEXT: &str = "PeersDiffThreadRailContext";
const HIGHLIGHT_THREAD_RAIL_STALE: &str = "PeersDiffThreadRailStale";
const HIGHLIGHT_THREAD_RAIL_DETACHED: &str = "PeersDiffThreadRailDetached";
const HIGHLIGHT_THREAD_LOCATION_NOTE: &str = "PeersDiffThreadLocationNote";
const HIGHLIGHT_EMPTY_TITLE: &str = "PeersDiffEmptyTitle";
const HIGHLIGHT_EMPTY_TEXT: &str = "PeersDiffEmptyText";
const HUNK_HEADER_PREFIX: &str = "@@";
const FILE_HEADER_PREFIX: &str = "diff -- ";
const LINE_NUMBER_WIDTH: usize = 5;
const LINE_PREFIX_WIDTH: u32 = 14;
const EMPTY_CARD_MARGIN: &str = "  ";
const EMPTY_CARD_WIDTH: usize = 62;
const EMPTY_TITLE: &str = "No file changes";
const EMPTY_BODY: &str = "This review has no diffs to show.";
const EMPTY_REFRESH: &str = "Run :PeersReview if you expected local edits to appear.";
const EMPTY_SYMBOL_NAME: &str = "No file changes";
const EMPTY_SYMBOL_DETAIL: &str = "empty review";
const THREAD_RAIL_START_COL: u32 = 0;
const THREAD_RAIL_END_COL: u32 = 1;
const THREAD_CARD_WIDTH: usize = 86;
const THREAD_CARD_MARGIN: &str = "  ";
const THREAD_HEADER_PREFIX: &str = "╭─ ";
const THREAD_BODY_PREFIX: &str = "│ ";
const THREAD_REPLY_META_PREFIX: &str = "├╴";
const THREAD_FOOTER: &str = "╰─";
const THREAD_COMMENT_ELISION_PREFIX: &str = "│ ";
const THREAD_STATUS_OPEN_ICON: &str = "●";
const THREAD_STATUS_RESOLVED_ICON: &str = "✓";
const THREAD_EMPTY_PREVIEW: &str = "No comment body";
const THREAD_COMMENT_ELISION_SUFFIX: &str = "comments hidden";
const TRUNCATION_SUFFIX: &str = "…";
const SIDEBAR_WIDTH: usize = 36;
const SIDEBAR_FOLDER_ICON: &str = "󰉋";
const SIDEBAR_FILE_PREFIX: &str = "  ";
const SIDEBAR_THREAD_MAX_VISIBLE_COMMENTS: usize = 3;
const HIGHLIGHT_SIDEBAR_STATUS_ADDED: &str = "PeersSidebarStatusAdded";
const HIGHLIGHT_SIDEBAR_STATUS_DELETED: &str = "PeersSidebarStatusDeleted";
const HIGHLIGHT_SIDEBAR_STATUS_MODIFIED: &str = "PeersSidebarStatusModified";
const HIGHLIGHT_SIDEBAR_STATUS_RENAMED: &str = "PeersSidebarStatusRenamed";
const HIGHLIGHT_SIDEBAR_STATUS_UNCHANGED: &str = "PeersSidebarStatusUnchanged";
const HIGHLIGHT_SIDEBAR_STATUS_BINARY: &str = "PeersSidebarStatusBinary";
const HIGHLIGHT_SIDEBAR_DELTA_ADDED: &str = "PeersSidebarDeltaAdded";
const HIGHLIGHT_SIDEBAR_DELTA_REMOVED: &str = "PeersSidebarDeltaRemoved";
const HIGHLIGHT_SIDEBAR_DELTA_POSITIVE: &str = "PeersSidebarDeltaPositive";
const HIGHLIGHT_SIDEBAR_DELTA_NEGATIVE: &str = "PeersSidebarDeltaNegative";
const HIGHLIGHT_SIDEBAR_DELTA_NEUTRAL: &str = "PeersSidebarDeltaNeutral";
const HIGHLIGHT_SIDEBAR_THREAD_META: &str = "PeersSidebarThreadMeta";
const HIGHLIGHT_SIDEBAR_COMMENT_ELISION: &str = "PeersSidebarCommentElision";
const HIGHLIGHT_SIDEBAR_THREAD_LOCATION_NOTE: &str = "PeersSidebarThreadLocationNote";
const HIGHLIGHT_SIDEBAR_THREAD_RESOLVED: &str = "PeersSidebarThreadResolved";

struct ReviewUpdatedNotification;

impl notification::Notification for ReviewUpdatedNotification {
    type Params = LSPAny;

    const METHOD: &'static str = NOTIFICATION_REVIEW_UPDATED;
}

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
    render_cache: Mutex<Option<RenderedReviewCache>>,
}

impl PeersDiffLanguageServer {
    fn new(client: Client, provider: ReviewProvider) -> Self {
        Self {
            client,
            provider,
            render_cache: Mutex::new(None),
        }
    }

    fn render_review_response(&self, review: ReviewProjection) -> LSPAny {
        let rendered = render_payload(review);
        self.cache_rendered_review(&rendered);
        rendered.into_lsp()
    }

    fn cache_rendered_review(&self, rendered: &RenderedReview) {
        let mut cache = self
            .render_cache
            .lock()
            .expect("render cache mutex poisoned");
        *cache = Some(RenderedReviewCache {
            rows: rendered.rows.clone(),
            sidebar: rendered.sidebar.clone(),
            sidebar_counts: rendered.sidebar_counts.clone(),
        });
    }

    fn update_cached_thread_sidebar(
        &self,
        thread_id: &str,
        replacement_rows: &[RenderedRow],
    ) -> Option<(RenderedSidebar, RenderedSidebarCounts)> {
        let mut cache = self
            .render_cache
            .lock()
            .expect("render cache mutex poisoned");
        let cache = cache.as_mut()?;
        let (first, last) = rendered_thread_block_range(&cache.rows, thread_id)?;
        cache
            .rows
            .splice(first..=last, replacement_rows.iter().cloned());
        cache.sidebar = render_sidebar(&cache.rows);
        Some((cache.sidebar.clone(), cache.sidebar_counts.clone()))
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
                    "repo",
                    review.target_label,
                    self.provider.author().display_name,
                    review.files.len(),
                    review.threads.len(),
                    unresolved_count
                )
            }
            Err(error) => {
                format!("{REVIEW_HOVER_TITLE} `repo`\n\n{LOAD_REVIEW_STATE_ERROR}: {error:#}")
            }
        }
    }

    #[instrument(name = "lsp.render_review", skip_all)]
    async fn render_review(&self) -> LspResult<LSPAny> {
        let review = self
            .provider
            .get_review()
            .await
            .map_err(|_| LspError::internal_error())?;
        let rendered = render_payload(review);
        self.cache_rendered_review(&rendered);
        Ok(rendered_review_into_lsp(rendered))
    }

    async fn create_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = create_thread_request(&params)?;
        let review = self
            .provider
            .create_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn reply_to_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_body_request(&params)?;
        let review = self
            .provider
            .reply_to_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn edit_comment(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = edit_comment_request(&params)?;
        let review = self
            .provider
            .edit_comment(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn delete_comment(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = comment_request(&params)?;
        let review = self
            .provider
            .delete_comment(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn delete_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let review = self
            .provider
            .delete_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn resolve_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let review = self
            .provider
            .resolve_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn reopen_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let review = self
            .provider
            .reopen_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(self.render_review_response(review))
    }

    async fn toggle_thread_collapsed(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let context = thread_render_context(&params)?;
        let thread = self
            .provider
            .toggle_thread_collapsed_state(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        let patch = render_thread_patch(thread, self.provider.author(), context);
        let (sidebar, sidebar_counts) = self
            .update_cached_thread_sidebar(&patch.thread_id, &patch.rows)
            .map_or((None, None), |(sidebar, sidebar_counts)| {
                (Some(sidebar), Some(sidebar_counts))
            });
        Ok(patch.into_lsp(sidebar, sidebar_counts))
    }

    async fn ask_agent(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = ask_agent_request(&params)?;
        let response = crate::agent::invoke_agent(self.provider.repo_root(), request)
            .await
            .map_err(|error| LspError::invalid_params(error.to_string()))?;
        Ok(lsp_payload(response))
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
                        COMMAND_EDIT_COMMENT.to_string(),
                        COMMAND_DELETE_COMMENT.to_string(),
                        COMMAND_DELETE_THREAD.to_string(),
                        COMMAND_RESOLVE_THREAD.to_string(),
                        COMMAND_REOPEN_THREAD.to_string(),
                        COMMAND_TOGGLE_THREAD_COLLAPSED.to_string(),
                        COMMAND_RESPOND_TO_THREAD.to_string(),
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
        let client = self.client.clone();
        let mut receiver = self.provider.updates().subscribe();
        tokio::spawn(async move {
            while let Ok(update) = receiver.recv().await {
                client
                    .send_notification::<ReviewUpdatedNotification>(review_update_param(
                        update.kind,
                        update.sequence,
                    ))
                    .await;
            }
        });
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        if !is_review_uri(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
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

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        if let Some(actions) = source_code_actions_for_range(
            self.provider.repo_root(),
            &params.text_document.uri,
            params.range,
        ) {
            return Ok(Some(actions));
        }
        if !is_review_uri(&params.text_document.uri) {
            return Ok(Some(Vec::new()));
        }

        let review = self
            .provider
            .get_review()
            .await
            .map_err(|_| LspError::internal_error())?;
        let rendered = render_review_payload(review);
        Ok(Some(code_actions_for_range(&rendered, params.range)))
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
        if !is_review_uri(&uri) {
            return Ok(None);
        }
        let review = self
            .provider
            .get_review()
            .await
            .map_err(|_| LspError::internal_error())?;
        let root_name = review.target_label.clone();
        let rendered = render_review_payload(review);
        let document_range = document_range(&rendered);

        Ok(Some(DocumentSymbolResponse::Nested(vec![DocumentSymbol {
            name: root_name,
            detail: Some(SYMBOL_REVIEW_DETAIL.to_string()),
            kind: SymbolKind::FILE,
            tags: None,
            deprecated: None,
            range: document_range,
            selection_range: document_range,
            children: Some(document_symbols(&rendered, uri.to_string())),
        }])))
    }
}

async fn serve_lsp_connection(stream: TcpStream, provider: ReviewProvider) -> Result<()> {
    let (read, write) = tokio::io::split(stream);
    let (service, socket) =
        LspService::build(|client| PeersDiffLanguageServer::new(client, provider.clone()))
            .custom_method(METHOD_RENDER_REVIEW, PeersDiffLanguageServer::render_review)
            .custom_method(METHOD_CREATE_THREAD, PeersDiffLanguageServer::create_thread)
            .custom_method(
                METHOD_REPLY_TO_THREAD,
                PeersDiffLanguageServer::reply_to_thread,
            )
            .custom_method(METHOD_EDIT_COMMENT, PeersDiffLanguageServer::edit_comment)
            .custom_method(
                METHOD_DELETE_COMMENT,
                PeersDiffLanguageServer::delete_comment,
            )
            .custom_method(METHOD_DELETE_THREAD, PeersDiffLanguageServer::delete_thread)
            .custom_method(
                METHOD_RESOLVE_THREAD,
                PeersDiffLanguageServer::resolve_thread,
            )
            .custom_method(METHOD_REOPEN_THREAD, PeersDiffLanguageServer::reopen_thread)
            .custom_method(
                METHOD_TOGGLE_THREAD_COLLAPSED,
                PeersDiffLanguageServer::toggle_thread_collapsed,
            )
            .custom_method(METHOD_ASK_AGENT, PeersDiffLanguageServer::ask_agent)
            .finish();
    Server::new(read, write, socket).serve(service).await;
    Ok(())
}

#[derive(Debug)]
struct RenderedReview {
    lines: Vec<String>,
    rows: Vec<RenderedRow>,
    highlights: Vec<RenderedHighlight>,
    source_decorations: Vec<SourceDecoration>,
    symbols: Vec<RenderedSymbol>,
    sidebar: RenderedSidebar,
    sidebar_counts: RenderedSidebarCounts,
}

#[derive(Clone, Debug, Default)]
struct RenderedReviewCache {
    rows: Vec<RenderedRow>,
    sidebar: RenderedSidebar,
    sidebar_counts: RenderedSidebarCounts,
}

impl RenderedReview {
    fn push_line(&mut self, line: String, row: RenderedRow) -> u32 {
        let index = self.lines.len() as u32;
        self.lines.push(line);
        self.rows.push(row);
        index
    }

    fn push_highlight(&mut self, line: u32, start_col: u32, end_col: u32, group: &'static str) {
        self.highlights.push(RenderedHighlight {
            line,
            start_col,
            end_col,
            group,
        });
    }

    fn into_lsp(self) -> LSPAny {
        lsp_payload(RenderedReviewPayload::from(self))
    }
}

#[derive(Debug, Facet)]
struct RenderedReviewPayload {
    lines: Vec<String>,
    rows: Vec<RenderedRow>,
    highlights: Vec<RenderedHighlight>,
    source_decorations: Vec<SourceDecoration>,
    sidebar: RenderedSidebar,
    sidebar_counts: RenderedSidebarCounts,
}

impl From<RenderedReview> for RenderedReviewPayload {
    fn from(rendered: RenderedReview) -> Self {
        Self {
            lines: rendered.lines,
            rows: rendered.rows,
            highlights: rendered.highlights,
            source_decorations: rendered.source_decorations,
            sidebar: rendered.sidebar,
            sidebar_counts: rendered.sidebar_counts,
        }
    }
}

#[instrument(name = "render_payload", skip_all)]
fn render_payload(review: ReviewProjection) -> RenderedReview {
    render_review_payload(review)
}

#[instrument(name = "into_lsp", skip_all)]
fn rendered_review_into_lsp(rendered: RenderedReview) -> LSPAny {
    rendered.into_lsp()
}

fn lsp_payload<T>(payload: T) -> LSPAny
where
    T: Facet<'static>,
{
    let json = facet_json::to_string(&payload).expect(LSP_PAYLOAD_ENCODE_ERROR);
    let mut payload = json.parse::<LSPAny>().expect(LSP_PAYLOAD_DECODE_ERROR);
    prune_null_object_fields(&mut payload);
    payload
}

fn prune_null_object_fields(payload: &mut LSPAny) {
    match payload {
        LSPAny::Object(object) => {
            object.retain(|_, value| {
                prune_null_object_fields(value);
                !value.is_null()
            });
        }
        LSPAny::Array(items) => {
            for item in items {
                prune_null_object_fields(item);
            }
        }
        _ => {}
    }
}

#[derive(Clone, Debug)]
struct ThreadRenderContext {
    scope: String,
    path: Option<String>,
    line_label: String,
    side: Option<String>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    anchor_placement: Option<AnchorPlacement>,
}

fn render_thread_patch(
    thread: CommentThread,
    current_author: &Author,
    context: ThreadRenderContext,
) -> RenderedThreadPatch {
    let thread = review_thread_from_context(thread, current_author, context);
    let thread_id = thread.id.clone();
    let collapsed = thread.collapsed;
    let mut rendered = RenderedReview {
        lines: Vec::new(),
        rows: Vec::new(),
        highlights: Vec::new(),
        source_decorations: Vec::new(),
        symbols: Vec::new(),
        sidebar: RenderedSidebar::default(),
        sidebar_counts: RenderedSidebarCounts::default(),
    };
    push_thread_block(&mut rendered, &thread);

    RenderedThreadPatch {
        thread_id,
        collapsed,
        lines: rendered.lines,
        rows: rendered.rows,
        highlights: rendered.highlights,
    }
}

#[derive(Debug)]
struct RenderedThreadPatch {
    thread_id: String,
    collapsed: bool,
    lines: Vec<String>,
    rows: Vec<RenderedRow>,
    highlights: Vec<RenderedHighlight>,
}

impl RenderedThreadPatch {
    fn into_lsp(
        self,
        sidebar: Option<RenderedSidebar>,
        sidebar_counts: Option<RenderedSidebarCounts>,
    ) -> LSPAny {
        lsp_payload(ThreadPatchPayload {
            kind: "thread_patch",
            thread_id: self.thread_id,
            collapsed: self.collapsed,
            lines: self.lines,
            rows: self.rows,
            highlights: self.highlights,
            sidebar,
            sidebar_counts,
        })
    }
}

#[derive(Debug, Facet)]
struct ThreadPatchPayload {
    kind: &'static str,
    thread_id: String,
    collapsed: bool,
    lines: Vec<String>,
    rows: Vec<RenderedRow>,
    highlights: Vec<RenderedHighlight>,
    sidebar: Option<RenderedSidebar>,
    sidebar_counts: Option<RenderedSidebarCounts>,
}

fn review_thread_from_context(
    thread: CommentThread,
    current_author: &Author,
    context: ThreadRenderContext,
) -> ReviewThread {
    ReviewThread {
        id: thread.id.to_string(),
        scope: context.scope,
        path: context.path,
        line_label: context.line_label,
        anchor: ReviewThreadAnchor {
            side: context.side,
            start_line: context.start_line,
            end_line: context.end_line,
            placement: context.anchor_placement,
            line_placements: Vec::new(),
        },
        resolved: thread.resolved,
        resolved_head_oid: thread.resolved_head_oid,
        collapsed: thread.collapsed,
        comments: thread
            .comments
            .into_iter()
            .filter(|comment| comment.deleted_at.is_none())
            .map(|comment| ReviewComment {
                can_edit: comment.author.kind == current_author.kind
                    && comment.author.display_name == current_author.display_name,
                comment,
            })
            .collect(),
    }
}

#[derive(Clone, Debug, Default, Facet)]
struct RenderedSidebarCounts {
    files: u32,
    comments: u32,
}

#[derive(Clone, Debug, Default, Facet)]
struct RenderedSidebar {
    files: RenderedSidebarPanel,
    comments: RenderedSidebarPanel,
}

#[derive(Clone, Debug, Default, Facet)]
struct RenderedSidebarPanel {
    lines: Vec<String>,
    rows: Vec<RenderedSidebarRow>,
    highlights: Vec<RenderedHighlight>,
}

impl RenderedSidebarPanel {
    fn push_line(&mut self, line: String, row: RenderedSidebarRow) -> u32 {
        let index = self.lines.len() as u32;
        self.lines.push(line);
        self.rows.push(row);
        index
    }

    fn push_highlight(&mut self, line: u32, start_col: u32, end_col: u32, group: &'static str) {
        self.highlights.push(RenderedHighlight {
            line,
            start_col,
            end_col,
            group,
        });
    }
}

#[derive(Clone, Debug, Default, Facet)]
struct RenderedSidebarRow {
    target_line: Option<u32>,
    path: Option<String>,
    thread_id: Option<String>,
}

impl RenderedSidebarRow {
    fn target(target_line: usize) -> Self {
        Self {
            target_line: Some(target_line as u32),
            path: None,
            thread_id: None,
        }
    }

    fn thread(target_line: usize, thread_id: &str) -> Self {
        Self {
            target_line: Some(target_line as u32),
            path: None,
            thread_id: Some(thread_id.to_string()),
        }
    }

    fn path(target_line: usize, path: String) -> Self {
        Self {
            target_line: Some(target_line as u32),
            path: Some(path),
            thread_id: None,
        }
    }
}

#[derive(Clone, Debug, Facet)]
struct RenderedRow {
    kind: &'static str,
    path: Option<String>,
    file_status: Option<&'static str>,
    #[facet(skip_serializing)]
    sidebar_file_status: Option<FileStatus>,
    added_lines: Option<u32>,
    removed_lines: Option<u32>,
    side: Option<&'static str>,
    source_start_line: Option<u32>,
    source_line: Option<u32>,
    code_start_col: Option<u32>,
    thread_id: Option<String>,
    comment_id: Option<String>,
    comment_body: Option<String>,
    comment_body_line: Option<String>,
    comment_meta: Option<String>,
    can_edit: Option<bool>,
    invalidates_later_activity: Option<bool>,
    resolved: Option<bool>,
    collapsed: Option<bool>,
    thread_comment_count: Option<u32>,
    thread_summary: Option<String>,
    anchor_placement: Option<AnchorPlacement>,
    placement_state: Option<&'static str>,
}

impl RenderedRow {
    fn meta(kind: &'static str) -> Self {
        Self {
            kind,
            path: None,
            file_status: None,
            sidebar_file_status: None,
            added_lines: None,
            removed_lines: None,
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_body_line: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            collapsed: None,
            thread_comment_count: None,
            thread_summary: None,
            anchor_placement: None,
            placement_state: None,
        }
    }

    fn file_meta(kind: &'static str, path: &str) -> Self {
        Self {
            kind,
            path: Some(path.to_string()),
            file_status: None,
            sidebar_file_status: None,
            added_lines: None,
            removed_lines: None,
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_body_line: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            collapsed: None,
            thread_comment_count: None,
            thread_summary: None,
            anchor_placement: None,
            placement_state: Some("file"),
        }
    }

    fn file_header(file: &crate::diff::ReviewFile) -> Self {
        Self {
            kind: ROW_KIND_FILE_HEADER,
            path: Some(file.path.clone()),
            file_status: Some(file_status_name(file.status)),
            sidebar_file_status: Some(file.status),
            added_lines: Some(file.added_lines),
            removed_lines: Some(file.removed_lines),
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_body_line: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            collapsed: None,
            thread_comment_count: None,
            thread_summary: None,
            anchor_placement: None,
            placement_state: Some("file"),
        }
    }

    fn source(kind: &'static str, path: &str, side: &'static str, source_line: u32) -> Self {
        Self {
            kind,
            path: Some(path.to_string()),
            file_status: None,
            sidebar_file_status: None,
            added_lines: None,
            removed_lines: None,
            side: Some(side),
            source_start_line: Some(source_line),
            source_line: Some(source_line),
            code_start_col: Some(LINE_PREFIX_WIDTH),
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_body_line: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            collapsed: None,
            thread_comment_count: None,
            thread_summary: None,
            anchor_placement: None,
            placement_state: Some("inline"),
        }
    }

    fn comment(
        thread: &ReviewThread,
        comment: Option<&ReviewComment>,
        comment_body_line: Option<String>,
        invalidates_later_activity: bool,
    ) -> Self {
        Self {
            kind: ROW_KIND_COMMENT,
            path: thread.path.clone(),
            file_status: None,
            sidebar_file_status: None,
            added_lines: None,
            removed_lines: None,
            side: thread.anchor.side.as_deref().and_then(rendered_side_name),
            source_start_line: thread.anchor.start_line,
            source_line: thread.anchor.end_line,
            code_start_col: None,
            thread_id: Some(thread.id.clone()),
            comment_id: comment.map(|comment| comment.comment.id.to_string()),
            comment_body: comment.map(|comment| comment.comment.body.clone()),
            comment_body_line,
            comment_meta: comment.map(comment_meta),
            can_edit: comment.map(|comment| comment.can_edit),
            invalidates_later_activity: comment.map(|_| invalidates_later_activity),
            resolved: Some(thread.resolved),
            collapsed: Some(thread.collapsed),
            thread_comment_count: Some(thread.comments.len() as u32),
            thread_summary: thread.collapsed.then(|| collapsed_thread_meta(thread)),
            anchor_placement: thread.anchor.placement,
            placement_state: Some(thread_placement_state(thread)),
        }
    }
}

fn rendered_thread_block_range(rows: &[RenderedRow], thread_id: &str) -> Option<(usize, usize)> {
    let mut first = None;
    let mut last = None;
    for (index, row) in rows.iter().enumerate() {
        if row.thread_id.as_deref() == Some(thread_id) {
            first.get_or_insert(index);
            last = Some(index);
        } else if first.is_some() {
            break;
        }
    }
    Some((first?, last?))
}

#[derive(Clone, Debug, Facet)]
struct RenderedHighlight {
    line: u32,
    start_col: u32,
    end_col: u32,
    group: &'static str,
}

#[derive(Clone, Debug, Facet)]
struct SourceDecoration {
    path: String,
    line: u32,
    thread_id: String,
    resolved: bool,
    group: &'static str,
}

#[derive(Debug)]
struct RenderedSymbol {
    name: String,
    detail: &'static str,
    kind: SymbolKind,
    start_line: u32,
    end_line: u32,
    parent: Option<usize>,
}

fn render_review_payload(review: ReviewProjection) -> RenderedReview {
    let mut rendered = RenderedReview {
        lines: Vec::new(),
        rows: Vec::new(),
        highlights: Vec::new(),
        source_decorations: Vec::new(),
        symbols: Vec::new(),
        sidebar: RenderedSidebar::default(),
        sidebar_counts: RenderedSidebarCounts::default(),
    };
    let mut rendered_thread_ids = BTreeSet::<String>::new();
    rendered.source_decorations = source_decorations(&review);

    if !review.files.iter().any(review_file_is_visible) {
        render_empty_review(&mut rendered);
        rendered.sidebar = render_sidebar(&rendered.rows);
        return rendered;
    }

    for file in &review.files {
        if !review_file_is_visible(file) {
            continue;
        }
        rendered.sidebar_counts.files += 1;

        let file_line = rendered.push_line(
            format!(
                "{FILE_HEADER_PREFIX}{}  {:?}  +{} -{}",
                file.path, file.status, file.added_lines, file.removed_lines
            ),
            RenderedRow::file_header(file),
        );
        rendered.push_highlight(
            file_line,
            0,
            rendered.lines[file_line as usize].len() as u32,
            HIGHLIGHT_FILE_HEADER,
        );
        let file_symbol = rendered.symbols.len();
        rendered.symbols.push(RenderedSymbol {
            name: file.path.clone(),
            detail: SYMBOL_FILE_DETAIL,
            kind: SymbolKind::FILE,
            start_line: file_line,
            end_line: file_line,
            parent: None,
        });

        let Some(diff) = review.file_diffs_by_path.get(&file.path) else {
            continue;
        };
        let content = review.file_contents_by_path.get(&file.path);
        let file_threads: Vec<_> = review
            .threads
            .iter()
            .filter(|thread| review_thread_visible_in_default_projection(thread, &review))
            .filter(|thread| thread.path.as_deref() == Some(file.path.as_str()))
            .collect();
        rendered.sidebar_counts.comments += file_threads.len() as u32;

        for hunk in &diff.hunks {
            let hunk_text = hunk_header(hunk.old, hunk.new);
            let hunk_line = rendered.push_line(
                hunk_text.clone(),
                RenderedRow::file_meta(ROW_KIND_HUNK_HEADER, &file.path),
            );
            rendered.push_highlight(hunk_line, 0, hunk_text.len() as u32, HIGHLIGHT_HUNK_HEADER);
            rendered.symbols.push(RenderedSymbol {
                name: hunk_symbol_name(hunk.new, hunk.old),
                detail: SYMBOL_HUNK_DETAIL,
                kind: SymbolKind::MODULE,
                start_line: hunk_line,
                end_line: hunk_line,
                parent: Some(file_symbol),
            });

            for section in &hunk.sections {
                match section {
                    DiffSection::Context { context } => {
                        for line in context.new.start..=context.new.end {
                            let text = content
                                .and_then(|content| content.new.as_ref())
                                .and_then(|lines| source_text(lines, line))
                                .unwrap_or_default();
                            let source_row = push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_CONTEXT,
                                SIDE_NEW,
                                Some(line),
                                Some(line),
                                " ",
                                text,
                            );
                            push_attachment_highlights(
                                &mut rendered,
                                &file_threads,
                                &file.path,
                                SIDE_NEW,
                                line,
                                source_row,
                            );
                            push_inline_threads(
                                &mut rendered,
                                &mut rendered_thread_ids,
                                &file_threads,
                                &file.path,
                                SIDE_NEW,
                                line,
                            );
                        }
                    }
                    DiffSection::Added { added } => {
                        for line in added.new.start..=added.new.end {
                            let text = content
                                .and_then(|content| content.new.as_ref())
                                .and_then(|lines| source_text(lines, line))
                                .unwrap_or_default();
                            let source_row = push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_ADD,
                                SIDE_NEW,
                                None,
                                Some(line),
                                "+",
                                text,
                            );
                            push_attachment_highlights(
                                &mut rendered,
                                &file_threads,
                                &file.path,
                                SIDE_NEW,
                                line,
                                source_row,
                            );
                            push_inline_threads(
                                &mut rendered,
                                &mut rendered_thread_ids,
                                &file_threads,
                                &file.path,
                                SIDE_NEW,
                                line,
                            );
                        }
                    }
                    DiffSection::Removed { removed } => {
                        for line in removed.old.start..=removed.old.end {
                            let text = content
                                .and_then(|content| content.old.as_ref())
                                .and_then(|lines| source_text(lines, line))
                                .unwrap_or_default();
                            let source_row = push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_DELETE,
                                SIDE_OLD,
                                Some(line),
                                None,
                                "-",
                                text,
                            );
                            push_attachment_highlights(
                                &mut rendered,
                                &file_threads,
                                &file.path,
                                SIDE_OLD,
                                line,
                                source_row,
                            );
                            push_inline_threads(
                                &mut rendered,
                                &mut rendered_thread_ids,
                                &file_threads,
                                &file.path,
                                SIDE_OLD,
                                line,
                            );
                        }
                    }
                }
            }
        }

        for thread in file_threads
            .iter()
            .copied()
            .filter(|thread| thread_needs_file_fallback_block(thread))
        {
            push_thread_block_once(&mut rendered, &mut rendered_thread_ids, thread);
        }

        for thread in file_threads
            .iter()
            .copied()
            .filter(|thread| thread.scope == THREAD_SCOPE_FILE)
        {
            push_thread_block_once(&mut rendered, &mut rendered_thread_ids, thread);
        }

        let end_line = rendered.lines.len().saturating_sub(1) as u32;
        if let Some(symbol) = rendered.symbols.get_mut(file_symbol) {
            symbol.end_line = end_line;
        }
    }

    for index in 0..rendered.symbols.len() {
        let Some(parent) = rendered.symbols[index].parent else {
            continue;
        };
        let end_line = rendered.symbols[index].end_line;
        if let Some(parent) = rendered.symbols.get_mut(parent) {
            parent.end_line = parent.end_line.max(end_line);
        }
    }

    rendered.sidebar = render_sidebar(&rendered.rows);
    rendered
}

fn source_decorations(review: &ReviewProjection) -> Vec<SourceDecoration> {
    let mut decorations = Vec::new();
    let visible_paths: BTreeSet<_> = review
        .files
        .iter()
        .filter(|file| review_file_is_visible(file))
        .map(|file| file.path.as_str())
        .collect();

    for thread in review
        .threads
        .iter()
        .filter(|thread| review_thread_visible_in_default_projection(thread, review))
    {
        let Some(path) = thread.path.as_ref() else {
            continue;
        };
        if !visible_paths.contains(path.as_str()) {
            continue;
        }
        if thread.anchor.side.as_deref() != Some(SIDE_NEW) {
            continue;
        }

        let group = thread_source_decoration_highlight(thread);
        let mut seen_lines = BTreeSet::new();
        for placement in &thread.anchor.line_placements {
            let Some(line) = placement.current_line else {
                continue;
            };
            if seen_lines.insert(line) {
                decorations.push(SourceDecoration {
                    path: path.clone(),
                    line,
                    thread_id: thread.id.clone(),
                    resolved: thread.resolved,
                    group: thread_line_source_decoration_highlight(thread, line),
                });
            }
        }
        if !seen_lines.is_empty() {
            continue;
        }

        let Some(start_line) = thread.anchor.start_line else {
            continue;
        };
        let end_line = thread.anchor.end_line.unwrap_or(start_line);
        for line in start_line.min(end_line)..=start_line.max(end_line) {
            decorations.push(SourceDecoration {
                path: path.clone(),
                line,
                thread_id: thread.id.clone(),
                resolved: thread.resolved,
                group,
            });
        }
    }

    decorations
}

fn review_file_is_visible(file: &crate::diff::ReviewFile) -> bool {
    file.is_changed || file.comment_count > 0
}

#[derive(Debug)]
struct SidebarFile {
    target_line: usize,
    path: String,
    name: String,
    status: Option<FileStatus>,
    added_lines: u32,
    removed_lines: u32,
    comment_count: u32,
}

#[derive(Debug)]
struct SidebarThread {
    id: String,
    target_line: usize,
    label: String,
    resolved: bool,
    collapsed: bool,
    comment_count: u32,
    anchor_placement: Option<AnchorPlacement>,
    summary: Option<String>,
    comments: Vec<SidebarComment>,
    seen_comments: BTreeSet<String>,
}

#[derive(Debug)]
struct SidebarComment {
    target_line: usize,
    body: String,
    meta: String,
}

fn render_sidebar(rows: &[RenderedRow]) -> RenderedSidebar {
    RenderedSidebar {
        files: render_sidebar_files(rows),
        comments: render_sidebar_comments(rows),
    }
}

fn render_sidebar_files(rows: &[RenderedRow]) -> RenderedSidebarPanel {
    let counts = sidebar_comment_counts(rows);
    let mut groups: Vec<(String, Vec<SidebarFile>)> = Vec::new();
    let mut seen = BTreeSet::new();

    for (index, row) in rows.iter().enumerate() {
        if row.kind != ROW_KIND_FILE_HEADER {
            continue;
        }
        let Some(path) = &row.path else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let dir = dirname(path);
        let file = SidebarFile {
            target_line: index + 1,
            path: path.clone(),
            name: basename(path),
            status: row.sidebar_file_status,
            added_lines: row.added_lines.unwrap_or(0),
            removed_lines: row.removed_lines.unwrap_or(0),
            comment_count: counts.get(path).copied().unwrap_or(0),
        };
        push_sidebar_group_item(&mut groups, dir, file);
    }

    let mut panel = RenderedSidebarPanel::default();
    for (dir, files) in groups {
        let folder_prefix = format!("{SIDEBAR_FOLDER_ICON} ");
        panel.push_line(
            format!(
                "{folder_prefix}{}",
                truncate_start(
                    &dir,
                    SIDEBAR_WIDTH.saturating_sub(display_width(&folder_prefix))
                )
            ),
            RenderedSidebarRow::path(
                files.first().map(|file| file.target_line).unwrap_or(1),
                files
                    .first()
                    .map(|file| file.path.clone())
                    .unwrap_or_default(),
            ),
        );
        for file in files {
            push_sidebar_file(&mut panel, file);
        }
    }

    if panel.lines.is_empty() {
        panel.push_line("No files".to_string(), RenderedSidebarRow::default());
    }
    panel
}

fn push_sidebar_file(panel: &mut RenderedSidebarPanel, file: SidebarFile) {
    let prefix = format!(
        "{SIDEBAR_FILE_PREFIX}{} ",
        sidebar_file_status_sign(file.status)
    );
    let delta_parts = sidebar_file_delta_parts(&file);
    let suffix = sidebar_file_delta_suffix(&delta_parts);
    let name_width = SIDEBAR_WIDTH
        .saturating_sub(display_width(&prefix))
        .saturating_sub(display_width(&suffix));
    let name = truncate_start(&file.name, name_width);
    let line_text = format!("{prefix}{name}{suffix}");
    let line = panel.push_line(
        line_text,
        RenderedSidebarRow::path(file.target_line, file.path),
    );
    panel.push_highlight(
        line,
        SIDEBAR_FILE_PREFIX.len() as u32,
        (SIDEBAR_FILE_PREFIX.len() + 1) as u32,
        sidebar_file_status_highlight(file.status),
    );

    let mut col = prefix.len() + name.len();
    for part in delta_parts {
        col += 1;
        if let Some(group) = part.highlight {
            panel.push_highlight(line, col as u32, (col + part.text.len()) as u32, group);
        }
        col += part.text.len();
    }
}

fn render_sidebar_comments(rows: &[RenderedRow]) -> RenderedSidebarPanel {
    let mut groups: Vec<(String, Vec<SidebarThread>)> = Vec::new();
    let mut by_id: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for (index, row) in rows.iter().enumerate() {
        if row.kind != ROW_KIND_COMMENT {
            continue;
        }
        let Some(thread_id) = &row.thread_id else {
            continue;
        };
        let (group_index, thread_index) =
            if let Some(&(group_index, thread_index)) = by_id.get(thread_id) {
                (group_index, thread_index)
            } else {
                let dir = dirname(row.path.as_deref().unwrap_or("review"));
                let group_index = sidebar_group_index(&mut groups, dir);
                let thread_index = groups[group_index].1.len();
                groups[group_index].1.push(SidebarThread {
                    id: thread_id.clone(),
                    target_line: index + 1,
                    label: sidebar_thread_label(row),
                    resolved: row.resolved.unwrap_or(false),
                    collapsed: row.collapsed.unwrap_or(false),
                    comment_count: row.thread_comment_count.unwrap_or(0),
                    anchor_placement: row.anchor_placement,
                    summary: row.thread_summary.clone(),
                    comments: Vec::new(),
                    seen_comments: BTreeSet::new(),
                });
                by_id.insert(thread_id.clone(), (group_index, thread_index));
                (group_index, thread_index)
            };

        let thread = &mut groups[group_index].1[thread_index];
        if thread.summary.is_none() {
            thread.summary = row.thread_summary.clone();
        }
        let Some(comment_id) = &row.comment_id else {
            continue;
        };
        if !thread.seen_comments.insert(comment_id.clone()) {
            continue;
        }
        if let (Some(body), Some(meta)) = (&row.comment_body, &row.comment_meta) {
            thread.comments.push(SidebarComment {
                target_line: index + 1,
                body: body.clone(),
                meta: meta.clone(),
            });
        }
    }

    let mut panel = RenderedSidebarPanel::default();
    for (dir, threads) in groups {
        let folder_prefix = format!("{SIDEBAR_FOLDER_ICON} ");
        panel.push_line(
            format!(
                "{folder_prefix}{}",
                truncate_start(
                    &dir,
                    SIDEBAR_WIDTH.saturating_sub(display_width(&folder_prefix))
                )
            ),
            RenderedSidebarRow::target(
                threads
                    .first()
                    .map(|thread| thread.target_line)
                    .unwrap_or(1),
            ),
        );
        for thread in threads {
            push_sidebar_thread(&mut panel, thread);
        }
    }

    if panel.lines.is_empty() {
        panel.push_line("No comments".to_string(), RenderedSidebarRow::default());
    }
    panel
}

fn push_sidebar_thread(panel: &mut RenderedSidebarPanel, thread: SidebarThread) {
    let status = if thread.resolved {
        THREAD_STATUS_RESOLVED_ICON
    } else {
        THREAD_STATUS_OPEN_ICON
    };
    let header_prefix = format!("{THREAD_HEADER_PREFIX}{status} ");
    let header_suffix = if thread.collapsed {
        format!(" [{}]", thread.comment_count)
    } else {
        String::new()
    };
    let label_width = SIDEBAR_WIDTH
        .saturating_sub(display_width(&header_prefix))
        .saturating_sub(display_width(&header_suffix));
    let header = format!(
        "{header_prefix}{}{header_suffix}",
        truncate_start(&thread.label, label_width)
    );
    let line = panel.push_line(
        header,
        RenderedSidebarRow::thread(thread.target_line, &thread.id),
    );
    if let Some(group) = sidebar_thread_border_highlight(thread.resolved) {
        panel.push_highlight(line, 0, header_prefix.len() as u32, group);
    }

    if !thread.collapsed {
        let visible_comments =
            visible_comment_indexes(thread.comments.len(), SIDEBAR_THREAD_MAX_VISIBLE_COMMENTS);
        for (visible_index, comment_index) in visible_comments.iter().copied().enumerate() {
            if let Some(hidden_count) =
                hidden_comment_count_before(&visible_comments, visible_index)
            {
                push_sidebar_comment_elision(
                    panel,
                    &thread.id,
                    thread.target_line,
                    hidden_count,
                    thread.resolved,
                );
            }
            push_sidebar_comment(
                panel,
                &thread.id,
                &thread.comments[comment_index],
                thread.resolved,
            );
        }
    }

    let footer_prefix = format!("{THREAD_FOOTER} ");
    let footer_text = if thread.collapsed {
        thread
            .summary
            .as_deref()
            .unwrap_or(THREAD_EMPTY_PREVIEW)
            .to_string()
    } else {
        placement_location_note(thread.anchor_placement).to_string()
    };
    let footer = format!(
        "{footer_prefix}{}",
        truncate_start(
            &footer_text,
            SIDEBAR_WIDTH.saturating_sub(display_width(&footer_prefix))
        )
    );
    let line = panel.push_line(
        footer,
        RenderedSidebarRow::thread(thread.target_line, &thread.id),
    );
    if let Some(group) = sidebar_thread_border_highlight(thread.resolved) {
        panel.push_highlight(line, 0, THREAD_FOOTER.len() as u32, group);
    }
    panel.push_highlight(
        line,
        footer_prefix.len() as u32,
        panel.lines[line as usize].len() as u32,
        HIGHLIGHT_SIDEBAR_THREAD_LOCATION_NOTE,
    );
}

fn push_sidebar_comment(
    panel: &mut RenderedSidebarPanel,
    thread_id: &str,
    comment: &SidebarComment,
    resolved: bool,
) {
    let meta_prefix = THREAD_BODY_PREFIX;
    let meta = format!(
        "{meta_prefix}{}",
        truncate_start(
            &comment.meta,
            SIDEBAR_WIDTH.saturating_sub(display_width(meta_prefix))
        )
    );
    let line = panel.push_line(
        meta,
        RenderedSidebarRow::thread(comment.target_line, thread_id),
    );
    if let Some(group) = sidebar_thread_border_highlight(resolved) {
        panel.push_highlight(line, 0, meta_prefix.len() as u32, group);
    }
    panel.push_highlight(
        line,
        meta_prefix.len() as u32,
        panel.lines[line as usize].len() as u32,
        HIGHLIGHT_SIDEBAR_THREAD_META,
    );

    let body = first_line(&comment.body);
    let body_line = format!(
        "{meta_prefix}{}",
        truncate_end(
            &body,
            SIDEBAR_WIDTH.saturating_sub(display_width(meta_prefix))
        )
    );
    let line = panel.push_line(
        body_line,
        RenderedSidebarRow::thread(comment.target_line, thread_id),
    );
    if let Some(group) = sidebar_thread_border_highlight(resolved) {
        panel.push_highlight(line, 0, meta_prefix.len() as u32, group);
    }
}

fn push_sidebar_comment_elision(
    panel: &mut RenderedSidebarPanel,
    thread_id: &str,
    target_line: usize,
    hidden_count: usize,
    resolved: bool,
) {
    let prefix = THREAD_COMMENT_ELISION_PREFIX;
    let label = format!("… {hidden_count} {THREAD_COMMENT_ELISION_SUFFIX} …");
    let line_text = format!(
        "{prefix}{}",
        truncate_end(&label, SIDEBAR_WIDTH.saturating_sub(display_width(prefix)))
    );
    let line = panel.push_line(
        line_text,
        RenderedSidebarRow::thread(target_line, thread_id),
    );
    if let Some(group) = sidebar_thread_border_highlight(resolved) {
        panel.push_highlight(line, 0, prefix.len() as u32, group);
    }
    panel.push_highlight(
        line,
        prefix.len() as u32,
        panel.lines[line as usize].len() as u32,
        HIGHLIGHT_SIDEBAR_COMMENT_ELISION,
    );
}

#[derive(Debug)]
struct SidebarDeltaPart {
    text: String,
    highlight: Option<&'static str>,
}

fn sidebar_comment_counts(rows: &[RenderedRow]) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        if row.kind != ROW_KIND_COMMENT {
            continue;
        }
        let (Some(path), Some(thread_id)) = (&row.path, &row.thread_id) else {
            continue;
        };
        if seen.insert(thread_id.clone()) {
            *counts.entry(path.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn push_sidebar_group_item<T>(groups: &mut Vec<(String, Vec<T>)>, key: String, item: T) {
    let index = sidebar_group_index(groups, key);
    groups[index].1.push(item);
}

fn sidebar_group_index<T>(groups: &mut Vec<(String, Vec<T>)>, key: String) -> usize {
    if let Some(index) = groups.iter().position(|(existing, _)| existing == &key) {
        return index;
    }
    groups.push((key, Vec::new()));
    groups.len() - 1
}

fn dirname(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_else(|| "./".to_string())
}

fn basename(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn sidebar_thread_label(row: &RenderedRow) -> String {
    let name = row
        .path
        .as_deref()
        .map(basename)
        .unwrap_or_else(|| "review".to_string());
    let start_line = row.source_start_line.or(row.source_line);
    let end_line = row.source_line.or(row.source_start_line);
    match (start_line, end_line) {
        (Some(start), Some(end)) if start != end => format!("{name}:{start}-{end}"),
        (_, Some(end)) => format!("{name}:{end}"),
        (Some(start), _) => format!("{name}:{start}"),
        _ => name,
    }
}

fn sidebar_file_status_sign(status: Option<FileStatus>) -> &'static str {
    match status {
        Some(FileStatus::Added) => "A",
        Some(FileStatus::Deleted) => "D",
        Some(FileStatus::Modified) => "M",
        Some(FileStatus::Renamed) => "R",
        Some(FileStatus::Unchanged) => "U",
        Some(FileStatus::Binary) => "B",
        _ => "?",
    }
}

fn sidebar_file_status_highlight(status: Option<FileStatus>) -> &'static str {
    match status {
        Some(FileStatus::Added) => HIGHLIGHT_SIDEBAR_STATUS_ADDED,
        Some(FileStatus::Deleted) => HIGHLIGHT_SIDEBAR_STATUS_DELETED,
        Some(FileStatus::Modified) => HIGHLIGHT_SIDEBAR_STATUS_MODIFIED,
        Some(FileStatus::Renamed) => HIGHLIGHT_SIDEBAR_STATUS_RENAMED,
        Some(FileStatus::Unchanged) => HIGHLIGHT_SIDEBAR_STATUS_UNCHANGED,
        Some(FileStatus::Binary) => HIGHLIGHT_SIDEBAR_STATUS_BINARY,
        _ => HIGHLIGHT_SIDEBAR_STATUS_UNCHANGED,
    }
}

fn file_status_name(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "Added",
        FileStatus::Deleted => "Deleted",
        FileStatus::Modified => "Modified",
        FileStatus::Renamed => "Renamed",
        FileStatus::Unchanged => "Unchanged",
        FileStatus::Binary => "Binary",
    }
}

fn sidebar_file_delta_parts(file: &SidebarFile) -> Vec<SidebarDeltaPart> {
    let mut parts = Vec::new();
    if file.added_lines > 0 {
        parts.push(SidebarDeltaPart {
            text: format!("+{}", file.added_lines),
            highlight: Some(HIGHLIGHT_SIDEBAR_DELTA_ADDED),
        });
    }
    if file.removed_lines > 0 {
        parts.push(SidebarDeltaPart {
            text: format!("−{}", file.removed_lines),
            highlight: Some(HIGHLIGHT_SIDEBAR_DELTA_REMOVED),
        });
    }
    if file.added_lines > 0 || file.removed_lines > 0 {
        let delta = file.added_lines as i64 - file.removed_lines as i64;
        let sign = if delta > 0 { "+" } else { "" };
        let highlight = if delta > 0 {
            HIGHLIGHT_SIDEBAR_DELTA_POSITIVE
        } else if delta < 0 {
            HIGHLIGHT_SIDEBAR_DELTA_NEGATIVE
        } else {
            HIGHLIGHT_SIDEBAR_DELTA_NEUTRAL
        };
        parts.push(SidebarDeltaPart {
            text: format!("Δ{sign}{delta}"),
            highlight: Some(highlight),
        });
    }
    if file.comment_count > 0 {
        parts.push(SidebarDeltaPart {
            text: format!("{THREAD_STATUS_OPEN_ICON}{}", file.comment_count),
            highlight: None,
        });
    }
    parts
}

fn sidebar_file_delta_suffix(parts: &[SidebarDeltaPart]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let mut suffix = String::new();
    for part in parts {
        suffix.push(' ');
        suffix.push_str(&part.text);
    }
    suffix
}

fn sidebar_thread_border_highlight(resolved: bool) -> Option<&'static str> {
    resolved.then_some(HIGHLIGHT_SIDEBAR_THREAD_RESOLVED)
}

fn placement_location_note(placement: Option<AnchorPlacement>) -> &'static str {
    placement
        .map(AnchorPlacement::location_note)
        .unwrap_or("source location")
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

fn display_width(text: &str) -> usize {
    text.chars().count()
}

fn truncate_start(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    let marker = TRUNCATION_SUFFIX;
    let marker_width = display_width(marker);
    if width <= marker_width {
        return text.chars().take(width).collect();
    }
    let keep = width - marker_width;
    let mut chars = text.chars().rev().take(keep).collect::<Vec<_>>();
    chars.reverse();
    format!("{marker}{}", chars.into_iter().collect::<String>())
}

fn truncate_end(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    let marker = TRUNCATION_SUFFIX;
    let marker_width = display_width(marker);
    if width <= marker_width {
        return text.chars().take(width).collect();
    }
    format!(
        "{}{marker}",
        text.chars().take(width - marker_width).collect::<String>()
    )
}

fn review_thread_visible_in_default_projection(
    thread: &ReviewThread,
    review: &ReviewProjection,
) -> bool {
    thread_visible_in_default_projection(
        thread.resolved,
        thread.resolved_head_oid.as_deref(),
        review.current_head_oid.as_deref(),
    )
}

fn render_empty_review(rendered: &mut RenderedReview) {
    let start_line = rendered.push_line(empty_border(), RenderedRow::meta(ROW_KIND_EMPTY));
    rendered.push_line(empty_card_line(""), RenderedRow::meta(ROW_KIND_EMPTY));
    let title_line = rendered.push_line(
        empty_card_line(EMPTY_TITLE),
        RenderedRow::meta(ROW_KIND_EMPTY),
    );
    rendered.push_line(
        empty_card_line(EMPTY_BODY),
        RenderedRow::meta(ROW_KIND_EMPTY),
    );
    rendered.push_line(
        empty_card_line(EMPTY_REFRESH),
        RenderedRow::meta(ROW_KIND_EMPTY),
    );
    rendered.push_line(empty_card_line(""), RenderedRow::meta(ROW_KIND_EMPTY));
    let end_line = rendered.push_line(empty_border(), RenderedRow::meta(ROW_KIND_EMPTY));

    rendered.push_highlight(
        title_line,
        0,
        rendered.lines[title_line as usize].len() as u32,
        HIGHLIGHT_EMPTY_TITLE,
    );
    for line in start_line..=end_line {
        if line != title_line {
            rendered.push_highlight(
                line,
                0,
                rendered.lines[line as usize].len() as u32,
                HIGHLIGHT_EMPTY_TEXT,
            );
        }
    }
    rendered.symbols.push(RenderedSymbol {
        name: EMPTY_SYMBOL_NAME.to_string(),
        detail: EMPTY_SYMBOL_DETAIL,
        kind: SymbolKind::FILE,
        start_line,
        end_line,
        parent: None,
    });
}

fn empty_border() -> String {
    format!(
        "{EMPTY_CARD_MARGIN}+{}+",
        "-".repeat(EMPTY_CARD_WIDTH.saturating_sub(2))
    )
}

fn empty_card_line(text: &str) -> String {
    let inner_width = EMPTY_CARD_WIDTH.saturating_sub(4);
    let truncated: String = text.chars().take(inner_width).collect();
    format!(
        "{EMPTY_CARD_MARGIN}| {truncated:<inner_width$} |",
        inner_width = inner_width
    )
}

fn push_source_line(
    rendered: &mut RenderedReview,
    path: &str,
    kind: &'static str,
    side: &'static str,
    old_line: Option<u32>,
    new_line: Option<u32>,
    sign: &str,
    text: &str,
) -> u32 {
    let line_number = match side {
        SIDE_NEW => new_line,
        SIDE_OLD => old_line,
        _ => None,
    }
    .unwrap_or(0);
    let line = rendered.push_line(
        format!(
            "{} {} {sign} {text}",
            format_line_number(old_line),
            format_line_number(new_line)
        ),
        RenderedRow::source(kind, path, side, line_number),
    );
    if let Some(background_group) = gutter_background_group(kind) {
        rendered.push_highlight(line, 0, LINE_PREFIX_WIDTH, background_group);
    }
    if let Some(background_group) = line_background_group(kind) {
        let end_col = rendered.lines[line as usize].len() as u32;
        rendered.push_highlight(line, LINE_PREFIX_WIDTH, end_col, background_group);
    }
    rendered.push_highlight(line, 0, 5, HIGHLIGHT_LINE_NUMBER);
    rendered.push_highlight(line, 6, 11, HIGHLIGHT_LINE_NUMBER);
    let gutter_group = match kind {
        ROW_KIND_ADD => HIGHLIGHT_ADD_GUTTER,
        ROW_KIND_DELETE => HIGHLIGHT_DELETE_GUTTER,
        _ => HIGHLIGHT_LINE_NUMBER,
    };
    rendered.push_highlight(line, 12, 13, gutter_group);
    line
}

fn push_attachment_highlights(
    rendered: &mut RenderedReview,
    threads: &[&ReviewThread],
    path: &str,
    side: &str,
    line: u32,
    rendered_line: u32,
) {
    let Some(thread) = threads
        .iter()
        .copied()
        .find(|thread| thread_covers_line(thread, path, side, line))
    else {
        return;
    };

    let end_col = rendered.lines[rendered_line as usize].len() as u32;
    rendered.push_highlight(
        rendered_line,
        LINE_PREFIX_WIDTH,
        end_col,
        HIGHLIGHT_THREAD_ATTACHMENT,
    );
    rendered.push_highlight(
        rendered_line,
        THREAD_RAIL_START_COL,
        THREAD_RAIL_END_COL,
        thread_line_rail_highlight(thread, line),
    );
}

fn push_inline_threads(
    rendered: &mut RenderedReview,
    rendered_thread_ids: &mut BTreeSet<String>,
    threads: &[&ReviewThread],
    path: &str,
    side: &str,
    line: u32,
) {
    for thread in threads
        .iter()
        .copied()
        .filter(|thread| thread_matches_line(thread, path, side, line))
    {
        push_thread_block_once(rendered, rendered_thread_ids, thread);
    }
}

fn thread_needs_file_fallback_block(thread: &ReviewThread) -> bool {
    thread.scope == THREAD_SCOPE_LINE
        && thread.path.is_some()
        && thread.anchor.start_line.is_none()
        && thread.anchor.end_line.is_none()
}

fn thread_matches_line(thread: &ReviewThread, path: &str, side: &str, line: u32) -> bool {
    thread.scope == THREAD_SCOPE_LINE
        && thread.path.as_deref() == Some(path)
        && thread.anchor.side.as_deref() == Some(side)
        && thread.anchor.end_line == Some(line)
}

fn thread_covers_line(thread: &ReviewThread, path: &str, side: &str, line: u32) -> bool {
    if thread.scope != THREAD_SCOPE_LINE
        || thread.path.as_deref() != Some(path)
        || thread.anchor.side.as_deref() != Some(side)
    {
        return false;
    }

    let start = thread.anchor.start_line.or(thread.anchor.end_line);
    let end = thread.anchor.end_line.or(thread.anchor.start_line);
    matches!((start, end), (Some(start), Some(end)) if line >= start && line <= end)
}

fn push_thread_block(rendered: &mut RenderedReview, thread: &ReviewThread) {
    render_comment_thread(
        rendered,
        thread,
        RenderCommentOptions {
            show_path: true,
            max_visible_comments: 6,
            collapsed: thread.collapsed,
        },
    );
}

fn push_thread_block_once(
    rendered: &mut RenderedReview,
    rendered_thread_ids: &mut BTreeSet<String>,
    thread: &ReviewThread,
) {
    if rendered_thread_ids.insert(thread.id.clone()) {
        push_thread_block(rendered, thread);
    }
}

#[derive(Clone, Copy)]
struct RenderCommentOptions {
    show_path: bool,
    max_visible_comments: usize,
    collapsed: bool,
}

fn render_comment_thread(
    rendered: &mut RenderedReview,
    thread: &ReviewThread,
    options: RenderCommentOptions,
) {
    let header = format!(
        "{THREAD_HEADER_PREFIX}{} {}{}",
        thread_status_icon(thread),
        thread_label_for_options(thread, options),
        collapsed_count_suffix(thread, options)
    );
    push_thread_line(
        rendered,
        header,
        thread,
        None,
        None,
        false,
        HIGHLIGHT_THREAD_HEADER,
    );

    if options.collapsed {
        push_collapsed_thread_footer(rendered, thread);
        return;
    }

    let visible_comments =
        visible_comment_indexes(thread.comments.len(), options.max_visible_comments);
    for (visible_index, comment_index) in visible_comments.iter().copied().enumerate() {
        if let Some(hidden_count) = hidden_comment_count_before(&visible_comments, visible_index) {
            push_thread_elision(rendered, thread, hidden_count);
        }
        let comment = &thread.comments[comment_index];
        let invalidates_later_activity =
            comment_index + 1 < thread.comments.len() || thread.resolved;
        push_thread_comment(
            rendered,
            thread,
            comment,
            visible_index > 0,
            invalidates_later_activity,
        );
    }

    push_thread_footer(rendered, thread);
}

fn thread_status_icon(thread: &ReviewThread) -> &'static str {
    if thread.resolved {
        THREAD_STATUS_RESOLVED_ICON
    } else {
        THREAD_STATUS_OPEN_ICON
    }
}

fn thread_label_for_options(thread: &ReviewThread, options: RenderCommentOptions) -> String {
    if options.show_path {
        return thread.line_label.clone();
    }
    thread_label_without_path(thread)
}

fn thread_label_without_path(thread: &ReviewThread) -> String {
    let name = thread
        .path
        .as_deref()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .unwrap_or("review");
    let start_line = thread.anchor.start_line.or(thread.anchor.end_line);
    let end_line = thread.anchor.end_line.or(thread.anchor.start_line);
    match (start_line, end_line) {
        (Some(start), Some(end)) if start != end => format!("{name}:{start}-{end}"),
        (_, Some(end)) => format!("{name}:{end}"),
        (Some(start), _) => format!("{name}:{start}"),
        _ => name.to_string(),
    }
}

fn collapsed_count_suffix(thread: &ReviewThread, options: RenderCommentOptions) -> String {
    if options.collapsed {
        format!(" [{}]", thread.comments.len())
    } else {
        String::new()
    }
}

fn visible_comment_indexes(count: usize, max_visible_comments: usize) -> Vec<usize> {
    if count <= max_visible_comments || max_visible_comments < 3 {
        return (0..count).collect();
    }
    vec![0, count - 1]
}

fn hidden_comment_count_before(visible_comments: &[usize], visible_index: usize) -> Option<usize> {
    if visible_index == 0 {
        return None;
    }
    let previous = visible_comments[visible_index - 1];
    let current = visible_comments[visible_index];
    let hidden_count = current.saturating_sub(previous + 1);
    (hidden_count > 0).then_some(hidden_count)
}

fn push_thread_comment(
    rendered: &mut RenderedReview,
    thread: &ReviewThread,
    comment: &ReviewComment,
    is_reply: bool,
    invalidates_later_activity: bool,
) {
    if is_reply {
        push_thread_line(
            rendered,
            THREAD_BODY_PREFIX.to_string(),
            thread,
            None,
            None,
            false,
            HIGHLIGHT_THREAD_BODY,
        );
    }

    let meta_prefix = if is_reply {
        THREAD_REPLY_META_PREFIX
    } else {
        THREAD_BODY_PREFIX
    };
    let meta = format!("{meta_prefix}{}", comment_meta(comment));
    push_thread_line(
        rendered,
        meta,
        thread,
        Some(comment),
        None,
        invalidates_later_activity,
        HIGHLIGHT_THREAD_META,
    );

    for line in formatted_comment_body_lines(&comment.comment.body) {
        push_thread_line(
            rendered,
            format!("{THREAD_BODY_PREFIX}{line}"),
            thread,
            Some(comment),
            Some(line),
            invalidates_later_activity,
            HIGHLIGHT_THREAD_BODY,
        );
    }
}

fn push_thread_elision(rendered: &mut RenderedReview, thread: &ReviewThread, hidden_count: usize) {
    push_thread_line(
        rendered,
        format!(
            "{THREAD_COMMENT_ELISION_PREFIX}… {hidden_count} {THREAD_COMMENT_ELISION_SUFFIX} …"
        ),
        thread,
        None,
        None,
        false,
        HIGHLIGHT_THREAD_BODY,
    );
}

fn push_collapsed_thread_footer(rendered: &mut RenderedReview, thread: &ReviewThread) {
    let meta = collapsed_thread_meta(thread);
    let line_text = format!("{THREAD_FOOTER} {meta}");
    let line = rendered.push_line(
        truncate_display_line(thread_card_line(&line_text)),
        RenderedRow::comment(thread, None, None, false),
    );
    let border_start = thread_card_start_col();
    let border_end = border_start + THREAD_FOOTER.len() as u32;
    let end_col = rendered.lines[line as usize].len() as u32;
    rendered.push_highlight(
        line,
        border_start,
        border_end,
        thread_card_border_highlight(thread),
    );
    if end_col > border_end {
        rendered.push_highlight(line, border_end + 1, end_col, HIGHLIGHT_THREAD_META);
    }
    rendered.push_highlight(
        line,
        THREAD_RAIL_START_COL,
        THREAD_RAIL_END_COL,
        thread_rail_highlight(thread),
    );
}

fn push_thread_line(
    rendered: &mut RenderedReview,
    line_text: String,
    thread: &ReviewThread,
    comment: Option<&ReviewComment>,
    comment_body_line: Option<String>,
    invalidates_later_activity: bool,
    group: &'static str,
) {
    let line = rendered.push_line(
        truncate_display_line(thread_card_line(&line_text)),
        RenderedRow::comment(
            thread,
            comment,
            comment_body_line,
            invalidates_later_activity,
        ),
    );
    let end_col = rendered.lines[line as usize].len() as u32;
    let border_start = thread_card_start_col();
    let border_end = thread_border_end_col(&line_text);
    rendered.push_highlight(
        line,
        border_start,
        border_end,
        thread_card_border_highlight(thread),
    );
    if end_col > border_end {
        rendered.push_highlight(line, border_end, end_col, group);
    }
    rendered.push_highlight(
        line,
        THREAD_RAIL_START_COL,
        THREAD_RAIL_END_COL,
        thread_rail_highlight(thread),
    );
}

fn push_thread_footer(rendered: &mut RenderedReview, thread: &ReviewThread) {
    let note = thread_location_note(thread);
    let line_text = format!("{THREAD_FOOTER} {note}");
    let line = rendered.push_line(
        truncate_display_line(thread_card_line(&line_text)),
        RenderedRow::comment(thread, None, None, false),
    );
    let border_start = thread_card_start_col();
    let border_end = border_start + THREAD_FOOTER.len() as u32;
    let end_col = rendered.lines[line as usize].len() as u32;
    rendered.push_highlight(
        line,
        border_start,
        border_end,
        thread_card_border_highlight(thread),
    );
    if end_col > border_end {
        rendered.push_highlight(
            line,
            border_end + 1,
            end_col,
            HIGHLIGHT_THREAD_LOCATION_NOTE,
        );
    }
    rendered.push_highlight(
        line,
        THREAD_RAIL_START_COL,
        THREAD_RAIL_END_COL,
        thread_rail_highlight(thread),
    );
}

fn thread_card_line(line: &str) -> String {
    format!("{THREAD_CARD_MARGIN}{line}")
}

fn thread_card_start_col() -> u32 {
    THREAD_CARD_MARGIN.len() as u32
}

fn thread_border_end_col(line_text: &str) -> u32 {
    let border_width = if line_text.starts_with(THREAD_HEADER_PREFIX) {
        THREAD_HEADER_PREFIX.len() + THREAD_STATUS_OPEN_ICON.len() + 1
    } else if line_text.starts_with(THREAD_BODY_PREFIX) {
        THREAD_BODY_PREFIX.len()
    } else if line_text.starts_with(THREAD_REPLY_META_PREFIX) {
        THREAD_REPLY_META_PREFIX.len()
    } else if line_text.starts_with(THREAD_FOOTER) {
        THREAD_FOOTER.len()
    } else {
        0
    };
    thread_card_start_col() + border_width as u32
}

fn thread_border_highlight(thread: &ReviewThread) -> &'static str {
    match thread_placement_state(thread) {
        "context" => HIGHLIGHT_THREAD_BORDER_CONTEXT,
        "stale" => HIGHLIGHT_THREAD_BORDER_STALE,
        "file" | "detached" => HIGHLIGHT_THREAD_BORDER_DETACHED,
        _ => HIGHLIGHT_THREAD_BORDER,
    }
}

fn thread_card_border_highlight(thread: &ReviewThread) -> &'static str {
    if thread.resolved {
        HIGHLIGHT_THREAD_RESOLVED
    } else {
        thread_border_highlight(thread)
    }
}

fn thread_rail_highlight(thread: &ReviewThread) -> &'static str {
    match thread_placement_state(thread) {
        "context" => HIGHLIGHT_THREAD_RAIL_CONTEXT,
        "stale" => HIGHLIGHT_THREAD_RAIL_STALE,
        "file" | "detached" => HIGHLIGHT_THREAD_RAIL_DETACHED,
        _ => HIGHLIGHT_THREAD_RAIL,
    }
}

fn thread_line_rail_highlight(thread: &ReviewThread, current_line: u32) -> &'static str {
    let placement = thread
        .anchor
        .line_placements
        .iter()
        .find(|line| line.current_line == Some(current_line))
        .map(|line| line.placement);
    match placement {
        Some(AnchorLinePlacement::Exact | AnchorLinePlacement::Content) => HIGHLIGHT_THREAD_RAIL,
        Some(AnchorLinePlacement::Context | AnchorLinePlacement::Changed) => {
            HIGHLIGHT_THREAD_RAIL_CONTEXT
        }
        Some(AnchorLinePlacement::LineFallback) => HIGHLIGHT_THREAD_RAIL_STALE,
        Some(
            AnchorLinePlacement::Gap | AnchorLinePlacement::Missing | AnchorLinePlacement::Detached,
        ) => HIGHLIGHT_THREAD_RAIL_DETACHED,
        _ => thread_rail_highlight(thread),
    }
}

fn thread_source_decoration_highlight(thread: &ReviewThread) -> &'static str {
    if thread.resolved {
        HIGHLIGHT_THREAD_RESOLVED
    } else {
        thread_border_highlight(thread)
    }
}

fn thread_line_source_decoration_highlight(
    thread: &ReviewThread,
    current_line: u32,
) -> &'static str {
    if thread.resolved {
        return HIGHLIGHT_THREAD_RESOLVED;
    }

    let placement = thread
        .anchor
        .line_placements
        .iter()
        .find(|line| line.current_line == Some(current_line))
        .map(|line| line.placement);
    match placement {
        Some(AnchorLinePlacement::Exact | AnchorLinePlacement::Content) => HIGHLIGHT_THREAD_BORDER,
        Some(AnchorLinePlacement::Context | AnchorLinePlacement::Changed) => {
            HIGHLIGHT_THREAD_BORDER_CONTEXT
        }
        Some(AnchorLinePlacement::LineFallback) => HIGHLIGHT_THREAD_BORDER_STALE,
        Some(
            AnchorLinePlacement::Gap | AnchorLinePlacement::Missing | AnchorLinePlacement::Detached,
        ) => HIGHLIGHT_THREAD_BORDER_DETACHED,
        _ => thread_border_highlight(thread),
    }
}

fn thread_location_note(thread: &ReviewThread) -> &'static str {
    placement_location_note(thread.anchor.placement)
}

fn thread_placement_state(thread: &ReviewThread) -> &'static str {
    match thread.anchor.placement {
        Some(
            AnchorPlacement::Exact | AnchorPlacement::PerLineHash | AnchorPlacement::MovedExact,
        ) => "inline",
        Some(AnchorPlacement::Context | AnchorPlacement::Window) => "context",
        Some(AnchorPlacement::LineFallback) => "stale",
        Some(AnchorPlacement::FileFallback) => "file",
        Some(AnchorPlacement::Detached) => "detached",
        _ => "inline",
    }
}

fn comment_meta(comment: &ReviewComment) -> String {
    let marker = if comment.comment.author.kind == AuthorKind::Agent {
        COMMENT_AGENT_MARKER
    } else {
        ""
    };
    let mut meta = format!(
        "{}{marker}{COMMENT_META_SEPARATOR}{}",
        comment.comment.author.display_name,
        comment_timestamp(comment.comment.created_at.as_str())
    );
    if let Some(edited_at) = &comment.comment.edited_at {
        meta.push_str(&format!(
            "{COMMENT_META_SEPARATOR}{COMMENT_EDITED_LABEL} {}",
            comment_timestamp(edited_at.as_str())
        ));
    }
    meta
}

fn collapsed_thread_meta(thread: &ReviewThread) -> String {
    let Some(comment) = thread.comments.first() else {
        return THREAD_EMPTY_PREVIEW.to_string();
    };
    let timestamp = comment_timestamp(comment.comment.created_at.as_str());
    let prefix = format!("{timestamp}{COMMENT_META_SEPARATOR}");
    let width = THREAD_CARD_WIDTH
        .saturating_sub(THREAD_CARD_MARGIN.chars().count())
        .saturating_sub(THREAD_FOOTER.chars().count())
        .saturating_sub(1)
        .saturating_sub(prefix.chars().count())
        .max(1);
    format!(
        "{prefix}{}",
        truncate_display_text(first_sentence(&comment.comment.body), width)
    )
}

fn first_sentence(body: &str) -> String {
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return THREAD_EMPTY_PREVIEW.to_string();
    }
    for (index, ch) in body.char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            return body[..=index].trim().to_string();
        }
    }
    body
}

fn formatted_comment_body_lines(body: &str) -> Vec<String> {
    let width = THREAD_CARD_WIDTH
        .saturating_sub(THREAD_CARD_MARGIN.chars().count())
        .saturating_sub(THREAD_BODY_PREFIX.chars().count())
        .max(10) as u32;
    let mut builder = ConfigurationBuilder::new();
    let config = builder
        .line_width(width)
        .text_wrap(TextWrap::Always)
        .emphasis_kind(EmphasisKind::Asterisks)
        .strong_kind(StrongKind::Asterisks)
        .build();
    let formatted =
        format_markdown_text(body, &config, |_, _, _| -> anyhow::Result<Option<String>> {
            Ok(None)
        })
        .ok()
        .flatten()
        .unwrap_or_else(|| body.to_string());
    let formatted = formatted.trim_end_matches(['\r', '\n']);
    let lines: Vec<_> = formatted.lines().map(str::to_string).collect();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn truncate_display_line(line: String) -> String {
    line.chars().take(THREAD_CARD_WIDTH).collect()
}

fn truncate_display_text(text: String, width: usize) -> String {
    if text.chars().count() <= width {
        return text;
    }
    if width <= TRUNCATION_SUFFIX.chars().count() {
        return text.chars().take(width).collect();
    }
    let prefix_width = width - TRUNCATION_SUFFIX.chars().count();
    format!(
        "{}{TRUNCATION_SUFFIX}",
        text.chars().take(prefix_width).collect::<String>()
    )
}

fn comment_timestamp(input: &str) -> String {
    let Ok(created_at) = humantime::parse_rfc3339(input) else {
        return input.to_string();
    };
    relative_timestamp(created_at)
}

fn relative_timestamp(timestamp: SystemTime) -> String {
    match SystemTime::now().duration_since(timestamp) {
        Ok(elapsed) if elapsed < Duration::from_secs(1) => "now".to_string(),
        Ok(elapsed) => format!(
            "{} ago",
            humantime::format_duration(coarse_duration(elapsed))
        ),
        Err(_) => match timestamp.duration_since(SystemTime::now()) {
            Ok(until) if until < Duration::from_secs(1) => "now".to_string(),
            Ok(until) => format!("in {}", humantime::format_duration(coarse_duration(until))),
            Err(_) => "now".to_string(),
        },
    }
}

fn coarse_duration(duration: Duration) -> Duration {
    const MINUTE: u64 = 60;
    let seconds = duration.as_secs();
    if seconds < MINUTE {
        return Duration::from_secs(seconds);
    }
    Duration::from_secs((seconds / MINUTE) * MINUTE)
}

fn code_actions_for_range(rendered: &RenderedReview, range: Range) -> CodeActionResponse {
    let mut actions = Vec::new();
    let start_row = row_at(rendered, range.start.line);

    if let Some(row) = start_row
        && row.kind == ROW_KIND_COMMENT
    {
        actions.extend(comment_row_actions(row));
    }

    if let Some(anchor) = line_comment_anchor(rendered, range) {
        let title = if anchor.start_line == anchor.end_line {
            ACTION_ADD_LINE_COMMENT.to_string()
        } else {
            format!(
                "{ACTION_ADD_RANGE_COMMENT_PREFIX} {}..{}",
                anchor.start_line, anchor.end_line
            )
        };
        actions.push(command_action(
            title,
            COMMAND_ADD_COMMENT,
            Some(vec![line_anchor_arg(&anchor)]),
        ));
    }

    if let Some(path) = file_context_path(start_row) {
        actions.push(command_action(
            ACTION_ADD_FILE_COMMENT.to_string(),
            COMMAND_ADD_COMMENT,
            Some(vec![file_anchor_arg(path)]),
        ));
    }

    actions
}

fn source_code_actions_for_range(
    repo_root: &Path,
    uri: &Uri,
    range: Range,
) -> Option<CodeActionResponse> {
    let path = source_uri_repo_path(repo_root, uri)?;
    let start_line = range.start.line.min(normalized_range_end(range)) + 1;
    let end_line = range.start.line.max(normalized_range_end(range)) + 1;
    let anchor = LineCommentAnchor {
        path: path.as_str(),
        side: SIDE_NEW,
        start_line,
        end_line,
    };
    let title = if start_line == end_line {
        ACTION_ADD_LINE_COMMENT.to_string()
    } else {
        format!("{ACTION_ADD_RANGE_COMMENT_PREFIX} {start_line}..{end_line}")
    };
    Some(vec![command_action(
        title,
        COMMAND_ADD_COMMENT,
        Some(vec![line_anchor_arg(&anchor)]),
    )])
}

fn is_review_uri(uri: &Uri) -> bool {
    uri.to_string().starts_with("peers://")
}

fn source_uri_repo_path(repo_root: &Path, uri: &Uri) -> Option<String> {
    if is_review_uri(uri) {
        return None;
    }
    let path = file_uri_path(uri)?;
    let relative = path.strip_prefix(repo_root).ok()?;
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn file_uri_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_string()
        .starts_with("file://")
        .then(|| uri.to_file_path())?
        .map(|path| path.into_owned())
}

fn comment_row_actions(row: &RenderedRow) -> CodeActionResponse {
    let mut actions = Vec::new();
    if let Some(thread_id) = &row.thread_id {
        actions.push(command_action(
            ACTION_REPLY.to_string(),
            COMMAND_REPLY,
            Some(vec![thread_arg(thread_id)]),
        ));
        actions.push(command_action(
            ACTION_RESPOND_TO_THREAD.to_string(),
            COMMAND_RESPOND_TO_THREAD,
            Some(vec![thread_arg(thread_id)]),
        ));
        if row.resolved.unwrap_or(false) {
            actions.push(command_action(
                ACTION_REOPEN_THREAD.to_string(),
                COMMAND_REOPEN_THREAD,
                Some(vec![thread_arg(thread_id)]),
            ));
        } else {
            actions.push(command_action(
                ACTION_RESOLVE_THREAD.to_string(),
                COMMAND_RESOLVE_THREAD,
                Some(vec![thread_arg(thread_id)]),
            ));
        }
        actions.push(command_action(
            ACTION_TOGGLE_THREAD_COLLAPSED.to_string(),
            COMMAND_TOGGLE_THREAD_COLLAPSED,
            Some(vec![thread_arg(thread_id)]),
        ));
        actions.push(command_action(
            ACTION_DELETE_THREAD.to_string(),
            COMMAND_DELETE_THREAD,
            Some(vec![thread_arg(thread_id)]),
        ));
    }
    if row.can_edit.unwrap_or(false) && row.comment_id.is_some() {
        actions.push(command_action(
            ACTION_EDIT_COMMENT.to_string(),
            COMMAND_EDIT_COMMENT,
            Some(vec![comment_arg(row)]),
        ));
        actions.push(command_action(
            ACTION_DELETE_COMMENT.to_string(),
            COMMAND_DELETE_COMMENT,
            Some(vec![comment_arg(row)]),
        ));
    }
    actions
}

fn row_at(rendered: &RenderedReview, line: u32) -> Option<&RenderedRow> {
    rendered.rows.get(line as usize)
}

#[derive(Debug)]
struct LineCommentAnchor<'a> {
    path: &'a str,
    side: &'static str,
    start_line: u32,
    end_line: u32,
}

fn line_comment_anchor(rendered: &RenderedReview, range: Range) -> Option<LineCommentAnchor<'_>> {
    let start = range.start.line.min(range.end.line);
    let end = normalized_range_end(range);
    if end < start {
        return None;
    }
    let mut path = None;
    let mut side = None;
    let mut start_line: Option<u32> = None;
    let mut end_line: Option<u32> = None;

    for row_index in start..=end {
        let row = row_at(rendered, row_index)?;
        if !row_is_source(row) {
            return None;
        }
        let source_line = row.source_line?;
        let row_path = row.path.as_deref()?;
        let row_side = row.side?;

        if let Some(path) = path {
            if path != row_path {
                return None;
            }
        } else {
            path = Some(row_path);
        }

        if let Some(side) = side {
            if side != row_side {
                return None;
            }
        } else {
            side = Some(row_side);
        }

        start_line = Some(start_line.map_or(source_line, |line| line.min(source_line)));
        end_line = Some(end_line.map_or(source_line, |line| line.max(source_line)));
    }

    Some(LineCommentAnchor {
        path: path?,
        side: side?,
        start_line: start_line?,
        end_line: end_line?,
    })
}

fn row_is_source(row: &RenderedRow) -> bool {
    matches!(row.kind, ROW_KIND_CONTEXT | ROW_KIND_ADD | ROW_KIND_DELETE)
}

fn normalized_range_end(range: Range) -> u32 {
    if range.end.character == 0 && range.end.line > range.start.line {
        range.end.line - 1
    } else {
        range.end.line
    }
}

fn file_context_path(row: Option<&RenderedRow>) -> Option<&str> {
    row.and_then(|row| row.path.as_deref())
}

fn command_action(
    title: String,
    command: &str,
    arguments: Option<Vec<LSPAny>>,
) -> CodeActionOrCommand {
    CodeActionOrCommand::Command(Command::new(title, command.to_string(), arguments))
}

fn line_anchor_arg(anchor: &LineCommentAnchor<'_>) -> LSPAny {
    lsp_payload(LineAnchorArg {
        scope: THREAD_SCOPE_LINE,
        path: anchor.path.to_string(),
        side: anchor.side,
        start_line: anchor.start_line,
        end_line: anchor.end_line,
    })
}

fn file_anchor_arg(path: &str) -> LSPAny {
    lsp_payload(FileAnchorArg {
        scope: THREAD_SCOPE_FILE,
        path: path.to_string(),
    })
}

fn thread_arg(thread_id: &str) -> LSPAny {
    lsp_payload(ThreadArg {
        thread_id: thread_id.to_string(),
    })
}

fn comment_arg(row: &RenderedRow) -> LSPAny {
    lsp_payload(CommentArg {
        comment_id: row.comment_id.clone(),
        body: row.comment_body.clone(),
        invalidates_later_activity: row.invalidates_later_activity,
    })
}

#[derive(Debug, Facet)]
struct LineAnchorArg {
    scope: &'static str,
    path: String,
    side: &'static str,
    start_line: u32,
    end_line: u32,
}

#[derive(Debug, Facet)]
struct FileAnchorArg {
    scope: &'static str,
    path: String,
}

#[derive(Debug, Facet)]
struct ThreadArg {
    thread_id: String,
}

#[derive(Debug, Facet)]
struct CommentArg {
    comment_id: Option<String>,
    body: Option<String>,
    invalidates_later_activity: Option<bool>,
}

fn gutter_background_group(kind: &str) -> Option<&'static str> {
    match kind {
        ROW_KIND_ADD => Some(HIGHLIGHT_ADD_GUTTER_BACKGROUND),
        ROW_KIND_DELETE => Some(HIGHLIGHT_DELETE_GUTTER_BACKGROUND),
        _ => None,
    }
}

fn line_background_group(kind: &str) -> Option<&'static str> {
    match kind {
        ROW_KIND_ADD => Some(HIGHLIGHT_ADD_LINE_BACKGROUND),
        ROW_KIND_DELETE => Some(HIGHLIGHT_DELETE_LINE_BACKGROUND),
        _ => None,
    }
}

fn format_line_number(line: Option<u32>) -> String {
    match line {
        Some(line) => format!("{line:>LINE_NUMBER_WIDTH$}"),
        None => " ".repeat(LINE_NUMBER_WIDTH),
    }
}

fn hunk_header(old: Option<LineRange>, new: Option<LineRange>) -> String {
    format!(
        "{HUNK_HEADER_PREFIX} -{} +{} {HUNK_HEADER_PREFIX}",
        format_range(old),
        format_range(new)
    )
}

fn format_range(range: Option<LineRange>) -> String {
    match range {
        Some(range) => {
            let count = line_range_len(range);
            let start = if count == 0 {
                range.start.saturating_sub(1)
            } else {
                range.start
            };
            format!("{start},{count}")
        }
        None => "0,0".to_string(),
    }
}

fn line_range_len(range: LineRange) -> u32 {
    if range.end < range.start {
        return 0;
    }
    range.end - range.start + 1
}

fn hunk_symbol_name(new: Option<LineRange>, old: Option<LineRange>) -> String {
    if let Some(new) = new {
        format!("{LINES_LABEL} {}", display_range(new))
    } else if let Some(old) = old {
        format!("{OLD_LINES_LABEL} {}", display_range(old))
    } else {
        LINES_LABEL.to_string()
    }
}

fn display_range(range: LineRange) -> String {
    if range.start == range.end {
        range.start.to_string()
    } else {
        format!("{}-{}", range.start, range.end)
    }
}

#[allow(deprecated)]
fn document_symbols(rendered: &RenderedReview, uri: String) -> Vec<DocumentSymbol> {
    let mut files = Vec::new();
    for (index, symbol) in rendered.symbols.iter().enumerate() {
        if symbol.parent.is_some() {
            continue;
        }
        files.push(DocumentSymbol {
            name: symbol.name.clone(),
            detail: Some(symbol.detail.to_string()),
            kind: symbol.kind,
            tags: None,
            deprecated: None,
            range: symbol_range(rendered, symbol),
            selection_range: symbol_selection_range(rendered, symbol),
            children: Some(child_symbols(rendered, index)),
        });
    }

    if files.is_empty() {
        vec![DocumentSymbol {
            name: uri,
            detail: Some(SYNTHETIC_BUFFER_SYMBOL_DETAIL.to_string()),
            kind: SymbolKind::MODULE,
            tags: None,
            deprecated: None,
            range: document_range(rendered),
            selection_range: document_range(rendered),
            children: None,
        }]
    } else {
        files
    }
}

#[allow(deprecated)]
fn child_symbols(rendered: &RenderedReview, parent: usize) -> Vec<DocumentSymbol> {
    rendered
        .symbols
        .iter()
        .filter(|symbol| symbol.parent == Some(parent))
        .map(|symbol| DocumentSymbol {
            name: symbol.name.clone(),
            detail: Some(symbol.detail.to_string()),
            kind: symbol.kind,
            tags: None,
            deprecated: None,
            range: symbol_range(rendered, symbol),
            selection_range: symbol_selection_range(rendered, symbol),
            children: None,
        })
        .collect()
}

fn document_range(rendered: &RenderedReview) -> Range {
    let end_line = rendered.lines.len().saturating_sub(1) as u32;
    let end_character = rendered
        .lines
        .last()
        .map(|line| line.len() as u32)
        .unwrap_or_default();
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: end_line,
            character: end_character,
        },
    }
}

fn symbol_range(rendered: &RenderedReview, symbol: &RenderedSymbol) -> Range {
    Range {
        start: Position {
            line: symbol.start_line,
            character: 0,
        },
        end: Position {
            line: symbol.end_line,
            character: line_len(rendered, symbol.end_line),
        },
    }
}

fn symbol_selection_range(rendered: &RenderedReview, symbol: &RenderedSymbol) -> Range {
    Range {
        start: Position {
            line: symbol.start_line,
            character: 0,
        },
        end: Position {
            line: symbol.start_line,
            character: line_len(rendered, symbol.start_line),
        },
    }
}

fn line_len(rendered: &RenderedReview, line: u32) -> u32 {
    rendered
        .lines
        .get(line as usize)
        .map(|line| line.len() as u32)
        .unwrap_or_default()
}

fn source_text(lines: &[String], line: u32) -> Option<&str> {
    lines
        .get(line.saturating_sub(1) as usize)
        .map(String::as_str)
}

fn create_thread_request(params: &LSPAny) -> LspResult<CreateThreadRequest> {
    Ok(CreateThreadRequest {
        scope: required_string_param(params, PARAM_SCOPE)?,
        path: optional_string_param(params, PARAM_PATH)?,
        side: optional_side_param(params, PARAM_SIDE)?,
        start_line: optional_u32_param(params, PARAM_START_LINE)?,
        end_line: optional_u32_param(params, PARAM_END_LINE)?,
        body: required_string_param(params, PARAM_BODY)?,
    })
}

fn thread_body_request(params: &LSPAny) -> LspResult<ThreadBodyRequest> {
    Ok(ThreadBodyRequest {
        thread_id: required_string_param(params, PARAM_THREAD_ID)?,
        body: required_string_param(params, PARAM_BODY)?,
    })
}

fn edit_comment_request(params: &LSPAny) -> LspResult<EditCommentRequest> {
    Ok(EditCommentRequest {
        comment_id: required_string_param(params, PARAM_COMMENT_ID)?,
        body: required_string_param(params, PARAM_BODY)?,
    })
}

fn comment_request(params: &LSPAny) -> LspResult<CommentRequest> {
    Ok(CommentRequest {
        comment_id: required_string_param(params, PARAM_COMMENT_ID)?,
    })
}

fn thread_request(params: &LSPAny) -> LspResult<ThreadRequest> {
    Ok(ThreadRequest {
        thread_id: required_string_param(params, PARAM_THREAD_ID)?,
    })
}

fn ask_agent_request(params: &LSPAny) -> LspResult<AgentInvocationRequest> {
    Ok(AgentInvocationRequest {
        prompt: required_string_param(params, PARAM_PROMPT)?,
    })
}

fn thread_render_context(params: &LSPAny) -> LspResult<ThreadRenderContext> {
    let Some(LSPAny::Object(context)) = object_param(params)?.get(PARAM_CONTEXT) else {
        return Err(LspError::invalid_params(format!(
            "{LSP_MISSING_FIELD} `{PARAM_CONTEXT}`"
        )));
    };
    Ok(ThreadRenderContext {
        scope: required_string_object_param(context, PARAM_SCOPE)?,
        path: optional_string_object_param(context, PARAM_PATH)?,
        line_label: required_string_object_param(context, PARAM_LINE_LABEL)?,
        side: optional_string_object_param(context, PARAM_SIDE)?,
        start_line: optional_u32_object_param(context, PARAM_START_LINE)?,
        end_line: optional_u32_object_param(context, PARAM_END_LINE)?,
        anchor_placement: optional_string_object_param(context, PARAM_ANCHOR_PLACEMENT)?
            .as_deref()
            .map(anchor_placement_from_name)
            .transpose()?,
    })
}

fn required_string_param(params: &LSPAny, field: &str) -> LspResult<String> {
    optional_string_param(params, field)?
        .ok_or_else(|| LspError::invalid_params(format!("{LSP_MISSING_FIELD} `{field}`")))
}

fn optional_string_param(params: &LSPAny, field: &str) -> LspResult<Option<String>> {
    match object_param(params)?.get(field) {
        Some(LSPAny::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_field(field)),
        None => Ok(None),
    }
}

fn optional_u32_param(params: &LSPAny, field: &str) -> LspResult<Option<u32>> {
    optional_u32_object_param(object_param(params)?, field)
}

fn required_string_object_param(object: &LSPObject, field: &str) -> LspResult<String> {
    optional_string_object_param(object, field)?
        .ok_or_else(|| LspError::invalid_params(format!("{LSP_MISSING_FIELD} `{field}`")))
}

fn optional_string_object_param(object: &LSPObject, field: &str) -> LspResult<Option<String>> {
    match object.get(field) {
        Some(LSPAny::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_field(field)),
        None => Ok(None),
    }
}

fn optional_u32_object_param(object: &LSPObject, field: &str) -> LspResult<Option<u32>> {
    match object.get(field) {
        Some(LSPAny::Number(value)) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| invalid_field(field)),
        Some(_) => Err(invalid_field(field)),
        None => Ok(None),
    }
}

fn anchor_placement_from_name(name: &str) -> LspResult<AnchorPlacement> {
    match name {
        "exact" => Ok(AnchorPlacement::Exact),
        "per_line_hash" => Ok(AnchorPlacement::PerLineHash),
        "context" => Ok(AnchorPlacement::Context),
        "moved_exact" => Ok(AnchorPlacement::MovedExact),
        "window" => Ok(AnchorPlacement::Window),
        "line_fallback" => Ok(AnchorPlacement::LineFallback),
        "file_fallback" => Ok(AnchorPlacement::FileFallback),
        "detached" => Ok(AnchorPlacement::Detached),
        _ => Err(invalid_field(PARAM_ANCHOR_PLACEMENT)),
    }
}

fn optional_side_param(params: &LSPAny, field: &str) -> LspResult<Option<FileSide>> {
    match optional_string_param(params, field)?.as_deref() {
        Some(SIDE_NEW) => Ok(Some(FileSide::New)),
        Some(SIDE_OLD) => Ok(Some(FileSide::Old)),
        Some(_) => Err(invalid_field(field)),
        None => Ok(None),
    }
}

fn rendered_side_name(side: &str) -> Option<&'static str> {
    match side {
        SIDE_NEW => Some(SIDE_NEW),
        SIDE_OLD => Some(SIDE_OLD),
        _ => None,
    }
}

fn object_param(params: &LSPAny) -> LspResult<&LSPObject> {
    match params {
        LSPAny::Object(object) => Ok(object),
        _ => Err(LspError::invalid_params(LSP_INVALID_PARAMS)),
    }
}

fn invalid_field(field: &str) -> LspError {
    LspError::invalid_params(format!("{LSP_INVALID_FIELD} `{field}`"))
}

fn review_update_param(kind: String, sequence: u64) -> LSPAny {
    lsp_payload(ReviewUpdateParam { kind, sequence })
}

#[derive(Debug, Facet)]
struct ReviewUpdateParam {
    kind: String,
    sequence: u64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::anchors::{AnchorLinePlacement, AnchorPlacement};
    use crate::comments::{Author, AuthorKind, Comment, CommentId, PeersTimestamp, ThreadId};
    use crate::diff::{FileDiff, FileStatus, ReviewFile};
    use crate::review_provider::{ReviewThreadAnchor, ReviewThreadLinePlacement};

    use super::*;

    fn action_titles(actions: CodeActionResponse) -> Vec<String> {
        actions
            .into_iter()
            .map(|action| match action {
                CodeActionOrCommand::Command(command) => command.title,
                CodeActionOrCommand::CodeAction(action) => action.title,
            })
            .collect()
    }

    #[test]
    fn formats_markdown_comment_body_for_card_width() {
        let body = concat!(
            "This is a **markdown** comment with enough words to need wrapping inside the ",
            "comment card instead of relying on Neovim markdown textwidth behavior.",
            "\n\n",
            "```ts\n",
            "const test = \"1234\"\n",
            "```",
        );
        let lines = formatted_comment_body_lines(body);
        let body_width = THREAD_CARD_WIDTH
            .saturating_sub(THREAD_CARD_MARGIN.chars().count())
            .saturating_sub(THREAD_BODY_PREFIX.chars().count());
        let fence_start = lines
            .iter()
            .position(|line| line == "```ts")
            .expect("formatted markdown should keep the opening code fence");

        assert!(fence_start > 1, "long prose should wrap before the fence");
        assert_eq!(lines[fence_start + 1], "const test = \"1234\"");
        assert_eq!(lines[fence_start + 2], "```");
        assert!(
            lines[..fence_start]
                .iter()
                .filter(|line| !line.is_empty())
                .all(|line| line.chars().count() <= body_width),
            "wrapped prose lines should fit the rendered comment body width"
        );
    }

    #[test]
    fn respond_to_thread_code_action_only_appears_on_comment_rows() {
        let rendered = RenderedReview {
            lines: vec!["comment".to_string(), "source".to_string()],
            rows: vec![
                RenderedRow {
                    thread_id: Some("thr_1".to_string()),
                    ..RenderedRow::meta(ROW_KIND_COMMENT)
                },
                RenderedRow::source(ROW_KIND_CONTEXT, "src/main.rs", SIDE_NEW, 1),
            ],
            highlights: Vec::new(),
            source_decorations: Vec::new(),
            symbols: Vec::new(),
            sidebar: RenderedSidebar::default(),
            sidebar_counts: RenderedSidebarCounts::default(),
        };

        let comment_titles = action_titles(code_actions_for_range(
            &rendered,
            Range {
                start: Position::new(0, 0),
                end: Position::new(0, 0),
            },
        ));
        let source_titles = action_titles(code_actions_for_range(
            &rendered,
            Range {
                start: Position::new(1, 0),
                end: Position::new(1, 0),
            },
        ));

        assert!(comment_titles.contains(&ACTION_RESPOND_TO_THREAD.to_string()));
        assert!(!source_titles.contains(&ACTION_RESPOND_TO_THREAD.to_string()));
    }

    #[test]
    fn source_file_code_actions_create_line_comment_anchor() {
        let repo_root = Path::new("/repo");
        let uri = "file:///repo/src/main.rs".parse::<Uri>().unwrap();
        let actions = source_code_actions_for_range(
            repo_root,
            &uri,
            Range {
                start: Position::new(4, 0),
                end: Position::new(6, 0),
            },
        )
        .expect("repo source file should offer Peers comment actions");

        let CodeActionOrCommand::Command(command) = &actions[0] else {
            panic!("expected source code action command");
        };
        assert_eq!(command.title, "Peers: Add comment on lines 5..6");
        assert_eq!(command.command, COMMAND_ADD_COMMENT);
        let args = command.arguments.as_ref().expect("anchor argument");
        assert!(format!("{:?}", args[0]).contains("src/main.rs"));
    }

    #[test]
    fn renders_unchanged_files_with_open_comments() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Unchanged,
                is_changed: false,
                comment_count: 1,
                added_lines: 0,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::new(),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: Vec::new(),
                },
            )]),
            threads: Vec::new(),
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains("src/main.rs"))
        );
        assert_eq!(rendered.sidebar_counts.files, 1);
        assert_eq!(rendered.sidebar_counts.comments, 0);
        assert!(!rendered.lines.iter().any(|line| line.contains(EMPTY_TITLE)));
    }

    #[test]
    fn hides_resolved_threads_from_default_render() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-2".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 0,
                added_lines: 1,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec!["fn main() {}".to_string()]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 1 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 1 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:1".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_NEW.to_string()),
                    start_line: Some(1),
                    end_line: Some(1),
                    placement: Some(AnchorPlacement::Exact),
                    line_placements: Vec::new(),
                },
                resolved: true,
                resolved_head_oid: Some("head-1".to_string()),
                collapsed: false,
                comments: Vec::new(),
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            !rendered
                .lines
                .iter()
                .any(|line| line.contains(THREAD_STATUS_RESOLVED_ICON))
        );
    }

    #[test]
    fn renders_resolved_threads_from_current_head() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 1,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec!["fn main() {}".to_string()]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 1 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 1 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:1".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_NEW.to_string()),
                    start_line: Some(1),
                    end_line: Some(1),
                    placement: Some(AnchorPlacement::Exact),
                    line_placements: Vec::new(),
                },
                resolved: true,
                resolved_head_oid: Some("head-1".to_string()),
                collapsed: false,
                comments: Vec::new(),
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains(&format!("{THREAD_STATUS_RESOLVED_ICON} src/main.rs:1")))
        );
        assert_eq!(rendered.sidebar_counts.files, 1);
        assert_eq!(rendered.sidebar_counts.comments, 1);
    }

    #[test]
    fn renders_thread_card_once_when_multiple_hunks_cover_anchor() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 0,
                removed_lines: 1,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: Some(vec!["fn main() {}".to_string()]),
                    new: None,
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![
                        crate::diff::DiffHunk {
                            old: Some(crate::diff::LineRange { start: 1, end: 1 }),
                            new: None,
                            sections: vec![crate::diff::DiffSection::Removed {
                                removed: crate::diff::OldRange {
                                    old: crate::diff::LineRange { start: 1, end: 1 },
                                },
                            }],
                        },
                        crate::diff::DiffHunk {
                            old: Some(crate::diff::LineRange { start: 1, end: 1 }),
                            new: None,
                            sections: vec![crate::diff::DiffSection::Removed {
                                removed: crate::diff::OldRange {
                                    old: crate::diff::LineRange { start: 1, end: 1 },
                                },
                            }],
                        },
                    ],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:1".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_OLD.to_string()),
                    start_line: Some(1),
                    end_line: Some(1),
                    placement: Some(AnchorPlacement::Exact),
                    line_placements: Vec::new(),
                },
                resolved: true,
                resolved_head_oid: Some("head-1".to_string()),
                collapsed: false,
                comments: Vec::new(),
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });
        let header = format!("{THREAD_STATUS_RESOLVED_ICON} src/main.rs:1");
        let rendered_count = rendered
            .lines
            .iter()
            .filter(|line| line.contains(&header))
            .count();

        assert_eq!(rendered_count, 1);
    }

    #[test]
    fn renders_stale_thread_with_stale_accent_highlights() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 1,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec!["fn main() {}".to_string()]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 1 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 1 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:1".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_NEW.to_string()),
                    start_line: Some(1),
                    end_line: Some(1),
                    placement: Some(AnchorPlacement::LineFallback),
                    line_placements: vec![ReviewThreadLinePlacement {
                        original_line: Some(1),
                        current_line: Some(1),
                        placement: AnchorLinePlacement::LineFallback,
                    }],
                },
                resolved: false,
                resolved_head_oid: None,
                collapsed: false,
                comments: vec![ReviewComment {
                    comment: Comment {
                        id: CommentId::from_raw("cmt_test"),
                        thread_id: ThreadId::from_raw("thr_test"),
                        author: Author {
                            kind: AuthorKind::Human,
                            display_name: "jonas".to_string(),
                            email: None,
                        },
                        body: "stale context".to_string(),
                        created_at: PeersTimestamp::from_rfc3339_unchecked("2026-05-28T12:00:00Z"),
                        edited_at: None,
                        deleted_at: None,
                    },
                    can_edit: true,
                }],
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(rendered.highlights.iter().any(|highlight| {
            highlight.group == HIGHLIGHT_THREAD_BORDER_STALE
                || highlight.group == HIGHLIGHT_THREAD_RAIL_STALE
        }));
        assert!(
            rendered.rows.iter().any(|row| {
                row.kind == ROW_KIND_COMMENT && row.placement_state == Some("stale")
            })
        );
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains("stale line fallback"))
        );
        assert!(
            rendered
                .highlights
                .iter()
                .any(|highlight| { highlight.group == HIGHLIGHT_THREAD_LOCATION_NOTE })
        );
    }

    #[test]
    fn renders_line_thread_relocated_to_file_fallback() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 1,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec!["fn main() {}".to_string()]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 1 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 1 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:360-406".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_NEW.to_string()),
                    start_line: None,
                    end_line: None,
                    placement: Some(AnchorPlacement::FileFallback),
                    line_placements: Vec::new(),
                },
                resolved: false,
                resolved_head_oid: None,
                collapsed: false,
                comments: vec![ReviewComment {
                    comment: Comment {
                        id: CommentId::from_raw("cmt_test"),
                        thread_id: ThreadId::from_raw("thr_test"),
                        author: Author {
                            kind: AuthorKind::Human,
                            display_name: "jonas".to_string(),
                            email: None,
                        },
                        body: "deleted range".to_string(),
                        created_at: PeersTimestamp::from_rfc3339_unchecked("2026-05-28T12:00:00Z"),
                        edited_at: None,
                        deleted_at: None,
                    },
                    can_edit: true,
                }],
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains("deleted range"))
        );
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains("file-level fallback"))
        );
        assert!(rendered.rows.iter().any(|row| {
            row.kind == ROW_KIND_COMMENT && row.thread_id.as_deref() == Some("thread-1")
        }));
        assert!(
            rendered
                .sidebar
                .comments
                .lines
                .iter()
                .any(|line| line.contains("deleted range"))
        );
        assert!(
            rendered
                .sidebar
                .comments
                .lines
                .iter()
                .any(|line| line.contains("file-level fallback"))
        );
        assert_eq!(rendered.sidebar_counts.comments, 1);
    }

    #[test]
    fn renders_source_attachment_rails_from_per_line_placement() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 4,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec![
                        "let first = load();".to_string(),
                        "let inserted = true;".to_string(),
                        "let second = recompute();".to_string(),
                        "apply(first, second);".to_string(),
                    ]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 4 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 4 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![
                ReviewThread {
                    id: "thread-1".to_string(),
                    scope: THREAD_SCOPE_LINE.to_string(),
                    path: Some("src/main.rs".to_string()),
                    line_label: "src/main.rs:1-4".to_string(),
                    anchor: ReviewThreadAnchor {
                        side: Some(SIDE_NEW.to_string()),
                        start_line: Some(1),
                        end_line: Some(4),
                        placement: Some(AnchorPlacement::Window),
                        line_placements: vec![
                            ReviewThreadLinePlacement {
                                original_line: Some(1),
                                current_line: Some(1),
                                placement: AnchorLinePlacement::Content,
                            },
                            ReviewThreadLinePlacement {
                                original_line: None,
                                current_line: Some(2),
                                placement: AnchorLinePlacement::Gap,
                            },
                            ReviewThreadLinePlacement {
                                original_line: Some(2),
                                current_line: Some(3),
                                placement: AnchorLinePlacement::Changed,
                            },
                            ReviewThreadLinePlacement {
                                original_line: Some(3),
                                current_line: Some(4),
                                placement: AnchorLinePlacement::Content,
                            },
                        ],
                    },
                    resolved: false,
                    resolved_head_oid: None,
                    collapsed: false,
                    comments: vec![ReviewComment {
                        comment: Comment {
                            id: CommentId::from_raw("cmt_test"),
                            thread_id: ThreadId::from_raw("thr_test"),
                            author: Author {
                                kind: AuthorKind::Human,
                                display_name: "jonas".to_string(),
                                email: None,
                            },
                            body: "mixed context".to_string(),
                            created_at: PeersTimestamp::from_rfc3339_unchecked(
                                "2026-05-28T12:00:00Z",
                            ),
                            edited_at: None,
                            deleted_at: None,
                        },
                        can_edit: true,
                    }],
                },
                ReviewThread {
                    id: "thread-2".to_string(),
                    scope: THREAD_SCOPE_LINE.to_string(),
                    path: Some("src/main.rs".to_string()),
                    line_label: "src/main.rs:4".to_string(),
                    anchor: ReviewThreadAnchor {
                        side: Some(SIDE_NEW.to_string()),
                        start_line: Some(4),
                        end_line: Some(4),
                        placement: Some(AnchorPlacement::Exact),
                        line_placements: Vec::new(),
                    },
                    resolved: true,
                    resolved_head_oid: Some("head-1".to_string()),
                    collapsed: false,
                    comments: vec![ReviewComment {
                        comment: Comment {
                            id: CommentId::from_raw("cmt_resolved"),
                            thread_id: ThreadId::from_raw("thr_resolved"),
                            author: Author {
                                kind: AuthorKind::Human,
                                display_name: "jonas".to_string(),
                                email: None,
                            },
                            body: "resolved exact".to_string(),
                            created_at: PeersTimestamp::from_rfc3339_unchecked(
                                "2026-05-28T12:00:00Z",
                            ),
                            edited_at: None,
                            deleted_at: None,
                        },
                        can_edit: true,
                    }],
                },
            ],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert_eq!(
            source_rail_groups(&rendered),
            vec![
                HIGHLIGHT_THREAD_RAIL,
                HIGHLIGHT_THREAD_RAIL_DETACHED,
                HIGHLIGHT_THREAD_RAIL_CONTEXT,
                HIGHLIGHT_THREAD_RAIL,
            ]
        );
        assert!(
            rendered
                .highlights
                .iter()
                .any(|highlight| { highlight.group == HIGHLIGHT_THREAD_BORDER_CONTEXT })
        );
        assert_eq!(
            rendered
                .source_decorations
                .iter()
                .map(|decoration| (decoration.line, decoration.group))
                .collect::<Vec<_>>(),
            vec![
                (1, HIGHLIGHT_THREAD_BORDER),
                (2, HIGHLIGHT_THREAD_BORDER_DETACHED),
                (3, HIGHLIGHT_THREAD_BORDER_CONTEXT),
                (4, HIGHLIGHT_THREAD_BORDER),
                (4, HIGHLIGHT_THREAD_RESOLVED),
            ]
        );
    }

    #[test]
    fn renders_collapsed_threads_as_compact_summary() {
        let rendered = render_review_payload(ReviewProjection {
            review_id: "repo".to_string(),
            target_label: "working tree".to_string(),
            current_head_oid: Some("head-1".to_string()),
            is_branch_review: false,
            files: vec![ReviewFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                status: FileStatus::Modified,
                is_changed: true,
                comment_count: 1,
                added_lines: 1,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                crate::diff::FileContent {
                    old: None,
                    new: Some(vec!["fn main() {}".to_string()]),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/main.rs".to_string(),
                FileDiff {
                    path: "src/main.rs".to_string(),
                    hunks: vec![crate::diff::DiffHunk {
                        old: None,
                        new: Some(crate::diff::LineRange { start: 1, end: 1 }),
                        sections: vec![crate::diff::DiffSection::Added {
                            added: crate::diff::NewRange {
                                new: crate::diff::LineRange { start: 1, end: 1 },
                            },
                        }],
                    }],
                },
            )]),
            threads: vec![ReviewThread {
                id: "thread-1".to_string(),
                scope: THREAD_SCOPE_LINE.to_string(),
                path: Some("src/main.rs".to_string()),
                line_label: "src/main.rs:1".to_string(),
                anchor: ReviewThreadAnchor {
                    side: Some(SIDE_NEW.to_string()),
                    start_line: Some(1),
                    end_line: Some(1),
                    placement: Some(AnchorPlacement::Exact),
                    line_placements: Vec::new(),
                },
                resolved: false,
                resolved_head_oid: None,
                collapsed: true,
                comments: vec![ReviewComment {
                    comment: Comment {
                        id: CommentId::from_raw("cmt_test"),
                        thread_id: ThreadId::from_raw("thr_test"),
                        author: Author {
                            kind: AuthorKind::Human,
                            display_name: "jonas".to_string(),
                            email: None,
                        },
                        body: "hidden while collapsed".to_string(),
                        created_at: PeersTimestamp::from_rfc3339_unchecked("2026-05-28T12:00:00Z"),
                        edited_at: None,
                        deleted_at: None,
                    },
                    can_edit: true,
                }],
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        let comment_rows = rendered
            .rows
            .iter()
            .filter(|row| row.thread_id.as_deref() == Some("thread-1"))
            .count();
        assert_eq!(comment_rows, 2);
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line == "  ╭─ ● src/main.rs:1 [1]")
        );
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains(" · hidden while collapsed"))
        );
        assert!(
            rendered
                .sidebar
                .comments
                .lines
                .iter()
                .any(|line| line == "╭─ ● main.rs:1 [1]")
        );
        assert!(
            rendered
                .sidebar
                .comments
                .lines
                .iter()
                .any(|line| line.contains(" · hidden while"))
        );
        assert!(
            !rendered
                .sidebar
                .comments
                .lines
                .iter()
                .any(|line| line.starts_with("│ hidden"))
        );
    }

    fn source_rail_groups(rendered: &RenderedReview) -> Vec<&'static str> {
        rendered
            .highlights
            .iter()
            .filter(|highlight| {
                highlight.start_col == THREAD_RAIL_START_COL
                    && highlight.end_col == THREAD_RAIL_END_COL
                    && rendered
                        .rows
                        .get(highlight.line as usize)
                        .is_some_and(row_is_source)
            })
            .map(|highlight| highlight.group)
            .collect()
    }
}
