use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use crate::agent::AgentLaunchRequest;
use crate::anchors::AnchorPlacement;
use crate::comments::{AuthorKind, CommentThread, PeersEvent, PeersState, ThreadPayload};
use crate::diff::{CommentAnchor, FileSide, ReviewTarget};
use crate::logging;
use crate::realtime::ReviewUpdateBroadcaster;
use crate::review::{
    AuthorOverride, append_peers_event, current_head_oid, discover_repo, load_peers_state,
    load_thread_payload, now_rfc3339, peers_paths, write_thread_payload,
};
use crate::review_provider::{
    CreateThreadRequest, EditCommentRequest, ReviewProvider, ThreadBodyRequest, ThreadRequest,
    thread_visible_in_default_projection,
};

const NEOVIM_LSP_LABEL: &str = "Neovim LSP";
const SESSION_STOP_HINT: &str = "Press Ctrl-C to stop the local Peers session.";
const REVIEW_LABEL: &str = "Review";
const TARGET_LABEL: &str = "Target";
const THREADS_LABEL: &str = "Threads";
const THREAD_LABEL: &str = "Thread";
const AGENT_HELP: &str = r#"Usage: peers agent <COMMAND>
       peers agent -- <COMMAND> [ARGS]...

Commands:
  codex              Start Codex with a Peers app-server session
  attach --addr URL  Attach an existing Codex app-server session

Passthrough:
  -- <COMMAND>       Start a Peers app-server session and launch a templated command
"#;
const ANCHOR_LABEL: &str = "Anchor";
const UPDATED_LABEL: &str = "Updated";
const EDITED_LABEL: &str = "edited";
const NO_THREADS_MESSAGE: &str = "No threads.";
const HUMAN_AUTHOR_KIND_LABEL: &str = "human";
const AGENT_AUTHOR_KIND_LABEL: &str = "agent";
const RESOLVED_THREAD_STATUS: &str = "resolved";
const UNRESOLVED_THREAD_STATUS: &str = "unresolved";
const DEFAULT_CLEAN_GRACE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const PEERS_SKILL_TEXT: &str = r#"Peers is a local Git review tool. It stores repo-scoped comments in `.peers/`
inside the reviewed repository so humans and agents can share the same review state.

Agent workflow:
1. Run `peers thread list --status open --context` to see unresolved relevant review threads with source context.
2. Use each thread anchor to inspect the referenced file and line/range.
3. Make the requested code changes in the working tree.
4. Run the relevant project checks.
5. For a selected thread, run `peers thread show <thread-id> --context 8` before acting; it includes the current anchor status and original evidence when the anchor is not exact.
6. For completed threads, reply and resolve in one command with `peers thread --agent "Codex (GPT-5)" reply <thread-id> --body "Done: ..." --resolve`.
7. If a thread cannot be addressed, reply with the blocker without `--resolve`.

Core commands:
- `peers thread list --status open --context`
- `peers thread list --status open --context 5`
- `peers thread list --all` to include hidden or no-longer-relevant threads
- `peers thread show <thread-id> --context 8`
- `peers thread --human add --path src/foo.rs --side new --lines 42:47 --body "..."`
- `peers thread --agent "Codex (GPT-5)" reply <thread-id> --body "Done: ..."`
- `peers thread --agent "Codex (GPT-5)" reply <thread-id> --body "Done: ..." --resolve`
- `peers thread --agent "Codex (GPT-5)" resolve <thread-id>` for manual resolve-only actions
- `peers clean --dry-run`
- `peers agent codex`
- `peers agent -- <command> [args...]` for a templated external agent command

Notes:
- Prefer the CLI over editing `.peers/events.jsonl` or payload files manually.
- `peers thread list` follows current visibility rules by default.
- Comment ids start with `cmt_`; thread ids start with `thr_`.
- Open means unresolved. Complete means resolved.
- Thread mutations require exactly one explicit author flag: `--human` or `--agent <identity>`.
- Agents should identify themselves with their model, for example `--agent "Codex (GPT-5)"`.
- Root `--agent <identity>` can also be supplied via `PEERS_AGENT`.
- Use `--body-file -` to read a multiline reply body from stdin.
"#;

