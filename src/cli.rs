use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::AsyncReadExt;

use crate::comments::{AuthorKind, CommentThread, ReviewEvent, ReviewState, hash_text};
use crate::diff::{FileSide, LineAnchor, ReviewTarget};
use crate::review::{
    AuthorOverride, append_review_event, create_review, current_or_create_fresh_review_id,
    current_or_create_review_id, current_review_id, discover_repo, list_reviews, load_review_state,
    new_comment_id, new_thread_id, now_rfc3339, review_paths,
};

const VOX_RPC_LABEL: &str = "Vox RPC";
const NEOVIM_LSP_LABEL: &str = "Neovim LSP";
const REVIEW_UI_LABEL: &str = "Review UI";
const FRONTEND_DEV_HINT: &str =
    "Run `cd frontend && bun run dev` in another terminal, then open the Review UI URL.";
const SESSION_STOP_HINT: &str = "Press Ctrl-C to stop the local Peers session.";
const CURRENT_REVIEW_ALIAS: &str = "current";
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
const PEERS_SKILL_TEXT: &str = r#"Peers is a local Git review tool. It stores review comments in `.peers/`
inside the reviewed repository so humans and agents can share the same review state.

Agent workflow:
1. Run `peers comment list --status open` to see unresolved review comments.
2. Use each thread anchor to inspect the referenced file and line/range.
3. Make the requested code changes in the working tree.
4. Run the relevant project checks.
5. Reply to each addressed thread with `peers --agent comment reply <thread-id> --body "..."`
6. Resolve completed threads with `peers --agent comment resolve <thread-id>`.
7. If a comment cannot be addressed, reply with the blocker instead of resolving it.

Core commands:
- `peers skill`
  Print this overview.
- `peers review current`
  Print the current review id.
- `peers review list`
  List stored reviews.
- `peers comment list`
  List all visible comments for the current review.
- `peers comment list --status open`
  List unresolved threads only. Use this first when asked to address comments.
- `peers comment list --status complete`
  List resolved threads only.
- `peers comment list current rev_123`
  List comments for multiple reviews. `current` means the current review.
- `peers --agent comment reply <thread-id> --body "Done: ..."`
  Add an agent reply to a thread.
- `peers --agent comment resolve <thread-id>`
  Mark a thread complete.
- `peers --agent comment reopen <thread-id>`
  Reopen a thread if follow-up work is needed.
- `peers --agent comment edit <comment-id> --body "..."`
  Edit one of your comments. Editing can invalidate later dependent activity.
- `peers --agent comment delete <comment-id>`
  Delete one of your comments. Deleting can invalidate later dependent activity.
- `peers agent-context`
  Print the path to the generated agent context file for the current review.

Notes:
- Prefer the CLI over editing `.peers/reviews/<review-id>/events.jsonl` manually.
- Comment ids start with `cmt_`; thread ids start with `thr_`.
- Open means unresolved. Complete means resolved.
- Use `--body-file -` to read a multiline reply body from stdin.
"#;

#[derive(Parser)]
#[command(name = "peers")]
#[command(about = "Local Git review tool")]
pub struct Cli {
    #[arg(long)]
    agent: bool,
    #[arg(long, value_enum)]
    author_kind: Option<AuthorKindArg>,
    #[arg(long)]
    author_name: Option<String>,
    #[arg(long)]
    author_email: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print an agent workflow overview for using Peers.
    Skill,
    Diff(DiffArgs),
    Review(ReviewArgs),
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    AgentContext(AgentContextArgs),
    Nvim(NvimArgs),
}

#[derive(Args)]
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
    #[command(subcommand)]
    command: Option<ReviewCommand>,
}

#[derive(Subcommand)]
enum ReviewCommand {
    Create(CreateReviewArgs),
    List,
    Current,
}

#[derive(Args)]
struct CreateReviewArgs {
    #[arg(long, value_enum)]
    kind: Option<CreateReviewKind>,
    #[arg(long)]
    base: Option<String>,
    #[arg(long, default_value = "HEAD")]
    head: String,
}

