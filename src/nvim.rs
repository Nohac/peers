use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tower_lsp_server::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::diff::{DiffSection, LineRange};
use crate::review_provider::ApiReviewPayload;
use crate::review_provider::ReviewProvider;

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
const METHOD_RENDER_REVIEW: &str = "peers/renderReview";

const ROW_KIND_FILE_HEADER: &str = "file_header";
const ROW_KIND_HUNK_HEADER: &str = "hunk_header";
const ROW_KIND_CONTEXT: &str = "context";
const ROW_KIND_ADD: &str = "add";
const ROW_KIND_DELETE: &str = "delete";
const ROW_KIND_COMMENT: &str = "comment";
const SIDE_NEW: &str = "new";
const SIDE_OLD: &str = "old";
const HIGHLIGHT_FILE_HEADER: &str = "PeersDiffFileHeader";
const HIGHLIGHT_HUNK_HEADER: &str = "PeersDiffHunkHeader";
const HIGHLIGHT_ADD_GUTTER: &str = "PeersDiffAddGutter";
const HIGHLIGHT_DELETE_GUTTER: &str = "PeersDiffDeleteGutter";
const HIGHLIGHT_LINE_NUMBER: &str = "PeersDiffLineNumber";
const HIGHLIGHT_COMMENT: &str = "PeersDiffComment";
const HUNK_HEADER_PREFIX: &str = "@@";
const COMMENT_PREFIX: &str = "      | ";
const FILE_HEADER_PREFIX: &str = "diff -- ";
const LINE_NUMBER_WIDTH: usize = 5;
const LINE_PREFIX_WIDTH: u32 = 14;

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

    async fn render_review(&self) -> LspResult<LSPAny> {
        let review = self
            .provider
            .get_review()
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
        LSPAny::Object(object)
    }
}

#[derive(Debug)]
struct RenderedRow {
    kind: &'static str,
    path: Option<String>,
    side: Option<&'static str>,
    source_line: Option<u32>,
    code_start_col: Option<u32>,
}

impl RenderedRow {
    fn meta(kind: &'static str) -> Self {
        Self {
            kind,
            path: None,
            side: None,
            source_line: None,
            code_start_col: None,
        }
    }

    fn source(kind: &'static str, path: &str, side: &'static str, source_line: u32) -> Self {
        Self {
            kind,
            path: Some(path.to_string()),
            side: Some(side),
            source_line: Some(source_line),
            code_start_col: Some(LINE_PREFIX_WIDTH),
        }
    }

    fn into_lsp(self) -> LSPAny {
        let mut object = LSPObject::new();
        object.insert("kind".to_string(), LSPAny::String(self.kind.to_string()));
        if let Some(path) = self.path {
            object.insert("path".to_string(), LSPAny::String(path));
        }
        if let Some(side) = self.side {
            object.insert("side".to_string(), LSPAny::String(side.to_string()));
        }
        if let Some(source_line) = self.source_line {
            object.insert("source_line".to_string(), lsp_number(source_line));
        }
        if let Some(code_start_col) = self.code_start_col {
            object.insert("code_start_col".to_string(), lsp_number(code_start_col));
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

fn render_review_payload(review: ApiReviewPayload) -> RenderedReview {
    let mut rendered = RenderedReview {
        lines: Vec::new(),
        rows: Vec::new(),
        highlights: Vec::new(),
        symbols: Vec::new(),
    };

    for file in &review.files {
        let file_line = rendered.push_line(
            format!(
                "{FILE_HEADER_PREFIX}{}  {:?}  +{} -{}",
                file.path, file.status, file.added_lines, file.removed_lines
            ),
            RenderedRow::meta(ROW_KIND_FILE_HEADER),
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

        for hunk in &diff.hunks {
            let hunk_text = hunk_header(hunk.old, hunk.new);
            let hunk_line =
                rendered.push_line(hunk_text.clone(), RenderedRow::meta(ROW_KIND_HUNK_HEADER));
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
                            push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_CONTEXT,
                                SIDE_NEW,
                                Some(line),
                                Some(line),
                                " ",
                                text,
                            );
                        }
                    }
                    DiffSection::Added { added } => {
                        for line in added.new.start..=added.new.end {
                            let text = content
                                .and_then(|content| content.new.as_ref())
                                .and_then(|lines| source_text(lines, line))
                                .unwrap_or_default();
                            push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_ADD,
                                SIDE_NEW,
                                None,
                                Some(line),
                                "+",
                                text,
                            );
                        }
                    }
                    DiffSection::Removed { removed } => {
                        for line in removed.old.start..=removed.old.end {
                            let text = content
                                .and_then(|content| content.old.as_ref())
                                .and_then(|lines| source_text(lines, line))
                                .unwrap_or_default();
                            push_source_line(
                                &mut rendered,
                                &file.path,
                                ROW_KIND_DELETE,
                                SIDE_OLD,
                                Some(line),
                                None,
                                "-",
                                text,
                            );
                        }
                    }
                }
            }
        }

        for thread in review
            .threads
            .iter()
            .filter(|thread| thread.path.as_deref() == Some(file.path.as_str()))
        {
            let Some(comment) = thread.comments.first() else {
                continue;
            };
            let state = if thread.resolved {
                "resolved"
            } else {
                "unresolved"
            };
            let comment_line = format!(
                "{COMMENT_PREFIX}[{state}] {}: {}",
                comment.author_name,
                first_line(&comment.body)
            );
            let line =
                rendered.push_line(comment_line.clone(), RenderedRow::meta(ROW_KIND_COMMENT));
            rendered.push_highlight(line, 0, comment_line.len() as u32, HIGHLIGHT_COMMENT);
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

fn push_source_line(
    rendered: &mut RenderedReview,
    path: &str,
    kind: &'static str,
    side: &'static str,
    old_line: Option<u32>,
    new_line: Option<u32>,
    sign: &str,
    text: &str,
) {
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
    rendered.push_highlight(line, 0, 5, HIGHLIGHT_LINE_NUMBER);
    rendered.push_highlight(line, 6, 11, HIGHLIGHT_LINE_NUMBER);
    let gutter_group = match kind {
        ROW_KIND_ADD => HIGHLIGHT_ADD_GUTTER,
        ROW_KIND_DELETE => HIGHLIGHT_DELETE_GUTTER,
        _ => HIGHLIGHT_LINE_NUMBER,
    };
    rendered.push_highlight(line, 12, 13, gutter_group);
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
        Some(range) => format!("{},{}", range.start, range.end - range.start + 1),
        None => "0,0".to_string(),
    }
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

fn first_line(input: &str) -> &str {
    input.lines().next().unwrap_or(input)
}

fn lsp_number(value: u32) -> LSPAny {
    LSPAny::Number(value.into())
}
