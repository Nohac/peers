use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt, BufReader};

use crate::comments::{
    Author, AuthorKind, CommentPayload, PayloadStore, PeersEvent, PeersState, ThreadPayload,
    decode_comment_payload, decode_thread_payload, encode_comment_payload, encode_event,
    encode_thread_payload, parse_events_from_reader, render_agent_context, render_review_markdown,
    replay_events,
};
use crate::diff::ReviewTarget;

const CURRENT_HEAD_ERROR: &str = "failed to inspect current HEAD";
const HEAD_REF: &str = "HEAD";
const THREAD_JSON: &str = "thread.json";
const COMMENTS_DIR: &str = "comments";

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

pub struct PeersPaths {
    pub root: PathBuf,
    pub events: PathBuf,
    pub threads: PathBuf,
    pub review_md: PathBuf,
    pub agent_context: PathBuf,
    pub session: PathBuf,
}

pub fn storage_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".peers")
}

pub fn peers_paths(repo_root: &Path) -> PeersPaths {
    let root = storage_root(repo_root);
    PeersPaths {
        events: root.join("events.jsonl"),
        threads: root.join("threads"),
        review_md: root.join("review.md"),
        agent_context: root.join("agent-context.md"),
        session: root.join("session.json"),
        root,
    }
}

pub async fn ensure_storage(repo_root: &Path) -> Result<()> {
    let paths = peers_paths(repo_root);
    fs::create_dir_all(&paths.threads).await?;
    if !paths.events.exists() {
        append_raw_events_file(&paths.events, "").await?;
    }
    Ok(())
}

pub async fn load_events_file(path: &Path) -> Result<Vec<PeersEvent>> {
    let Ok(file) = fs::File::open(path).await else {
        return Ok(Vec::new());
    };
    parse_events_from_reader(BufReader::new(file)).await
}

pub async fn load_peers_state(repo_root: &Path) -> Result<PeersState> {
    let paths = peers_paths(repo_root);
    let events = load_events_file(&paths.events).await?;
    let payloads = load_payload_store(repo_root).await?;
    replay_events(&events, &payloads)
}

pub async fn load_payload_store(repo_root: &Path) -> Result<PayloadStore> {
    let paths = peers_paths(repo_root);
    let mut payloads = PayloadStore::default();
    let Ok(mut thread_entries) = fs::read_dir(paths.threads).await else {
        return Ok(payloads);
    };

    while let Some(entry) = thread_entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let thread_path = entry.path().join(THREAD_JSON);
        if let Ok(thread) = read_thread_payload_file(&thread_path).await {
            payloads.threads.insert(thread.id.clone(), thread);
        }

        let comments_dir = entry.path().join(COMMENTS_DIR);
        let Ok(mut comment_entries) = fs::read_dir(comments_dir).await else {
            continue;
        };
        while let Some(comment_entry) = comment_entries.next_entry().await? {
            if !comment_entry.file_type().await?.is_file() {
                continue;
            }
            if let Ok(comment) = read_comment_payload_file(&comment_entry.path()).await {
                payloads.comments.insert(comment.id.clone(), comment);
            }
        }
    }

    Ok(payloads)
}

pub async fn write_thread_payload(repo_root: &Path, payload: &ThreadPayload) -> Result<()> {
    write_thread_payload_file(&thread_payload_path(repo_root, &payload.id), payload).await
}

pub async fn write_comment_payload(repo_root: &Path, payload: &CommentPayload) -> Result<()> {
    write_comment_payload_file(
        &comment_payload_path(repo_root, &payload.thread_id, &payload.id),
        payload,
    )
    .await
}

pub async fn load_thread_payload(repo_root: &Path, thread_id: &str) -> Result<ThreadPayload> {
    read_thread_payload_file(&thread_payload_path(repo_root, thread_id)).await
}

pub async fn load_comment_payload(
    repo_root: &Path,
    thread_id: &str,
    comment_id: &str,
) -> Result<CommentPayload> {
    read_comment_payload_file(&comment_payload_path(repo_root, thread_id, comment_id)).await
}

async fn read_thread_payload_file(path: &Path) -> Result<ThreadPayload> {
    let input = fs::read_to_string(path).await?;
    decode_thread_payload(&input)
}

async fn read_comment_payload_file(path: &Path) -> Result<CommentPayload> {
    let input = fs::read_to_string(path).await?;
    decode_comment_payload(&input)
}

async fn write_thread_payload_file(path: &Path, payload: &ThreadPayload) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let json = encode_thread_payload(payload)?;
    fs::write(path, format!("{json}\n")).await?;
    Ok(())
}