#[derive(Parser)]
#[command(name = "peers")]
#[command(about = "Local Git review tool")]
pub struct Cli {
    #[arg(long, env = "PEERS_AGENT", value_name = "IDENTITY")]
    agent: Option<String>,
    #[arg(long, env = "PEERS_AUTHOR_NAME")]
    author_name: Option<String>,
    #[arg(long, env = "PEERS_AUTHOR_EMAIL")]
    author_email: Option<String>,
    #[arg(long, hide = true, global = true)]
    nvim_listen: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print an agent workflow overview for using Peers.
    Skill,
    #[command(hide = true)]
    Diff(DiffArgs),
    #[command(hide = true)]
    Review(ReviewArgs),
    #[command(hide = true)]
    Session(SessionArgs),
    #[command(alias = "comment")]
    Thread(ThreadArgs),
    Clean(CleanArgs),
    Agent(AgentArgs),
}

#[derive(Args, Clone)]
struct DiffArgs {
    #[arg(long)]
    cached: bool,
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct ReviewArgs {
    #[arg(long, default_value = "main")]
    base: String,
    #[arg(long, default_value = "HEAD")]
    head: String,
}

#[derive(Args)]
struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Subcommand)]
enum SessionCommand {
    Diff(DiffArgs),
    Review(ReviewArgs),
}

#[derive(Args)]
struct AgentArgs {
    #[arg(long, default_value = "ws")]
    listen: String,
    #[command(subcommand)]
    command: Option<AgentCommand>,
    #[arg(last = true, num_args = 1.., allow_hyphen_values = true)]
    passthrough: Vec<String>,
}

#[derive(Subcommand)]
enum AgentCommand {
    Codex,
    Attach(AgentAttachArgs),
}

#[derive(Args)]
struct AgentAttachArgs {
    #[arg(long)]
    addr: String,
}

#[derive(Args)]
struct ThreadArgs {
    #[command(flatten)]
    author: CommentAuthorArgs,
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Subcommand)]
enum ThreadCommand {
    /// List visible threads.
    List(ListCommentsArgs),
    /// Show one thread with optional source context.
    Show(ShowThreadArgs),
    Add(AddCommentArgs),
    Reply(ReplyCommentArgs),
    Edit(EditCommentArgs),
    Delete(DeleteCommentArgs),
    Resolve(ThreadCommandArgs),
    Reopen(ThreadCommandArgs),
}

#[derive(Args)]
struct ListCommentsArgs {
    #[arg(long, value_enum, default_value = "all")]
    status: CommentListStatus,
    #[arg(long, value_enum, default_value = "repo")]
    scope: CommentListScope,
    /// Include hidden, stale resolved, archived, and pruned threads.
    #[arg(long)]
    all: bool,
    #[arg(
        long,
        alias = "conext",
        num_args = 0..=1,
        default_missing_value = "3",
        value_name = "LINES"
    )]
    context: Option<usize>,
}

#[derive(Args)]
struct ShowThreadArgs {
    thread_id: String,
    #[arg(
        long,
        alias = "conext",
        num_args = 0..=1,
        default_missing_value = "3",
        value_name = "LINES"
    )]
    context: Option<usize>,
    #[arg(long, conflicts_with = "no_evidence")]
    evidence: bool,
    #[arg(long)]
    no_evidence: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum CommentListStatus {
    All,
    Open,
    Complete,
}

#[derive(Clone, Copy, ValueEnum)]
enum CommentListScope {
    View,
    Repo,
    Detached,
}

#[derive(Args)]
struct AddCommentArgs {
    #[arg(long)]
    path: String,
    #[arg(long, value_enum)]
    side: FileSideArg,
    #[arg(long, value_parser = parse_line_range)]
    lines: LineRange,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    body_file: Option<PathBuf>,
}

