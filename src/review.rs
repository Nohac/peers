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

const CURRENT_REVIEW_STALE_CONTEXT: &str = "failed to inspect current review freshness";
const CURRENT_HEAD_ERROR: &str = "failed to inspect current HEAD";
const HEAD_REF: &str = "HEAD";

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
            target: target.clone(),
        },
    )
    .await?;
    if target.is_local_diff() {
        append_event_file(
            &paths.events,
            &ReviewEvent::ReviewDiffBaseCaptured {
                review_id: review_id.clone(),
                captured_at: now_rfc3339()?,
                base_oid: current_head_oid(repo_root).await?,
            },
        )
        .await?;
    }
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

pub async fn current_or_create_fresh_review_id(repo_root: &Path, author: Author) -> Result<String> {
    let review_id = current_review_id(repo_root).await?;
    let state = load_review_state(repo_root, &review_id).await?;
    if !review_needs_fresh_successor(repo_root, &state)
        .await
        .context(CURRENT_REVIEW_STALE_CONTEXT)?
    {
        return Ok(review_id);
    }

    let target = state
        .target
        .ok_or_else(|| anyhow!("current review has no target"))?;
    create_review(repo_root, author, target).await
}

pub async fn current_or_create_review_id(
    repo_root: &Path,
    author: Author,
    target: ReviewTarget,
) -> Result<String> {
    let Ok(review_id) = current_review_id(repo_root).await else {
        return create_review(repo_root, author, target).await;
    };
    let Ok(state) = load_review_state(repo_root, &review_id).await else {
        return create_review(repo_root, author, target).await;
    };
    if state.target.as_ref() != Some(&target) {
        return create_review(repo_root, author, target).await;
    }
    if review_needs_fresh_successor(repo_root, &state)
        .await
        .context(CURRENT_REVIEW_STALE_CONTEXT)?
    {
        return create_review(repo_root, author, target).await;
    }
    Ok(review_id)
}

pub async fn review_needs_fresh_successor(repo_root: &Path, state: &ReviewState) -> Result<bool> {
    let Some(target) = &state.target else {
        return Ok(false);
    };
    if !target.is_local_diff() {
        return Ok(false);
    }
    let Some(captured_base) = &state.diff_base_oid else {
        return Ok(false);
    };
    Ok(*captured_base != current_head_oid(repo_root).await?)
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const TEST_AUTHOR_NAME: &str = "Jonas";
    const TEST_AUTHOR_EMAIL: &str = "jonas@example.com";
    const TEST_FILE: &str = "file.txt";
    const TEST_INITIAL_BODY: &str = "initial\n";
    const TEST_NEXT_BODY: &str = "next\n";
    const TEST_INITIAL_COMMIT: &str = "initial";
    const TEST_NEXT_COMMIT: &str = "next";
    const GIT_USER_NAME_FIELD: &str = "name";
    const GIT_USER_EMAIL_FIELD: &str = "email";

    #[tokio::test]
    async fn local_diff_current_review_rotates_when_head_changes() {
        let root = test_root("head_change");
        fs::create_dir_all(&root).unwrap();
        init_repo(&root);
        fs::write(root.join(TEST_FILE), TEST_INITIAL_BODY).unwrap();
        commit_file(&root, TEST_INITIAL_COMMIT, TEST_INITIAL_BODY);

        let review_id = create_review(&root, author(), ReviewTarget::WorkingTree)
            .await
            .unwrap();
        let state = load_review_state(&root, &review_id).await.unwrap();
        assert!(!review_needs_fresh_successor(&root, &state).await.unwrap());

        fs::write(root.join(TEST_FILE), TEST_NEXT_BODY).unwrap();
        commit_file(&root, TEST_NEXT_COMMIT, TEST_NEXT_BODY);

        let state = load_review_state(&root, &review_id).await.unwrap();
        assert!(review_needs_fresh_successor(&root, &state).await.unwrap());

        let next_review_id = current_or_create_fresh_review_id(&root, author())
            .await
            .unwrap();
        assert_ne!(review_id, next_review_id);

        let next_state = load_review_state(&root, &next_review_id).await.unwrap();
        assert_eq!(
            next_state.diff_base_oid,
            Some(current_head_oid(&root).await.unwrap())
        );

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

    fn init_repo(root: &Path) {
        gix::init(root).expect("failed to init gix");
        let config_path = root.join(".git").join("config");
        let mut config = fs::read_to_string(&config_path).unwrap();
        config.push_str(&format!(
            "\n[user]\n\t{} = {}\n\t{} = {}\n",
            GIT_USER_NAME_FIELD, TEST_AUTHOR_NAME, GIT_USER_EMAIL_FIELD, TEST_AUTHOR_EMAIL
        ));
        fs::write(config_path, config).unwrap();
    }

    fn commit_file(root: &Path, message: &str, body: &str) {
        let repo = gix::open(root).unwrap();
        let blob_id = repo.write_blob(body.as_bytes()).unwrap().detach();
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: TEST_FILE.into(),
                oid: blob_id,
            }],
        };
        let tree_id = repo.write_object(tree).unwrap().detach();
        let parents = current_head_oid_blocking(&repo)
            .into_iter()
            .collect::<Vec<_>>();
        repo.commit(HEAD_REF, message, tree_id, parents).unwrap();
    }

    fn current_head_oid_blocking(repo: &gix::Repository) -> Option<gix::ObjectId> {
        repo.rev_parse_single(HEAD_REF).ok().map(|id| id.detach())
    }
}
