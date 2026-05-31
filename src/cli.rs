use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::AsyncReadExt;

use crate::comments::{AuthorKind, ReviewEvent, hash_text};
use crate::diff::{FileSide, LineAnchor, ReviewTarget};
use crate::review::{
    AuthorOverride, append_review_event, create_review, current_or_create_fresh_review_id,
    current_review_id, discover_repo, list_reviews, load_review_state, new_comment_id,
    new_thread_id, now_rfc3339, review_paths,
};

const VOX_RPC_LABEL: &str = "Vox RPC";
const NEOVIM_LSP_LABEL: &str = "Neovim LSP";
const REVIEW_UI_LABEL: &str = "Review UI";
const FRONTEND_DEV_HINT: &str =
    "Run `cd frontend && bun run dev` in another terminal, then open the Review UI URL.";
const SESSION_STOP_HINT: &str = "Press Ctrl-C to stop the local Peers session.";

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
    Add(AddCommentArgs),
    Reply(ReplyCommentArgs),
    Edit(EditCommentArgs),
    Delete(DeleteCommentArgs),
    Resolve(ThreadCommandArgs),
    Reopen(ThreadCommandArgs),
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
    let repo = discover_repo(AuthorOverride {
        kind: cli.author_kind.map(Into::into),
        name: cli.author_name,
        email: cli.author_email,
        agent: cli.agent,
    })?;

    match cli.command {
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
            let review_id = match args.review {
                Some(review_id) => review_id,
                None => current_or_create_fresh_review_id(&repo.root, repo.author.clone()).await?,
            };
            open_review_session(&repo.root, &review_id, repo.author, args.nvim_listen).await?;
        }
    }

    Ok(())
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
    let review_id = current_review_id(repo_root).await?;
    match command {
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