#[derive(Args)]
struct ReplyCommentArgs {
    thread_id: String,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    body_file: Option<PathBuf>,
    #[arg(long)]
    resolve: bool,
}

#[derive(Args)]
struct EditCommentArgs {
    comment_id: String,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    body_file: Option<PathBuf>,
}

#[derive(Args)]
struct DeleteCommentArgs {
    comment_id: String,
}

#[derive(Args)]
struct ThreadCommandArgs {
    thread_id: String,
}

#[derive(Args)]
struct CommentAuthorArgs {
    #[arg(
        id = "comment_human",
        long = "human",
        global = true,
        conflicts_with = "comment_agent"
    )]
    human: bool,
    #[arg(
        id = "comment_agent",
        long = "agent",
        global = true,
        conflicts_with = "comment_human",
        value_name = "IDENTITY"
    )]
    agent: Option<String>,
}

impl CommentAuthorArgs {
    fn selection(&self) -> Result<CommentAuthorSelection> {
        match (self.human, self.agent.as_deref()) {
            (true, None) => Ok(CommentAuthorSelection::Human),
            (false, Some(identity)) => {
                let identity = identity.trim();
                if identity.is_empty() {
                    return Err(anyhow!("agent identity cannot be empty"));
                }
                Ok(CommentAuthorSelection::Agent(identity.to_string()))
            }
            (false, None) => Err(anyhow!(
                "thread mutations require an explicit author: pass either `--human` or `--agent <identity>`"
            )),
            (true, Some(_)) => Err(anyhow!(
                "thread mutations accept only one author flag: pass either `--human` or `--agent <identity>`"
            )),
        }
    }
}

enum CommentAuthorSelection {
    Human,
    Agent(String),
}

#[derive(Args)]
struct CleanArgs {
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value = "complete")]
    status: CleanStatus,
    #[arg(long)]
    older_than: Option<String>,
    #[arg(long)]
    detached: bool,
    #[arg(long)]
    hidden: bool,
    #[arg(long)]
    no_interactive: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum CleanStatus {
    Complete,
}

#[derive(Clone, ValueEnum)]
enum FileSideArg {
    Old,
    New,
}

impl From<FileSideArg> for FileSide {
    fn from(value: FileSideArg) -> Self {
        match value {
            FileSideArg::Old => Self::Old,
            FileSideArg::New => Self::New,
        }
    }
}

#[derive(Clone)]
struct LineRange {
    start: u32,
    end: u32,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Skill = &cli.command {
        logging::init();
        print!("{PEERS_SKILL_TEXT}");
        return Ok(());
    }

    let repo = discover_repo(AuthorOverride {
        name: cli.author_name,
        email: cli.author_email,
        agent: cli.agent,
    })?;
    logging::init_file(&peers_paths(&repo.root).backend_log);

    let nvim_listen = cli.nvim_listen;

    match cli.command {
        Command::Skill => unreachable!("skill exits before repository discovery"),
        Command::Diff(args) => {
            open_review_session(&repo.root, diff_target(args), repo.author, nvim_listen).await?;
        }
        Command::Review(args) => {
            open_review_session(
                &repo.root,
                ReviewTarget::Branch {
                    base: args.base,
                    head: args.head,
                },
                repo.author,
                nvim_listen,
            )
            .await?;
        }
        Command::Session(args) => match args.command {
            SessionCommand::Diff(args) => {
                open_review_session(&repo.root, diff_target(args), repo.author, nvim_listen)
                    .await?;
            }
            SessionCommand::Review(args) => {
                open_review_session(
                    &repo.root,
                    ReviewTarget::Branch {
                        base: args.base,
                        head: args.head,
                    },
                    repo.author,
                    nvim_listen,
                )
                .await?;
            }
        },
        Command::Thread(args) => handle_thread(args, &repo.root, repo.author).await?,
        Command::Clean(args) => handle_clean(args, &repo.root, repo.author).await?,
        Command::Agent(args) => handle_agent(args, &repo.root).await?,
    }

    Ok(())
}

