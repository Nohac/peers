use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use crate::comments::{AuthorKind, CommentThread, PeersEvent, PeersState};
use crate::diff::{CommentAnchor, FileSide, ReviewTarget};
use crate::logging;
use crate::realtime::ReviewUpdateBroadcaster;
use crate::review::{
    AuthorOverride, append_peers_event, discover_repo, load_peers_state, load_thread_payload,
    now_rfc3339, peers_paths, regenerate_outputs, write_thread_payload,
};
use crate::review_provider::{
    CreateThreadRequest, EditCommentRequest, ReviewProvider, ThreadBodyRequest, ThreadRequest,
};

const VOX_RPC_LABEL: &str = "Vox RPC";
const NEOVIM_LSP_LABEL: &str = "Neovim LSP";
const SESSION_STOP_HINT: &str = "Press Ctrl-C to stop the local Peers session.";
const REVIEW_LABEL: &str = "Review";
const TARGET_LABEL: &str = "Target";
const THREADS_LABEL: &str = "Threads";
const THREAD_LABEL: &str = "Thread";
const UPDATED_LABEL: &str = "Updated";
const EDITED_LABEL: &str = "edited";
const NO_COMMENTS_MESSAGE: &str = "No comments.";
const HUMAN_AUTHOR_KIND_LABEL: &str = "human";
const AGENT_AUTHOR_KIND_LABEL: &str = "agent";
const RESOLVED_THREAD_STATUS: &str = "resolved";
const UNRESOLVED_THREAD_STATUS: &str = "unresolved";
const DEFAULT_CLEAN_GRACE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const PEERS_SKILL_TEXT: &str = r#"Peers is a local Git review tool. It stores repo-scoped comments in `.peers/`
inside the reviewed repository so humans and agents can share the same review state.

Agent workflow:
1. Run `peers comment list --status open --context` to see unresolved review comments with source context.
2. Use each thread anchor to inspect the referenced file and line/range.
3. Make the requested code changes in the working tree.
4. Run the relevant project checks.
5. Reply to each addressed thread with `peers comment --agent "Codex (GPT-5)" reply <thread-id> --body "..."`
6. Resolve completed threads with `peers comment --agent "Codex (GPT-5)" resolve <thread-id>`.
7. If a comment cannot be addressed, reply with the blocker instead of resolving it.

Core commands:
- `peers diff`
- `peers diff --cached`
- `peers diff --all`
- `peers review --base main --head HEAD`
- `peers comment list --status open --context`
- `peers comment list --status open --context 5`
- `peers comment --human add --path src/foo.rs --side new --lines 42:47 --body "..."`
- `peers comment --agent "Codex (GPT-5)" reply <thread-id> --body "Done: ..."`
- `peers comment --agent "Codex (GPT-5)" resolve <thread-id>`
- `peers clean --dry-run`
- `peers agent-context`

Notes:
- Prefer the CLI over editing `.peers/events.jsonl` or payload files manually.
- Comment ids start with `cmt_`; thread ids start with `thr_`.
- Open means unresolved. Complete means resolved.
- Comment mutations require exactly one explicit author flag: `--human` or `--agent <identity>`.
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
    Diff(DiffArgs),
    Review(ReviewArgs),
    Comment(CommentArgs),
    Clean(CleanArgs),
    AgentContext,
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
struct CommentArgs {
    #[command(flatten)]
    author: CommentAuthorArgs,
    #[command(subcommand)]
    command: CommentCommand,
}