async fn write_comment_payload_file(path: &Path, payload: &CommentPayload) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let json = encode_comment_payload(payload)?;
    fs::write(path, format!("{json}\n")).await?;
    Ok(())
}

fn thread_payload_path(repo_root: &Path, thread_id: &str) -> PathBuf {
    peers_paths(repo_root)
        .threads
        .join(thread_id)
        .join(THREAD_JSON)
}

fn comment_payload_path(repo_root: &Path, thread_id: &str, comment_id: &str) -> PathBuf {
    peers_paths(repo_root)
        .threads
        .join(thread_id)
        .join(COMMENTS_DIR)
        .join(format!("{comment_id}.json"))
}

pub async fn append_event_file(path: &Path, event: &PeersEvent) -> Result<()> {
    let line = encode_event(event)?;
    append_raw_events_file(path, &format!("{line}\n")).await
}

async fn append_raw_events_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(contents.as_bytes()).await?;
    file.flush().await?;

    Ok(())
}

pub async fn append_peers_event(
    repo_root: &Path,
    event: &PeersEvent,
    target: Option<&ReviewTarget>,
) -> Result<()> {
    let paths = peers_paths(repo_root);
    append_event_file(&paths.events, event).await?;
    regenerate_outputs(repo_root, target).await
}

pub async fn regenerate_outputs(repo_root: &Path, target: Option<&ReviewTarget>) -> Result<()> {
    let paths = peers_paths(repo_root);
    let state = load_peers_state(repo_root).await?;

    let mut review_md = fs::File::create(&paths.review_md).await?;
    render_review_markdown(&state, target, &mut review_md).await?;
    review_md.flush().await?;

    let mut agent_context = fs::File::create(&paths.agent_context).await?;
    render_agent_context(&state, target, &mut agent_context).await?;
    agent_context.flush().await?;

    Ok(())
}

pub async fn current_head_oid(repo_root: &Path) -> Result<Option<String>> {
    let repo_root = repo_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = gix::discover(repo_root).context(CURRENT_HEAD_ERROR)?;
        match repo.rev_parse_single(HEAD_REF) {
            Ok(id) => Ok(Some(id.detach().to_string())),
            Err(_) => Ok(None),
        }
    })
    .await
    .context(CURRENT_HEAD_ERROR)?
}

pub fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format timestamp")
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::comments::{CreationProvenance, ThreadStatus};
    use crate::diff::{CommentAnchor, FileSide, LineAnchor};

    const TEST_AUTHOR_NAME: &str = "Jonas";
    const TEST_AUTHOR_EMAIL: &str = "jonas@example.com";

    #[tokio::test]
    async fn repo_scoped_state_loads_thread_and_comment_payloads() {
        let root = test_root("repo_state");
        fs::create_dir_all(&root).unwrap();

        let thread = ThreadPayload {
            id: "thr_test".to_string(),
            status: ThreadStatus::Open,
            anchor: CommentAnchor::Line {
                line: LineAnchor::new("src/main.rs".to_string(), FileSide::New, 1, 1),
            },
            created_at: "2026-05-28T12:01:00Z".to_string(),
            updated_at: "2026-05-28T12:01:00Z".to_string(),
            provenance: CreationProvenance::from_target(&ReviewTarget::WorkingTree),
            archived_at: None,
            pruned_at: None,
        };
        let comment = CommentPayload {
            id: "cmt_test".to_string(),
            thread_id: thread.id.clone(),
            author: author(),
            body: "body".to_string(),
            created_at: thread.created_at.clone(),
            edited_at: None,
            deleted_at: None,
        };
        write_thread_payload(&root, &thread).await.unwrap();
        write_comment_payload(&root, &comment).await.unwrap();
        append_event_file(
            &peers_paths(&root).events,
            &PeersEvent::ThreadCreated {
                thread_id: thread.id.clone(),
                comment_id: comment.id.clone(),
                created_at: thread.created_at.clone(),
                author: author(),
            },
        )
        .await
        .unwrap();

        let state = load_peers_state(&root).await.unwrap();
        assert_eq!(state.threads["thr_test"].comments[0].body, "body");

        let _ = fs::remove_dir_all(root);
    }

    fn author() -> Author {
        Author {
            kind: AuthorKind::Human,
            display_name: TEST_AUTHOR_NAME.to_string(),
            email: Some(TEST_AUTHOR_EMAIL.to_string()),
        }
    }

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("peers_review_{name}_{nonce}"))
    }
}