#[derive(Clone, ValueEnum)]
enum CreateReviewKind {
    WorkingTree,
    Cached,
    All,
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
    #[arg(value_name = "REVIEW")]
    reviews: Vec<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum CommentListStatus {
    All,
    Open,
    Complete,
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
struct AgentContextArgs {
    #[arg(long)]
    review: Option<String>,
}

#[derive(Args)]
struct NvimArgs {
    #[arg(long)]
    review: Option<String>,
    #[arg(long)]
    nvim_listen: Option<String>,
    #[command(subcommand)]
    command: Option<NvimCommand>,
}

#[derive(Subcommand)]
enum NvimCommand {
    Diff(DiffArgs),
    Review(NvimReviewArgs),
}

#[derive(Args)]
struct NvimReviewArgs {
    #[arg(long, default_value = "main")]
    base: String,
    #[arg(long, default_value = "HEAD")]
    head: String,
}

#[derive(Clone, ValueEnum)]
enum AuthorKindArg {
    Human,
    Agent,
}

impl From<AuthorKindArg> for AuthorKind {
    fn from(value: AuthorKindArg) -> Self {
        match value {
            AuthorKindArg::Human => Self::Human,
            AuthorKindArg::Agent => Self::Agent,
        }
    }
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
        print!("{PEERS_SKILL_TEXT}");
        return Ok(());
    }

    let repo = discover_repo(AuthorOverride {
        kind: cli.author_kind.map(Into::into),
        name: cli.author_name,
        email: cli.author_email,
        agent: cli.agent,
    })?;

    match cli.command {
        Command::Skill => unreachable!("skill exits before repository discovery"),
        Command::Diff(args) => {
            let target = if args.all {
                ReviewTarget::All
            } else if args.cached {
                ReviewTarget::Cached
            } else {
                ReviewTarget::WorkingTree
            };
            let review_id = create_review(&repo.root, repo.author.clone(), target.clone()).await?;
            open_review_session(&repo.root, &review_id, repo.author, None).await?;
        }
        Command::Review(args) => match args.command {
            Some(ReviewCommand::Create(create_args)) => {
                let target = create_review_target(create_args);
                let review_id =
                    create_review(&repo.root, repo.author.clone(), target.clone()).await?;
                println!("Created review `{review_id}` for {}.", target.label());
            }
            Some(ReviewCommand::List) => {
                for review_id in list_reviews(&repo.root).await? {
                    println!("{review_id}");
                }
            }
            Some(ReviewCommand::Current) => {
                let review_id = current_review_id(&repo.root).await?;
                println!("{review_id}");
            }
            None => {
                let target = ReviewTarget::Branch {
                    base: args.base,
                    head: args.head,
                };
                let review_id =
                    create_review(&repo.root, repo.author.clone(), target.clone()).await?;
                open_review_session(&repo.root, &review_id, repo.author, None).await?;
            }
        },
        Command::Comment { command } => handle_comment(command, &repo.root, repo.author).await?,
        Command::AgentContext(args) => {
            let review_id = match args.review {
                Some(review_id) => review_id,
                None => current_review_id(&repo.root).await?,
            };
            let paths = review_paths(&repo.root, &review_id);
            println!("{}", paths.agent_context.display());
        }
        Command::Nvim(args) => {
            let nvim_listen = args.nvim_listen.clone();
            let review_id = nvim_review_id(&repo.root, repo.author.clone(), args).await?;
            open_review_session(&repo.root, &review_id, repo.author, nvim_listen).await?;
        }
    }

    Ok(())
}

async fn nvim_review_id(
    repo_root: &std::path::Path,
    author: crate::comments::Author,
    args: NvimArgs,
) -> Result<String> {
    if let Some(review_id) = args.review {
        return Ok(review_id);
    }

    match args.command {
        Some(NvimCommand::Diff(diff_args)) => {
            current_or_create_review_id(repo_root, author, diff_target(diff_args)).await
        }
        Some(NvimCommand::Review(review_args)) => {
            let target = ReviewTarget::Branch {
                base: review_args.base,
                head: review_args.head,
            };
            create_review(repo_root, author, target).await
        }
        None => match current_or_create_fresh_review_id(repo_root, author.clone()).await {
            Ok(review_id) => Ok(review_id),
            Err(_) => create_review(repo_root, author, ReviewTarget::WorkingTree).await,
        },
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
    repo_root: &std::path::Path,
    review_id: &str,
    author: crate::comments::Author,
    nvim_listen: Option<String>,
) -> Result<()> {
    let server = crate::server::LocalServer::bind(
        repo_root.to_path_buf(),
        review_id.to_string(),
        author,
        nvim_listen,
    )
    .await?;
    println!("{VOX_RPC_LABEL}: {}", server.vox_url());
    println!("{NEOVIM_LSP_LABEL}: {}", server.nvim_lsp_url());
    println!("{REVIEW_UI_LABEL}: {}", server.frontend_url());
    println!("{FRONTEND_DEV_HINT}");
    println!("{SESSION_STOP_HINT}");
    server.run_until_shutdown().await
}

fn create_review_target(args: CreateReviewArgs) -> ReviewTarget {
    match args.kind {
        Some(CreateReviewKind::WorkingTree) => ReviewTarget::WorkingTree,
        Some(CreateReviewKind::Cached) => ReviewTarget::Cached,
        Some(CreateReviewKind::All) => ReviewTarget::All,
        None => ReviewTarget::Branch {
            base: args.base.unwrap_or_else(|| "main".to_string()),
            head: args.head,
        },
    }
}

async fn handle_comment(
    command: CommentCommand,
    repo_root: &std::path::Path,
    author: crate::comments::Author,
) -> Result<()> {
    match command {
        CommentCommand::List(args) => handle_comment_list(args, repo_root).await,
        command => handle_comment_mutation(command, repo_root, author).await,
    }
}

async fn handle_comment_mutation(
    command: CommentCommand,
    repo_root: &std::path::Path,
    author: crate::comments::Author,
) -> Result<()> {
    let review_id = current_review_id(repo_root).await?;
    match command {
        CommentCommand::List(_) => unreachable!("list commands are handled before mutations"),
        CommentCommand::Add(args) => {
            let body = read_body(args.body, args.body_file).await?;
            let now = now_rfc3339()?;
            let thread_id = new_thread_id();
            let comment_id = new_comment_id();
            let selected_text_hash = Some(hash_text(&body));
            let mut anchor = LineAnchor::new(
                args.path,
                args.side.into(),
                args.lines.start,
                args.lines.end,
            );
            anchor.selected_text_hash = selected_text_hash;
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::ThreadCreated {
                    thread_id: thread_id.clone(),
                    comment_id: comment_id.clone(),
                    created_at: now,
                    author,
                    anchor: crate::diff::CommentAnchor::Line { line: anchor },
                    body,
                },
            )
            .await?;
            println!("Created thread `{thread_id}` with comment `{comment_id}`.");
        }
        CommentCommand::Reply(args) => {
            let body = read_body(args.body, args.body_file).await?;
            let comment_id = new_comment_id();
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::CommentAdded {
                    thread_id: args.thread_id.clone(),
                    comment_id: comment_id.clone(),
                    created_at: now_rfc3339()?,
                    author,
                    body,
                },
            )
            .await?;
            println!(
                "Added comment `{comment_id}` to thread `{}`.",
                args.thread_id
            );
        }
        CommentCommand::Edit(args) => {
            let body = read_body(args.body, args.body_file).await?;
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::CommentEdited {
                    comment_id: args.comment_id.clone(),
                    edited_at: now_rfc3339()?,
                    author,
                    body,
                },
            )
            .await?;
            println!("Edited comment `{}`.", args.comment_id);
        }
        CommentCommand::Delete(args) => {
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::CommentDeleted {
                    comment_id: args.comment_id.clone(),
                    deleted_at: now_rfc3339()?,
                    author,
                },
            )
            .await?;
            println!("Deleted comment `{}`.", args.comment_id);
        }
        CommentCommand::Resolve(args) => {
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::ThreadResolved {
                    thread_id: args.thread_id.clone(),
                    resolved_at: now_rfc3339()?,
                    author,
                },
            )
            .await?;
            println!("Resolved thread `{}`.", args.thread_id);
        }
        CommentCommand::Reopen(args) => {
            append_review_event(
                repo_root,
                &review_id,
                &ReviewEvent::ThreadReopened {
                    thread_id: args.thread_id.clone(),
                    reopened_at: now_rfc3339()?,
                    author,
                },
            )
            .await?;
            println!("Reopened thread `{}`.", args.thread_id);
        }
    }

    let state = load_review_state(repo_root, &review_id).await?;
    println!(
        "Review `{review_id}` now has {} thread(s), {} unresolved.",
        state.threads.len(),
        state.unresolved_threads().count()
    );

    Ok(())
}