#[derive(Subcommand)]
enum CommentCommand {
    /// List visible comment threads.
    List(ListCommentsArgs),
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
    #[arg(
        long,
        alias = "conext",
        num_args = 0..=1,
        default_missing_value = "3",
        value_name = "LINES"
    )]
    context: Option<usize>,
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
                "comment mutations require an explicit author: pass either `--human` or `--agent <identity>`"
            )),
            (true, Some(_)) => Err(anyhow!(
                "comment mutations accept only one author flag: pass either `--human` or `--agent <identity>`"
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
        Command::Comment(args) => handle_comment(args, &repo.root, repo.author).await?,
        Command::Clean(args) => handle_clean(args, &repo.root, repo.author).await?,
        Command::AgentContext => {
            regenerate_outputs(&repo.root, None).await?;
            println!("{}", peers_paths(&repo.root).agent_context.display());
        }
    }

    Ok(())
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
    println!("{VOX_RPC_LABEL}: {}", server.vox_url());
    println!("{NEOVIM_LSP_LABEL}: {}", server.nvim_lsp_url());
    println!("{SESSION_STOP_HINT}");
    server.run_until_shutdown().await
}

async fn handle_comment(
    args: CommentArgs,
    repo_root: &Path,
    author: crate::comments::Author,
) -> Result<()> {
    let CommentArgs {
        author: comment_author_args,
        command,
    } = args;

    match command {
        CommentCommand::List(args) => handle_comment_list(args, repo_root).await,
        command => {
            let selection = comment_author_args.selection()?;
            handle_comment_mutation(command, repo_root, comment_author(author, selection)).await
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

async fn handle_comment_mutation(
    command: CommentCommand,
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
        CommentCommand::List(_) => unreachable!("list commands are handled before mutations"),
        CommentCommand::Add(args) => {
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
        CommentCommand::Reply(args) => {
            let body = read_body(args.body, args.body_file).await?;
            provider
                .reply_to_thread(ThreadBodyRequest {
                    thread_id: args.thread_id.clone(),
                    body,
                })
                .await?;
            println!("Added reply to thread `{}`.", args.thread_id);
        }
        CommentCommand::Edit(args) => {
            let body = read_body(args.body, args.body_file).await?;
            provider
                .edit_comment(EditCommentRequest {
                    comment_id: args.comment_id.clone(),
                    body,
                })
                .await?;
            println!("Edited comment `{}`.", args.comment_id);
        }
        CommentCommand::Delete(args) => {
            provider
                .delete_comment(crate::review_provider::CommentRequest {
                    comment_id: args.comment_id.clone(),
                })
                .await?;
            println!("Deleted comment `{}`.", args.comment_id);
        }
        CommentCommand::Resolve(args) => {
            provider
                .resolve_thread(ThreadRequest {
                    thread_id: args.thread_id.clone(),
                })
                .await?;
            println!("Resolved thread `{}`.", args.thread_id);
        }
        CommentCommand::Reopen(args) => {
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
        "Repo comments now have {} thread(s), {} unresolved.",
        state.threads.len(),
        state.unresolved_threads().count()
    );

    Ok(())
}

async fn handle_comment_list(args: ListCommentsArgs, repo_root: &Path) -> Result<()> {
    let state = load_peers_state(repo_root).await?;
    print_comment_list(
        &state,
        args.status,
        args.scope,
        repo_root,
        args.context.map(|lines| CommentListContext { lines }),
    )
    .await?;
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
    context: Option<CommentListContext>,
) -> Result<()> {
    println!("{REVIEW_LABEL}: repo");
    println!("{TARGET_LABEL}: repo-scoped");
    println!(
        "{THREADS_LABEL}: {}, {} unresolved",
        state.threads.len(),
        state.unresolved_threads().count()
    );

    let visible_threads: Vec<_> = state
        .threads
        .values()
        .filter(|thread| comment_list_status_matches(thread, &status_filter))
        .filter(|thread| thread.pruned_at.is_none() && thread.archived_at.is_none())
        .collect();

    if visible_threads.is_empty() {
        println!("{NO_COMMENTS_MESSAGE}");
        return Ok(());
    }

    for thread in visible_threads {
        print_comment_thread(thread);
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

fn print_comment_thread(thread: &CommentThread) {
    let status = if thread.resolved {
        RESOLVED_THREAD_STATUS
    } else {
        UNRESOLVED_THREAD_STATUS
    };

    println!();
    println!("[{status}] {}", thread.anchor.label());
    println!("{THREAD_LABEL}: {}", thread.id);
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
