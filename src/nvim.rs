use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tower_lsp_server::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::comments::AuthorKind;
use crate::diff::{DiffSection, FileSide, LineRange};
use crate::review_provider::ReviewProvider;
use crate::review_provider::{
    CommentRequest, CreateThreadRequest, EditCommentRequest, ReviewComment, ReviewProjection,
    ReviewThread, ThreadBodyRequest, ThreadRequest, thread_visible_in_default_projection,
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
const COMMENT_STATUS_OPEN: &str = "Open";
const COMMENT_STATUS_RESOLVED: &str = "Resolved";
const COMMENT_AGENT_MARKER: &str = " [agent]";
const COMMENT_EDITED_LABEL: &str = "edited";
const COMMENT_META_SEPARATOR: &str = " · ";
const COMMENT_COMMENT_LABEL: &str = "comment";
const COMMENT_COMMENTS_LABEL: &str = "comments";
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
const COMMAND_RESOLVE_THREAD: &str = "peers.resolveThread";
const COMMAND_REOPEN_THREAD: &str = "peers.reopenThread";
const COMMAND_ASK_AGENT: &str = "peers.askAgent";

const ACTION_ADD_LINE_COMMENT: &str = "Peers: Add line comment";
const ACTION_ADD_RANGE_COMMENT_PREFIX: &str = "Peers: Add comment on lines";
const ACTION_ADD_FILE_COMMENT: &str = "Peers: Add comment on file";
const ACTION_REPLY: &str = "Peers: Reply";
const ACTION_EDIT_COMMENT: &str = "Peers: Edit comment";
const ACTION_DELETE_COMMENT: &str = "Peers: Delete comment";
const ACTION_RESOLVE_THREAD: &str = "Peers: Resolve thread";
const ACTION_REOPEN_THREAD: &str = "Peers: Reopen thread";
const NOTIFICATION_REVIEW_UPDATED: &str = "peers/reviewUpdated";
const METHOD_RENDER_REVIEW: &str = "peers/renderReview";
const METHOD_CREATE_THREAD: &str = "peers/createThread";
const METHOD_REPLY_TO_THREAD: &str = "peers/replyToThread";
const METHOD_EDIT_COMMENT: &str = "peers/editComment";
const METHOD_DELETE_COMMENT: &str = "peers/deleteComment";
const METHOD_RESOLVE_THREAD: &str = "peers/resolveThread";
const METHOD_REOPEN_THREAD: &str = "peers/reopenThread";
const PARAM_SCOPE: &str = "scope";
const PARAM_PATH: &str = "path";
const PARAM_SIDE: &str = "side";
const PARAM_START_LINE: &str = "start_line";
const PARAM_END_LINE: &str = "end_line";
const PARAM_BODY: &str = "body";
const PARAM_THREAD_ID: &str = "thread_id";
const PARAM_COMMENT_ID: &str = "comment_id";
const PARAM_INVALIDATES_LATER_ACTIVITY: &str = "invalidates_later_activity";
const LSP_INVALID_PARAMS: &str = "invalid Peers request params";
const LSP_MISSING_FIELD: &str = "missing field";
const LSP_INVALID_FIELD: &str = "invalid field";

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
const HIGHLIGHT_THREAD_HEADER: &str = "PeersDiffThreadHeader";
const HIGHLIGHT_THREAD_META: &str = "PeersDiffThreadMeta";
const HIGHLIGHT_THREAD_RAIL: &str = "PeersDiffThreadRail";
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
const THREAD_HEADER_PREFIX: &str = "│ ╭─ ";
const THREAD_BODY_PREFIX: &str = "│ │ ";
const THREAD_FOOTER: &str = "│ ╰─";
const THREAD_COUNT_SEPARATOR: &str = " ";
const THREAD_BLANK: &str = "│ │";

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

    async fn render_review(&self) -> LspResult<LSPAny> {
        let review = self
            .provider
            .get_review()
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn create_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = create_thread_request(&params)?;
        let review = self
            .provider
            .create_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn reply_to_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_body_request(&params)?;
        let review = self
            .provider
            .reply_to_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn edit_comment(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = edit_comment_request(&params)?;
        let review = self
            .provider
            .edit_comment(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn delete_comment(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = comment_request(&params)?;
        let review = self
            .provider
            .delete_comment(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn resolve_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let review = self
            .provider
            .resolve_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
    }

    async fn reopen_thread(&self, params: LSPAny) -> LspResult<LSPAny> {
        let request = thread_request(&params)?;
        let review = self
            .provider
            .reopen_thread(request)
            .await
            .map_err(|_| LspError::internal_error())?;
        Ok(render_review_payload(review).into_lsp())
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
                        COMMAND_RESOLVE_THREAD.to_string(),
                        COMMAND_REOPEN_THREAD.to_string(),
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
            .custom_method(
                METHOD_RESOLVE_THREAD,
                PeersDiffLanguageServer::resolve_thread,
            )
            .custom_method(METHOD_REOPEN_THREAD, PeersDiffLanguageServer::reopen_thread)
            .finish();
    Server::new(read, write, socket).serve(service).await;
    Ok(())
}

#[derive(Debug)]
struct RenderedReview {
    lines: Vec<String>,
    rows: Vec<RenderedRow>,
    highlights: Vec<RenderedHighlight>,
    symbols: Vec<RenderedSymbol>,
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
        let mut object = LSPObject::new();
        object.insert(
            "lines".to_string(),
            LSPAny::Array(self.lines.into_iter().map(LSPAny::String).collect()),
        );
        object.insert(
            "rows".to_string(),
            LSPAny::Array(self.rows.into_iter().map(RenderedRow::into_lsp).collect()),
        );
        object.insert(
            "highlights".to_string(),
            LSPAny::Array(
                self.highlights
                    .into_iter()
                    .map(RenderedHighlight::into_lsp)
                    .collect(),
            ),
        );
        object.insert("sidebar_counts".to_string(), self.sidebar_counts.into_lsp());
        LSPAny::Object(object)
    }
}

#[derive(Debug, Default)]
struct RenderedSidebarCounts {
    files: u32,
    comments: u32,
}

impl RenderedSidebarCounts {
    fn into_lsp(self) -> LSPAny {
        let mut object = LSPObject::new();
        object.insert("files".to_string(), lsp_number(self.files));
        object.insert("comments".to_string(), lsp_number(self.comments));
        LSPAny::Object(object)
    }
}

#[derive(Debug)]
struct RenderedRow {
    kind: &'static str,
    path: Option<String>,
    file_status: Option<String>,
    added_lines: Option<u32>,
    removed_lines: Option<u32>,
    side: Option<&'static str>,
    source_start_line: Option<u32>,
    source_line: Option<u32>,
    code_start_col: Option<u32>,
    thread_id: Option<String>,
    comment_id: Option<String>,
    comment_body: Option<String>,
    comment_meta: Option<String>,
    can_edit: Option<bool>,
    invalidates_later_activity: Option<bool>,
    resolved: Option<bool>,
    placement_state: Option<&'static str>,
}

impl RenderedRow {
    fn meta(kind: &'static str) -> Self {
        Self {
            kind,
            path: None,
            file_status: None,
            added_lines: None,
            removed_lines: None,
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            placement_state: None,
        }
    }

    fn file_meta(kind: &'static str, path: &str) -> Self {
        Self {
            kind,
            path: Some(path.to_string()),
            file_status: None,
            added_lines: None,
            removed_lines: None,
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            placement_state: Some("file"),
        }
    }

    fn file_header(file: &crate::diff::ReviewFile) -> Self {
        Self {
            kind: ROW_KIND_FILE_HEADER,
            path: Some(file.path.clone()),
            file_status: Some(format!("{:?}", file.status)),
            added_lines: Some(file.added_lines),
            removed_lines: Some(file.removed_lines),
            side: None,
            source_start_line: None,
            source_line: None,
            code_start_col: None,
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            placement_state: Some("file"),
        }
    }

    fn source(kind: &'static str, path: &str, side: &'static str, source_line: u32) -> Self {
        Self {
            kind,
            path: Some(path.to_string()),
            file_status: None,
            added_lines: None,
            removed_lines: None,
            side: Some(side),
            source_start_line: Some(source_line),
            source_line: Some(source_line),
            code_start_col: Some(LINE_PREFIX_WIDTH),
            thread_id: None,
            comment_id: None,
            comment_body: None,
            comment_meta: None,
            can_edit: None,
            invalidates_later_activity: None,
            resolved: None,
            placement_state: Some("inline"),
        }
    }

    fn comment(
        thread: &ReviewThread,
        comment: Option<&ReviewComment>,
        invalidates_later_activity: bool,
    ) -> Self {
        Self {
            kind: ROW_KIND_COMMENT,
            path: thread.path.clone(),
            file_status: None,
            added_lines: None,
            removed_lines: None,
            side: thread.anchor.side.as_deref().and_then(rendered_side_name),
            source_start_line: thread.anchor.start_line,
            source_line: thread.anchor.end_line,
            code_start_col: None,
            thread_id: Some(thread.id.clone()),
            comment_id: comment.map(|comment| comment.comment.id.to_string()),
            comment_body: comment.map(|comment| comment.comment.body.clone()),
            comment_meta: comment.map(comment_meta),
            can_edit: comment.map(|comment| comment.can_edit),
            invalidates_later_activity: comment.map(|_| invalidates_later_activity),
            resolved: Some(thread.resolved),
            placement_state: Some("inline"),
        }
    }

    fn into_lsp(self) -> LSPAny {
        let mut object = LSPObject::new();
        object.insert("kind".to_string(), LSPAny::String(self.kind.to_string()));
        if let Some(path) = self.path {
            object.insert("path".to_string(), LSPAny::String(path));
        }
        if let Some(file_status) = self.file_status {
            object.insert("file_status".to_string(), LSPAny::String(file_status));
        }
        if let Some(added_lines) = self.added_lines {
            object.insert("added_lines".to_string(), lsp_number(added_lines));
        }
        if let Some(removed_lines) = self.removed_lines {
            object.insert("removed_lines".to_string(), lsp_number(removed_lines));
        }
        if let Some(side) = self.side {
            object.insert("side".to_string(), LSPAny::String(side.to_string()));
        }
        if let Some(source_start_line) = self.source_start_line {
            object.insert(
                "source_start_line".to_string(),
                lsp_number(source_start_line),
            );
        }
        if let Some(source_line) = self.source_line {
            object.insert("source_line".to_string(), lsp_number(source_line));
        }
        if let Some(code_start_col) = self.code_start_col {
            object.insert("code_start_col".to_string(), lsp_number(code_start_col));
        }
        if let Some(thread_id) = self.thread_id {
            object.insert("thread_id".to_string(), LSPAny::String(thread_id));
        }
        if let Some(comment_id) = self.comment_id {
            object.insert("comment_id".to_string(), LSPAny::String(comment_id));
        }
        if let Some(comment_body) = self.comment_body {
            object.insert("comment_body".to_string(), LSPAny::String(comment_body));
        }
        if let Some(comment_meta) = self.comment_meta {
            object.insert("comment_meta".to_string(), LSPAny::String(comment_meta));
        }
        if let Some(can_edit) = self.can_edit {
            object.insert("can_edit".to_string(), LSPAny::Bool(can_edit));
        }
        if let Some(invalidates_later_activity) = self.invalidates_later_activity {
            object.insert(
                "invalidates_later_activity".to_string(),
                LSPAny::Bool(invalidates_later_activity),
            );
        }
        if let Some(resolved) = self.resolved {
            object.insert("resolved".to_string(), LSPAny::Bool(resolved));
        }
        if let Some(placement_state) = self.placement_state {
            object.insert(
                "placement_state".to_string(),
                LSPAny::String(placement_state.to_string()),
            );
        }
        LSPAny::Object(object)
    }
}

#[derive(Debug)]
struct RenderedHighlight {
    line: u32,
    start_col: u32,
    end_col: u32,
    group: &'static str,
}

impl RenderedHighlight {
    fn into_lsp(self) -> LSPAny {
        let mut object = LSPObject::new();
        object.insert("line".to_string(), lsp_number(self.line));
        object.insert("start_col".to_string(), lsp_number(self.start_col));
        object.insert("end_col".to_string(), lsp_number(self.end_col));
        object.insert("group".to_string(), LSPAny::String(self.group.to_string()));
        LSPAny::Object(object)
    }
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
        symbols: Vec::new(),
        sidebar_counts: RenderedSidebarCounts::default(),
    };

    if !review.files.iter().any(review_file_is_visible) {
        render_empty_review(&mut rendered);
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
            .filter(|thread| thread.scope == THREAD_SCOPE_FILE)
        {
            push_thread_block(&mut rendered, thread);
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

    rendered
}

fn review_file_is_visible(file: &crate::diff::ReviewFile) -> bool {
    file.is_changed || file.comment_count > 0
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
    if !threads
        .iter()
        .copied()
        .any(|thread| thread_covers_line(thread, path, side, line))
    {
        return;
    }

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
        HIGHLIGHT_THREAD_RAIL,
    );
}

fn push_inline_threads(
    rendered: &mut RenderedReview,
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
        push_thread_block(rendered, thread);
    }
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
    let status = if thread.resolved {
        COMMENT_STATUS_RESOLVED
    } else {
        COMMENT_STATUS_OPEN
    };
    let count = thread.comments.len();
    let count_label = if count == 1 {
        COMMENT_COMMENT_LABEL
    } else {
        COMMENT_COMMENTS_LABEL
    };
    let header = format!(
        "{THREAD_HEADER_PREFIX}{} [{status}] {count}{THREAD_COUNT_SEPARATOR}{count_label}",
        thread.line_label
    );
    push_thread_line(
        rendered,
        header,
        thread,
        None,
        false,
        HIGHLIGHT_THREAD_HEADER,
    );

    for (index, comment) in thread.comments.iter().enumerate() {
        let invalidates_later_activity = index + 1 < thread.comments.len() || thread.resolved;
        let meta = format!("{THREAD_BODY_PREFIX}{}", comment_meta(comment));
        push_thread_line(
            rendered,
            meta,
            thread,
            Some(comment),
            invalidates_later_activity,
            HIGHLIGHT_THREAD_META,
        );

        for line in wrap_comment_body(&comment.comment.body) {
            push_thread_line(
                rendered,
                format!("{THREAD_BODY_PREFIX}{line}"),
                thread,
                Some(comment),
                invalidates_later_activity,
                HIGHLIGHT_THREAD_BODY,
            );
        }
        if index + 1 < thread.comments.len() {
            push_thread_line(
                rendered,
                THREAD_BLANK.to_string(),
                thread,
                None,
                false,
                HIGHLIGHT_THREAD_BODY,
            );
        }
    }

    push_thread_line(
        rendered,
        THREAD_FOOTER.to_string(),
        thread,
        None,
        false,
        HIGHLIGHT_THREAD_BORDER,
    );
}

fn push_thread_line(
    rendered: &mut RenderedReview,
    line_text: String,
    thread: &ReviewThread,
    comment: Option<&ReviewComment>,
    invalidates_later_activity: bool,
    group: &'static str,
) {
    let line = rendered.push_line(
        truncate_display_line(line_text),
        RenderedRow::comment(thread, comment, invalidates_later_activity),
    );
    let end_col = rendered.lines[line as usize].len() as u32;
    rendered.push_highlight(line, 0, end_col, group);
    rendered.push_highlight(
        line,
        THREAD_RAIL_START_COL,
        THREAD_RAIL_END_COL,
        HIGHLIGHT_THREAD_RAIL,
    );
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

fn wrap_comment_body(body: &str) -> Vec<String> {
    let width = THREAD_CARD_WIDTH
        .saturating_sub(THREAD_BODY_PREFIX.chars().count())
        .max(1);
    let mut lines = Vec::new();
    for paragraph in body.lines() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let separator = usize::from(!current.is_empty());
            if !current.is_empty() && current.len() + separator + word.len() > width {
                lines.push(current);
                current = String::new();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_display_line(line: String) -> String {
    line.chars().take(THREAD_CARD_WIDTH).collect()
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

fn comment_row_actions(row: &RenderedRow) -> CodeActionResponse {
    let mut actions = Vec::new();
    if let Some(thread_id) = &row.thread_id {
        actions.push(command_action(
            ACTION_REPLY.to_string(),
            COMMAND_REPLY,
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
    let mut object = LSPObject::new();
    object.insert(
        PARAM_SCOPE.to_string(),
        LSPAny::String(THREAD_SCOPE_LINE.to_string()),
    );
    object.insert(
        PARAM_PATH.to_string(),
        LSPAny::String(anchor.path.to_string()),
    );
    object.insert(
        PARAM_SIDE.to_string(),
        LSPAny::String(anchor.side.to_string()),
    );
    object.insert(PARAM_START_LINE.to_string(), lsp_number(anchor.start_line));
    object.insert(PARAM_END_LINE.to_string(), lsp_number(anchor.end_line));
    LSPAny::Object(object)
}

fn file_anchor_arg(path: &str) -> LSPAny {
    let mut object = LSPObject::new();
    object.insert(
        PARAM_SCOPE.to_string(),
        LSPAny::String(THREAD_SCOPE_FILE.to_string()),
    );
    object.insert(PARAM_PATH.to_string(), LSPAny::String(path.to_string()));
    LSPAny::Object(object)
}

fn thread_arg(thread_id: &str) -> LSPAny {
    let mut object = LSPObject::new();
    object.insert(
        PARAM_THREAD_ID.to_string(),
        LSPAny::String(thread_id.to_string()),
    );
    LSPAny::Object(object)
}

fn comment_arg(row: &RenderedRow) -> LSPAny {
    let mut object = LSPObject::new();
    if let Some(comment_id) = &row.comment_id {
        object.insert(
            PARAM_COMMENT_ID.to_string(),
            LSPAny::String(comment_id.to_string()),
        );
    }
    if let Some(body) = &row.comment_body {
        object.insert(PARAM_BODY.to_string(), LSPAny::String(body.to_string()));
    }
    if let Some(invalidates_later_activity) = row.invalidates_later_activity {
        object.insert(
            PARAM_INVALIDATES_LATER_ACTIVITY.to_string(),
            LSPAny::Bool(invalidates_later_activity),
        );
    }
    LSPAny::Object(object)
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
    match object_param(params)?.get(field) {
        Some(LSPAny::Number(value)) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| invalid_field(field)),
        Some(_) => Err(invalid_field(field)),
        None => Ok(None),
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

fn lsp_number(value: u32) -> LSPAny {
    LSPAny::Number(value.into())
}

fn review_update_param(kind: String, sequence: u64) -> LSPAny {
    let mut object = LSPObject::new();
    object.insert("kind".to_string(), LSPAny::String(kind));
    object.insert("sequence".to_string(), LSPAny::Number(sequence.into()));
    LSPAny::Object(object)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::diff::{FileDiff, FileStatus, ReviewFile};
    use crate::review_provider::ReviewThreadAnchor;

    use super::*;

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
                },
                resolved: true,
                resolved_head_oid: Some("head-1".to_string()),
                comments: Vec::new(),
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            !rendered
                .lines
                .iter()
                .any(|line| line.contains(COMMENT_STATUS_RESOLVED))
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
                },
                resolved: true,
                resolved_head_oid: Some("head-1".to_string()),
                comments: Vec::new(),
            }],
            review_threads: Vec::new(),
            commits: Vec::new(),
        });

        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains(COMMENT_STATUS_RESOLVED))
        );
        assert_eq!(rendered.sidebar_counts.files, 1);
        assert_eq!(rendered.sidebar_counts.comments, 1);
    }
}