async fn handle_agent(args: AgentArgs, repo_root: &Path) -> Result<()> {
    if !args.passthrough.is_empty() {
        return crate::agent::launch_agent(
            repo_root,
            AgentLaunchRequest {
                listen: args.listen,
                command: args.passthrough,
            },
        )
        .await;
    }

    match args.command {
        Some(AgentCommand::Codex) => {
            crate::agent::launch_agent(
                repo_root,
                AgentLaunchRequest {
                    listen: args.listen,
                    command: vec!["codex".to_string()],
                },
            )
            .await
        }
        Some(AgentCommand::Attach(args)) => {
            crate::agent::attach_agent(repo_root, &args.addr).await?;
            println!("Attached agent session at {}.", args.addr);
            println!("{}", peers_paths(repo_root).agent_session.display());
            Ok(())
        }
        None => {
            print!("{AGENT_HELP}");
            Ok(())
        }
    }
}

fn diff_target(args: DiffArgs) -> ReviewTarget {
    if args.all {
        ReviewTarget::All
    } else if args.cached {
        ReviewTarget::Cached
    } else {
        ReviewTarget::WorkingTree
    }
}

async fn open_review_session(
    repo_root: &Path,
    target: ReviewTarget,
    author: crate::comments::Author,
    nvim_listen: Option<String>,
) -> Result<()> {
    let server =
        crate::server::LocalServer::bind(repo_root.to_path_buf(), target, author, nvim_listen)
            .await?;
    println!("{NEOVIM_LSP_LABEL}: {}", server.nvim_lsp_url());
    println!("{SESSION_STOP_HINT}");
    server.run_until_shutdown().await
}

async fn handle_thread(
    args: ThreadArgs,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    let ThreadArgs {
        author: comment_author_args,
        command,
    } = args;

    match command {
        ThreadCommand::List(args) => handle_thread_list(args, repo_root, author.clone()).await,
        ThreadCommand::Show(args) => handle_thread_show(args, repo_root, author.clone()).await,
        command => {
            let selection = comment_author_args.selection()?;
            handle_thread_mutation(command, repo_root, comment_author(author, selection)).await
        }
    }
}

fn comment_author(
    author: crate::comments::Author,
    selection: CommentAuthorSelection,
) -> crate::comments::Author {
    match selection {
        CommentAuthorSelection::Human if author.kind == AuthorKind::Human => author,
        CommentAuthorSelection::Human => crate::comments::Author::fallback_human(),
        CommentAuthorSelection::Agent(display_name) => crate::comments::Author {
            kind: AuthorKind::Agent,
            display_name,
            email: None,
        },
    }
}

async fn handle_thread_mutation(
    command: ThreadCommand,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    let provider = ReviewProvider::new(
        repo_root.to_path_buf(),
        ReviewTarget::WorkingTree,
        author,
        ReviewUpdateBroadcaster::new(),
    );
    match command {
        ThreadCommand::List(_) | ThreadCommand::Show(_) => {
            unreachable!("read-only commands are handled before mutations")
        }
        ThreadCommand::Add(args) => {
            let body = read_body(args.body, args.body_file).await?;
            let review = provider
                .create_thread(CreateThreadRequest {
                    scope: "line".to_string(),
                    path: Some(args.path),
                    side: Some(args.side.into()),
                    start_line: Some(args.lines.start),
                    end_line: Some(args.lines.end),
                    body,
                })
                .await?;
            println!(
                "Created thread. Repo now has {} thread(s).",
                review.threads.len()
            );
        }
        ThreadCommand::Reply(args) => {
            let body = read_body(args.body, args.body_file).await?;
            provider
                .reply_to_thread(ThreadBodyRequest {
                    thread_id: args.thread_id.clone(),
                    body,
                })
                .await?;
            if args.resolve {
                provider
                    .resolve_thread(ThreadRequest {
                        thread_id: args.thread_id.clone(),
                    })
                    .await
                    .with_context(|| {
                        format!(
                            "failed to resolve thread `{}` after adding reply",
                            args.thread_id
                        )
                    })?;
                println!("Added reply and resolved thread `{}`.", args.thread_id);
            } else {
                println!("Added reply to thread `{}`.", args.thread_id);
            }
        }
        ThreadCommand::Edit(args) => {
            let body = read_body(args.body, args.body_file).await?;
            provider
                .edit_comment(EditCommentRequest {
                    comment_id: args.comment_id.clone(),
                    body,
                })
                .await?;
            println!("Edited comment `{}`.", args.comment_id);
        }
        ThreadCommand::Delete(args) => {
            provider
                .delete_comment(crate::review_provider::CommentRequest {
                    comment_id: args.comment_id.clone(),
                })
                .await?;
            println!("Deleted comment `{}`.", args.comment_id);
        }
        ThreadCommand::Resolve(args) => {
            provider
                .resolve_thread(ThreadRequest {
                    thread_id: args.thread_id.clone(),
                })
                .await?;
            println!("Resolved thread `{}`.", args.thread_id);
        }
        ThreadCommand::Reopen(args) => {
            provider
                .reopen_thread(ThreadRequest {
                    thread_id: args.thread_id.clone(),
                })
                .await?;
            println!("Reopened thread `{}`.", args.thread_id);
        }
    }

    let state = load_peers_state(repo_root).await?;
    println!(
        "Repo now has {} thread(s), {} unresolved.",
        state.threads.len(),
        state.unresolved_threads().count()
    );

    Ok(())
}

