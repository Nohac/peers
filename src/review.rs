use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt, BufReader};

use crate::comments::{
    Author, AuthorKind, ReviewEvent, ReviewState, encode_event, parse_events_from_reader,
    render_agent_context, render_review_markdown, replay_events,
};
use crate::diff::ReviewTarget;

pub struct RepoContext {
    pub root: PathBuf,
    pub author: Author,
}

pub fn discover_repo(author_override: AuthorOverride) -> Result<RepoContext> {
    let repo =
        gix::discover(std::env::current_dir()?).context("failed to discover a Git repository")?;
    let root = repo
        .workdir()
        .map(Path::to_path_buf)
        .or_else(|| repo.path().parent().map(Path::to_path_buf))
        .ok_or_else(|| anyhow!("could not determine repository root"))?;
    let author = author_override.into_author(&repo);

    Ok(RepoContext { root, author })
}

pub struct AuthorOverride {
    pub kind: Option<AuthorKind>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub agent: bool,
}

impl AuthorOverride {
    pub fn into_author(self, repo: &gix::Repository) -> Author {
        let env_kind = std::env::var("PEERS_AUTHOR_KIND").ok();
        let env_name = std::env::var("PEERS_AUTHOR_NAME").ok();
        let env_email = std::env::var("PEERS_AUTHOR_EMAIL").ok();

        let kind = self
            .kind
            .or_else(|| env_kind.as_deref().and_then(parse_author_kind))
            .unwrap_or(if self.agent {
                AuthorKind::Agent
            } else {
                AuthorKind::Human
            });

        let git_author = git_author(repo);
        let display_name = author_display_name(
            &kind,
            self.name.or(env_name),
            git_author
                .as_ref()
                .map(|author| author.display_name.clone()),
        );

        let email = author_email(&kind, self.email.or(env_email), git_author);

        Author {
            kind,
            display_name,
            email,
        }
    }
}

fn author_display_name(
    kind: &AuthorKind,
    configured_name: Option<String>,
    git_name: Option<String>,
) -> String {
    configured_name.unwrap_or_else(|| match kind {
        AuthorKind::Human => git_name.unwrap_or_else(|| Author::fallback_human().display_name),
        AuthorKind::Agent => Author::fallback_agent().display_name,
    })
}

fn author_email(
    kind: &AuthorKind,
    configured_email: Option<String>,
    git_author: Option<Author>,
) -> Option<String> {
    configured_email.or_else(|| match kind {
        AuthorKind::Human => git_author.and_then(|author| author.email),
        AuthorKind::Agent => None,
    })
}

fn parse_author_kind(input: &str) -> Option<AuthorKind> {
    match input {
        "human" => Some(AuthorKind::Human),
        "agent" => Some(AuthorKind::Agent),
        _ => None,
    }
}

fn git_author(repo: &gix::Repository) -> Option<Author> {
    let signature = repo.author()?.ok()?;
    Some(Author {
        kind: AuthorKind::Human,
        display_name: signature.name.to_string(),
        email: Some(signature.email.to_string()),
    })
}

pub struct ReviewPaths {
    pub dir: PathBuf,
    pub events: PathBuf,
    pub review_md: PathBuf,
    pub agent_context: PathBuf,
    pub session: PathBuf,
}

pub fn storage_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".peers")
}

pub fn current_path(repo_root: &Path) -> PathBuf {
    storage_root(repo_root).join("current")
}

pub fn review_paths(repo_root: &Path, review_id: &str) -> ReviewPaths {
    let dir = storage_root(repo_root).join("reviews").join(review_id);
    ReviewPaths {
        events: dir.join("events.jsonl"),
        review_md: dir.join("review.md"),
        agent_context: dir.join("agent-context.md"),
        session: dir.join("session.json"),
        dir,
    }
}

pub async fn create_review(
    repo_root: &Path,
    author: Author,
    target: ReviewTarget,
) -> Result<String> {
    let now = now_rfc3339()?;
    let review_id = new_review_id(&now);
    let paths = review_paths(repo_root, &review_id);
    fs::create_dir_all(&paths.dir).await?;

    append_event_file(
        &paths.events,
        &ReviewEvent::ReviewCreated {
            review_id: review_id.clone(),
            created_at: now,
            author,
            target,
        },
    )
    .await?;
    write_current(repo_root, &review_id).await?;
    regenerate_outputs(repo_root, &review_id).await?;

    Ok(review_id)
}

pub async fn write_current(repo_root: &Path, review_id: &str) -> Result<()> {
    fs::create_dir_all(storage_root(repo_root)).await?;
    fs::write(current_path(repo_root), format!("{review_id}\n")).await?;
    Ok(())
}

pub async fn current_review_id(repo_root: &Path) -> Result<String> {
    let id = fs::read_to_string(current_path(repo_root))
        .await
        .context("no current review exists; create one with `peers review create`")?;
    let id = id.trim();
    if id.is_empty() {
        return Err(anyhow!("current review file is empty"));
    }
    Ok(id.to_string())
}

pub async fn list_reviews(repo_root: &Path) -> Result<Vec<String>> {
    let reviews_dir = storage_root(repo_root).join("reviews");
    let mut ids = Vec::new();
    let Ok(mut entries) = fs::read_dir(reviews_dir).await else {
        return Ok(ids);
    };

    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            ids.push(entry.file_name().to_string_lossy().into_owned());
        }
    }

    ids.sort();
    Ok(ids)
}

pub async fn load_events_file(path: &Path) -> Result<Vec<ReviewEvent>> {
    let file = fs::File::open(path).await?;
    parse_events_from_reader(BufReader::new(file)).await
}

pub async fn load_review_state(repo_root: &Path, review_id: &str) -> Result<ReviewState> {
    let paths = review_paths(repo_root, review_id);
    let events = load_events_file(&paths.events).await?;
    replay_events(&events)
}

pub async fn append_event_file(path: &Path, event: &ReviewEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let line = encode_event(event)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    Ok(())
}

pub async fn append_review_event(
    repo_root: &Path,
    review_id: &str,
    event: &ReviewEvent,
) -> Result<()> {
    let paths = review_paths(repo_root, review_id);
    append_event_file(&paths.events, event).await?;
    regenerate_outputs(repo_root, review_id).await
}

pub async fn regenerate_outputs(repo_root: &Path, review_id: &str) -> Result<()> {
    let paths = review_paths(repo_root, review_id);
    let state = load_review_state(repo_root, review_id).await?;

    let mut review_md = fs::File::create(&paths.review_md).await?;
    render_review_markdown(&state, &mut review_md).await?;
    review_md.flush().await?;

    let mut agent_context = fs::File::create(&paths.agent_context).await?;
    render_agent_context(&state, &mut agent_context).await?;
    agent_context.flush().await?;

    Ok(())
}

pub fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format timestamp")
}

pub fn new_review_id(now: &str) -> String {
    let compact: String = now
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(15)
        .collect();
    format!("rev_{compact}_{}", id_suffix())
}

pub fn new_thread_id() -> String {
    format!("thr_{}", id_suffix())
}

pub fn new_comment_id() -> String {
    format!("cmt_{}", id_suffix())
}

fn id_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}{:x}", std::process::id(), nanos)
}