async fn handle_comment_list(args: ListCommentsArgs, repo_root: &std::path::Path) -> Result<()> {
    let status = args.status;
    let review_ids = resolve_comment_list_reviews(repo_root, args.reviews).await?;

    for (index, review_id) in review_ids.iter().enumerate() {
        if index > 0 {
            println!();
        }

        let state = load_review_state(repo_root, review_id).await?;
        print_comment_list(review_id, &state, status);
    }

    Ok(())
}

async fn resolve_comment_list_reviews(
    repo_root: &std::path::Path,
    reviews: Vec<String>,
) -> Result<Vec<String>> {
    if reviews.is_empty() {
        return Ok(vec![current_review_id(repo_root).await?]);
    }

    let mut review_ids = Vec::with_capacity(reviews.len());
    for review in reviews {
        if review == CURRENT_REVIEW_ALIAS {
            review_ids.push(current_review_id(repo_root).await?);
        } else {
            review_ids.push(review);
        }
    }

    Ok(review_ids)
}

fn print_comment_list(review_id: &str, state: &ReviewState, status_filter: CommentListStatus) {
    println!("{REVIEW_LABEL}: {review_id}");
    if let Some(target) = &state.target {
        println!("{TARGET_LABEL}: {}", target.label());
    }
    println!(
        "{THREADS_LABEL}: {}, {} unresolved",
        state.threads.len(),
        state.unresolved_threads().count()
    );

    let visible_threads: Vec<_> = state
        .threads
        .values()
        .filter(|thread| comment_list_status_matches(thread, &status_filter))
        .collect();

    if visible_threads.is_empty() {
        println!("{NO_COMMENTS_MESSAGE}");
        return;
    }

    for thread in visible_threads {
        print_comment_thread(thread);
    }
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