async fn handle_thread_list(
    args: ListCommentsArgs,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    let state = load_peers_state(repo_root).await?;
    let current_head_oid = if args.all {
        None
    } else {
        current_head_oid(repo_root).await?
    };
    print_comment_list(
        &state,
        args.status,
        args.scope,
        repo_root,
        author,
        args.context.map(|lines| CommentListContext { lines }),
        current_head_oid.as_deref(),
        args.all,
    )
    .await?;
    Ok(())
}

async fn handle_thread_show(
    args: ShowThreadArgs,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    let state = load_peers_state(repo_root).await?;
    let thread = state
        .threads
        .values()
        .find(|thread| thread.id.as_str() == args.thread_id.as_str())
        .ok_or_else(|| anyhow!("unknown thread `{}`", args.thread_id))?;

    println!("{REVIEW_LABEL}: repo");
    println!("{TARGET_LABEL}: repo-scoped");
    let projected_anchor = projected_anchor_status(repo_root, author, &args.thread_id).await?;
    print_comment_thread(thread, projected_anchor.location_note);
    if let Some(lines) = args.context {
        print_comment_context(repo_root, thread, CommentListContext { lines }).await?;
    }
    if should_print_original_evidence(args.evidence, args.no_evidence, projected_anchor.placement) {
        let payload = load_thread_payload(repo_root, &thread.id).await?;
        print_original_evidence(&payload);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CommentListContext {
    lines: usize,
}

async fn print_comment_list(
    state: &PeersState,
    status_filter: CommentListStatus,
    _scope: CommentListScope,
    repo_root: &Path,
    author: crate::comments::Author,
    context: Option<CommentListContext>,
    current_head_oid: Option<&str>,
    include_all: bool,
) -> Result<()> {
    let visible_threads: Vec<_> = state
        .threads
        .values()
        .filter(|thread| comment_list_status_matches(thread, &status_filter))
        .filter(|thread| {
            if include_all {
                true
            } else {
                thread.archived_at.is_none()
                    && thread.pruned_at.is_none()
                    && thread_visible_in_default_projection(
                        thread.resolved,
                        thread.resolved_head_oid.as_deref(),
                        current_head_oid,
                    )
            }
        })
        .collect();
    println!("{REVIEW_LABEL}: repo");
    println!("{TARGET_LABEL}: repo-scoped");
    println!(
        "{THREADS_LABEL}: {}, {} unresolved",
        visible_threads.len(),
        visible_threads
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
    );

    if visible_threads.is_empty() {
        println!("{NO_THREADS_MESSAGE}");
        return Ok(());
    }

    for thread in visible_threads {
        let projected_anchor = if include_all || context.is_none() {
            None
        } else {
            Some(projected_anchor_status(repo_root, author.clone(), thread.id.as_str()).await?)
        };
        print_comment_thread(
            thread,
            projected_anchor.and_then(|status| status.location_note),
        );
        if let Some(context) = context {
            print_comment_context(repo_root, thread, context).await?;
        }
    }
    Ok(())
}

fn comment_list_status_matches(thread: &CommentThread, status_filter: &CommentListStatus) -> bool {
    match status_filter {
        CommentListStatus::All => true,
        CommentListStatus::Open => !thread.resolved,
        CommentListStatus::Complete => thread.resolved,
    }
}

fn print_comment_thread(thread: &CommentThread, anchor_status: Option<&'static str>) {
    let status = if thread.resolved {
        RESOLVED_THREAD_STATUS
    } else {
        UNRESOLVED_THREAD_STATUS
    };

    println!();
    println!("[{status}] {}", thread.anchor.label());
    println!("{THREAD_LABEL}: {}", thread.id);
    if let Some(anchor_status) = anchor_status {
        println!("{ANCHOR_LABEL}: {anchor_status}");
    }
    println!("{UPDATED_LABEL}: {}", thread.updated_at);

    for comment in &thread.comments {
        let kind = match &comment.author.kind {
            AuthorKind::Human => HUMAN_AUTHOR_KIND_LABEL,
            AuthorKind::Agent => AGENT_AUTHOR_KIND_LABEL,
        };
        let edited = comment
            .edited_at
            .as_ref()
            .map(|edited_at| format!(" ({EDITED_LABEL} {edited_at})"))
            .unwrap_or_default();
        println!(
            "- {} by {} ({kind}) at {}{edited}:",
            comment.id, comment.author.display_name, comment.created_at
        );
        print_indented_body(&comment.body);
    }
}

#[derive(Clone, Copy)]
struct ProjectedAnchorStatus {
    placement: Option<AnchorPlacement>,
    location_note: Option<&'static str>,
}

async fn projected_anchor_status(
    repo_root: &Path,
    author: crate::comments::Author,
    thread_id: &str,
) -> Result<ProjectedAnchorStatus> {
    let provider = ReviewProvider::new(
        repo_root.to_path_buf(),
        ReviewTarget::WorkingTree,
        author,
        ReviewUpdateBroadcaster::new(),
    );
    let placement = provider.thread_anchor_placement(thread_id).await?;
    Ok(ProjectedAnchorStatus {
        placement,
        location_note: placement.map(AnchorPlacement::location_note),
    })
}

fn should_print_original_evidence(
    force: bool,
    suppress: bool,
    placement: Option<AnchorPlacement>,
) -> bool {
    if suppress {
        return false;
    }
    if force {
        return true;
    }
    !matches!(placement, Some(AnchorPlacement::Exact) | None)
}

fn print_original_evidence(payload: &ThreadPayload) {
    println!("Original evidence:");
    println!("  Created in: {}", payload.provenance.view_kind);
    if let Some(branch) = &payload.provenance.branch {
        println!("  Branch: {branch}");
    }
    if let Some(head_oid) = &payload.provenance.head_oid {
        println!("  Head: {head_oid}");
    }
    if let Some(merge_base_oid) = &payload.provenance.merge_base_oid {
        println!("  Merge base: {merge_base_oid}");
    }

    match &payload.anchor {
        CommentAnchor::Line { line } => print_line_anchor_evidence(line),
        CommentAnchor::File { path } => println!("  File: {path}"),
        CommentAnchor::Review => println!("  Scope: review"),
    }
}

fn print_line_anchor_evidence(line: &crate::diff::LineAnchor) {
    println!("  Path: {}", line.path);
    if let Some(old_path) = &line.old_path {
        println!("  Old path: {old_path}");
    }
    println!("  Side: {}", file_side_label(&line.side));
    if line.start_line == line.end_line {
        println!("  Lines: {}", line.start_line);
    } else {
        println!("  Lines: {}-{}", line.start_line, line.end_line);
    }
    if let Some(hunk_header) = &line.hunk_header {
        println!("  Hunk: {hunk_header}");
    }
    if let Some(view_kind) = &line.view_kind {
        println!("  Anchor view: {view_kind}");
    }
    if let Some(branch) = &line.branch {
        println!("  Anchor branch: {branch}");
    }
    if let Some(head_oid) = &line.head_oid {
        println!("  Anchor head: {head_oid}");
    }
    if let Some(base_oid) = &line.base_oid {
        println!("  Anchor base: {base_oid}");
    }
    if let Some(merge_base_oid) = &line.merge_base_oid {
        println!("  Anchor merge base: {merge_base_oid}");
    }

    if let Some(selected_text) = &line.selected_text {
        print_evidence_text_block("Selected", selected_text);
    }
    print_evidence_lines_block("Before", &line.context_before);
    print_evidence_lines_block("After", &line.context_after);
}

fn file_side_label(side: &FileSide) -> &'static str {
    match side {
        FileSide::Old => "old",
        FileSide::New => "new",
    }
}

fn print_evidence_text_block(label: &str, text: &str) {
    println!("  {label}:");
    if text.is_empty() {
        println!("    ");
        return;
    }
    for line in text.lines() {
        println!("    {line}");
    }
}

fn print_evidence_lines_block(label: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    println!("  {label}:");
    for line in lines {
        println!("    {line}");
    }
}

async fn print_comment_context(
    repo_root: &Path,
    thread: &CommentThread,
    context: CommentListContext,
) -> Result<()> {
    let CommentAnchor::Line { line } = &thread.anchor else {
        return Ok(());
    };

    let path = match line.side {
        FileSide::Old => line.old_path.as_ref().unwrap_or(&line.path),
        FileSide::New => &line.path,
    };
    let full_path = repo_root.join(path);
    let source = match tokio::fs::read_to_string(&full_path).await {
        Ok(source) => source,
        Err(error) => {
            println!("Context: unavailable for `{path}` ({error})");
            return Ok(());
        }
    };
    let source_lines: Vec<_> = source.lines().collect();
    if source_lines.is_empty() {
        println!("Context: `{path}` is empty");
        return Ok(());
    }

    let anchor_start = line.start_line.max(1) as usize;
    let anchor_end = line.end_line.max(line.start_line).max(1) as usize;
    if anchor_start > source_lines.len() {
        println!(
            "Context: `{path}` lines {anchor_start}-{anchor_end} are outside the current file ({} lines)",
            source_lines.len()
        );
        return Ok(());
    }
    let first = anchor_start.saturating_sub(context.lines).max(1);
    let last = anchor_end
        .saturating_add(context.lines)
        .min(source_lines.len());
    let width = last.to_string().len().max(anchor_end.to_string().len());
    println!("Context: `{path}` lines {first}-{last}");
    for number in first..=last {
        let marker = if number >= anchor_start && number <= anchor_end {
            ">"
        } else {
            " "
        };
        let text = source_lines.get(number - 1).copied().unwrap_or_default();
        println!("{marker} {number:>width$} | {text}");
    }
    Ok(())
}

async fn handle_clean(
    args: CleanArgs,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    validate_clean_args(&args)?;
    let clean_cutoff = clean_cutoff(args.older_than.as_deref())?;
    let state = load_peers_state(repo_root).await?;
    let candidates = clean_candidates(&state, clean_cutoff)?;

    if candidates.is_empty() {
        println!("No clean candidates.");
        return Ok(());
    }

    for thread in &candidates {
        println!(
            "{}: resolved thread hidden from default open lists",
            thread.id
        );
    }

    if args.dry_run {
        println!("Dry run: no changes written.");
        return Ok(());
    }
    if !args.no_interactive && !confirm_clean(candidates.len()).await? {
        println!("Clean cancelled.");
        return Ok(());
    }

    for thread in candidates {
        let mut payload = load_thread_payload(repo_root, &thread.id).await?;
        let archived_at = now_rfc3339()?;
        payload.archived_at = Some(archived_at.clone());
        payload.updated_at = archived_at.clone();
        write_thread_payload(repo_root, &payload).await?;
        append_peers_event(
            repo_root,
            &PeersEvent::ThreadArchived {
                thread_id: thread.id.clone(),
                archived_at,
                author: author.clone(),
                reason: Some("peers clean".to_string()),
            },
            None,
        )
        .await?;
    }

    println!("Archived clean candidates.");
    Ok(())
}

fn validate_clean_args(args: &CleanArgs) -> Result<()> {
    match args.status {
        CleanStatus::Complete => {}
    }
    if args.detached {
        return Err(anyhow!(
            "`peers clean --detached` is specified but detached cleanup candidates are not implemented yet"
        ));
    }
    if args.hidden {
        return Err(anyhow!(
            "`peers clean --hidden` is specified but projection-hidden cleanup candidates are not implemented yet"
        ));
    }
    Ok(())
}

fn clean_cutoff(older_than: Option<&str>) -> Result<SystemTime> {
    let age = older_than
        .map(parse_clean_age)
        .transpose()?
        .unwrap_or(DEFAULT_CLEAN_GRACE);
    SystemTime::now()
        .checked_sub(age)
        .ok_or_else(|| anyhow!("clean age is too large"))
}

fn parse_clean_age(input: &str) -> Result<Duration> {
    let age =
        humantime::parse_duration(input).with_context(|| format!("invalid clean age `{input}`"))?;
    if age.is_zero() {
        return Err(anyhow!("clean age must be greater than zero"));
    }
    Ok(age)
}

fn clean_candidates(state: &PeersState, older_than: SystemTime) -> Result<Vec<&CommentThread>> {
    state
        .threads
        .values()
        .filter(|thread| thread.resolved)
        .filter(|thread| thread.archived_at.is_none() && thread.pruned_at.is_none())
        .filter_map(|thread| match thread_updated_at(thread) {
            Ok(updated_at) if updated_at < older_than => Some(Ok(thread)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn thread_updated_at(thread: &CommentThread) -> Result<SystemTime> {
    Ok(humantime::parse_rfc3339(thread.updated_at.as_str())
        .with_context(|| format!("invalid thread updated timestamp `{}`", thread.updated_at))?)
}

async fn confirm_clean(candidate_count: usize) -> Result<bool> {
    println!("Archive {candidate_count} thread(s)? Type `yes` to continue:");
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    reader.read_line(&mut line).await?;
    Ok(line.trim() == "yes")
}

fn print_indented_body(body: &str) {
    if body.is_empty() {
        println!("  ");
        return;
    }

    for line in body.lines() {
        println!("  {line}");
    }
}

async fn read_body(body: Option<String>, body_file: Option<PathBuf>) -> Result<String> {
    match (body, body_file) {
        (Some(body), None) => Ok(body),
        (None, Some(path)) if path == PathBuf::from("-") => {
            let mut body = String::new();
            tokio::io::stdin().read_to_string(&mut body).await?;
            Ok(body.trim_end().to_string())
        }
        (None, Some(path)) => Ok(tokio::fs::read_to_string(path).await?),
        (None, None) => Err(anyhow!("provide `--body` or `--body-file`")),
        (Some(_), Some(_)) => Err(anyhow!("provide only one of `--body` or `--body-file`")),
    }
}

fn parse_line_range(input: &str) -> Result<LineRange, String> {
    let (start, end) = match input.split_once(':') {
        Some((start, end)) => (start, end),
        None => (input, input),
    };

    let start = start
        .parse::<u32>()
        .map_err(|_| format!("invalid start line `{start}`"))?;
    let end = end
        .parse::<u32>()
        .map_err(|_| format!("invalid end line `{end}`"))?;

    if start == 0 || end == 0 {
        return Err("line numbers must be 1-based".to_string());
    }
    if start > end {
        return Err("line range start must be before or equal to end".to_string());
    }

    Ok(LineRange { start, end })
}
