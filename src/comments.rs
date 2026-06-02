use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use facet::Facet;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::diff::{CommentAnchor, ReviewTarget};

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum AuthorKind {
    Human,
    Agent,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct Author {
    pub kind: AuthorKind,
    pub display_name: String,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[facet(transparent)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if !value.starts_with("thr_") {
            return Err(anyhow!("thread id must start with `thr_`"));
        }
        Ok(Self(value))
    }

    pub fn from_raw(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Facet, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[facet(transparent)]
pub struct CommentId(String);

impl CommentId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if !value.starts_with("cmt_") {
            return Err(anyhow!("comment id must start with `cmt_`"));
        }
        Ok(Self(value))
    }

    pub fn from_raw(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CommentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Facet, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[facet(transparent)]
pub struct PeersTimestamp(String);

impl PeersTimestamp {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let _ = DateTime::parse_from_rfc3339(&value)
            .with_context(|| format!("invalid RFC3339 timestamp `{value}`"))?
            .with_timezone(&Utc);
        Ok(Self(value))
    }

    pub fn from_rfc3339_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PeersTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Author {
    pub fn fallback_human() -> Self {
        Self {
            kind: AuthorKind::Human,
            display_name: "unknown user".to_string(),
            email: None,
        }
    }

    pub fn fallback_agent() -> Self {
        Self {
            kind: AuthorKind::Agent,
            display_name: "ai agent".to_string(),
            email: None,
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
#[facet(tag = "kind", rename_all = "snake_case")]
pub enum PeersEvent {
    ThreadCreated {
        thread_id: ThreadId,
        comment_id: CommentId,
        created_at: PeersTimestamp,
        author: Author,
    },
    CommentAdded {
        thread_id: ThreadId,
        comment_id: CommentId,
        created_at: PeersTimestamp,
        author: Author,
    },
    CommentEdited {
        thread_id: ThreadId,
        comment_id: CommentId,
        edited_at: PeersTimestamp,
        author: Author,
    },
    CommentDeleted {
        thread_id: ThreadId,
        comment_id: CommentId,
        deleted_at: PeersTimestamp,
        author: Author,
    },
    ThreadResolved {
        thread_id: ThreadId,
        resolved_at: PeersTimestamp,
        author: Author,
    },
    ThreadReopened {
        thread_id: ThreadId,
        reopened_at: PeersTimestamp,
        author: Author,
    },
    ThreadArchived {
        thread_id: ThreadId,
        archived_at: PeersTimestamp,
        author: Author,
        reason: Option<String>,
    },
    ThreadPruned {
        thread_id: ThreadId,
        pruned_at: PeersTimestamp,
        author: Author,
        reason: Option<String>,
    },
}

pub type ReviewEvent = PeersEvent;

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum ThreadStatus {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CreationProvenance {
    pub view_kind: String,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub merge_base_oid: Option<String>,
}

impl CreationProvenance {
    pub fn from_target(target: &ReviewTarget) -> Self {
        Self {
            view_kind: target.label(),
            branch: match target {
                ReviewTarget::Branch { head, .. } => Some(head.clone()),
                _ => None,
            },
            head_oid: None,
            merge_base_oid: None,
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ThreadPayload {
    pub id: ThreadId,
    pub status: ThreadStatus,
    pub anchor: CommentAnchor,
    pub created_at: PeersTimestamp,
    pub updated_at: PeersTimestamp,
    pub provenance: CreationProvenance,
    pub archived_at: Option<PeersTimestamp>,
    pub pruned_at: Option<PeersTimestamp>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct Comment {
    pub id: CommentId,
    pub thread_id: ThreadId,
    pub author: Author,
    pub body: String,
    pub created_at: PeersTimestamp,
    pub edited_at: Option<PeersTimestamp>,
    pub deleted_at: Option<PeersTimestamp>,
}

impl Comment {
    fn visible_body(&self) -> &str {
        if self.deleted_at.is_some() {
            "[deleted]"
        } else {
            &self.body
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PayloadStore {
    pub threads: BTreeMap<ThreadId, ThreadPayload>,
    pub comments: BTreeMap<CommentId, Comment>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentThread {
    pub id: ThreadId,
    pub anchor: CommentAnchor,
    pub comments: Vec<Comment>,
    pub resolved: bool,
    pub created_at: PeersTimestamp,
    pub updated_at: PeersTimestamp,
    pub archived_at: Option<PeersTimestamp>,
    pub pruned_at: Option<PeersTimestamp>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeersState {
    pub threads: BTreeMap<ThreadId, CommentThread>,
}

impl PeersState {
    pub fn unresolved_threads(&self) -> impl Iterator<Item = &CommentThread> {
        self.threads
            .values()
            .filter(|thread| !thread.resolved && thread.archived_at.is_none())
    }
}

#[cfg(test)]
async fn parse_events(input: &str) -> Result<Vec<PeersEvent>> {
    let mut events = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event = facet_json::from_str::<PeersEvent>(line)
            .with_context(|| format!("failed to parse Peers event on line {}", index + 1))?;
        events.push(event);
    }
    Ok(events)
}

pub async fn parse_events_from_reader(
    reader: impl AsyncBufRead + Unpin,
) -> Result<Vec<PeersEvent>> {
    let mut lines = reader.lines();
    let mut events = Vec::new();
    let mut line_number = 0usize;

    while let Some(line) = lines.next_line().await? {
        line_number += 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event = facet_json::from_str::<PeersEvent>(line)
            .with_context(|| format!("failed to parse Peers event on line {line_number}"))?;
        events.push(event);
    }

    Ok(events)
}

pub fn encode_event(event: &PeersEvent) -> Result<String> {
    facet_json::to_string(event).context("failed to encode Peers event")
}

pub fn encode_thread_payload(payload: &ThreadPayload) -> Result<String> {
    facet_json::to_string(payload).context("failed to encode thread payload")
}

pub fn decode_thread_payload(input: &str) -> Result<ThreadPayload> {
    facet_json::from_str(input).context("failed to decode thread payload")
}

pub fn encode_comment_payload(payload: &Comment) -> Result<String> {
    facet_json::to_string(payload).context("failed to encode comment payload")
}

pub fn decode_comment_payload(input: &str) -> Result<Comment> {
    facet_json::from_str(input).context("failed to decode comment payload")
}

pub fn replay_events(events: &[PeersEvent], payloads: &PayloadStore) -> Result<PeersState> {
    let mut state = PeersState::default();

    for event in events {
        apply_event(&mut state, payloads, event)?;
    }

    Ok(state)
}

fn apply_event(state: &mut PeersState, payloads: &PayloadStore, event: &PeersEvent) -> Result<()> {
    match event {
        PeersEvent::ThreadCreated {
            thread_id,
            comment_id,
            ..
        } => {
            let payload = payloads
                .threads
                .get(thread_id)
                .ok_or_else(|| anyhow!("thread event references missing payload `{thread_id}`"))?;
            let comment = payloads.comments.get(comment_id).ok_or_else(|| {
                anyhow!("thread event references missing comment payload `{comment_id}`")
            })?;
            state.threads.insert(
                thread_id.clone(),
                CommentThread {
                    id: payload.id.clone(),
                    anchor: payload.anchor.clone(),
                    comments: vec![comment.clone()],
                    resolved: payload.status == ThreadStatus::Resolved,
                    created_at: payload.created_at.clone(),
                    updated_at: payload.updated_at.clone(),
                    archived_at: payload.archived_at.clone(),
                    pruned_at: payload.pruned_at.clone(),
                },
            );
        }
        PeersEvent::CommentAdded {
            thread_id,
            comment_id,
            created_at,
            ..
        } => {
            let comment = payloads.comments.get(comment_id).ok_or_else(|| {
                anyhow!("comment event references missing payload `{comment_id}`")
            })?;
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("comment references unknown thread `{thread_id}`"))?;
            thread.comments.push(comment.clone());
            thread.updated_at = created_at.clone();
        }
        PeersEvent::CommentEdited {
            thread_id,
            comment_id,
            edited_at,
            ..
        } => {
            let payload = payloads
                .comments
                .get(comment_id)
                .ok_or_else(|| anyhow!("edit references missing comment payload `{comment_id}`"))?;
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("edit references unknown thread `{thread_id}`"))?;
            let comment_index = thread
                .comments
                .iter()
                .position(|comment| comment.id == *comment_id)
                .ok_or_else(|| anyhow!("unknown comment `{comment_id}`"))?;
            thread.comments.truncate(comment_index + 1);
            thread.comments[comment_index] = payload.clone();
            thread.resolved = false;
            thread.updated_at = edited_at.clone();
        }
        PeersEvent::CommentDeleted {
            thread_id,
            comment_id,
            deleted_at,
            ..
        } => {
            let payload = payloads.comments.get(comment_id).ok_or_else(|| {
                anyhow!("delete references missing comment payload `{comment_id}`")
            })?;
            let remove_thread = {
                let thread = state
                    .threads
                    .get_mut(thread_id)
                    .ok_or_else(|| anyhow!("delete references unknown thread `{thread_id}`"))?;
                let comment_index = thread
                    .comments
                    .iter()
                    .position(|comment| comment.id == *comment_id)
                    .ok_or_else(|| anyhow!("unknown comment `{comment_id}`"))?;
                thread.comments.truncate(comment_index + 1);
                thread.comments[comment_index] = payload.clone();
                thread.comments = visible_comments(std::mem::take(&mut thread.comments));
                thread.resolved = false;
                thread.updated_at = deleted_at.clone();
                thread.comments.is_empty()
            };
            if remove_thread {
                state.threads.remove(thread_id);
            }
        }
        PeersEvent::ThreadResolved {
            thread_id,
            resolved_at,
            ..
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("resolve references unknown thread `{thread_id}`"))?;
            thread.resolved = true;
            thread.updated_at = resolved_at.clone();
        }
        PeersEvent::ThreadReopened {
            thread_id,
            reopened_at,
            ..
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("reopen references unknown thread `{thread_id}`"))?;
            thread.resolved = false;
            thread.updated_at = reopened_at.clone();
        }
        PeersEvent::ThreadArchived {
            thread_id,
            archived_at,
            ..
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("archive references unknown thread `{thread_id}`"))?;
            thread.archived_at = Some(archived_at.clone());
            thread.updated_at = archived_at.clone();
        }
        PeersEvent::ThreadPruned {
            thread_id,
            pruned_at,
            ..
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("prune references unknown thread `{thread_id}`"))?;
            thread.pruned_at = Some(pruned_at.clone());
            thread.updated_at = pruned_at.clone();
        }
    }

    Ok(())
}

fn visible_comments(comments: Vec<Comment>) -> Vec<Comment> {
    comments
        .into_iter()
        .filter(|comment| comment.deleted_at.is_none())
        .collect()
}

pub async fn render_agent_context(
    state: &PeersState,
    target: Option<&ReviewTarget>,
    mut out: impl AsyncWrite + Unpin,
) -> Result<()> {
    out.write_all(b"# Peers Review\n\nReview: repo-scoped comments\n")
        .await?;

    if let Some(target) = target {
        out.write_all(format!("Target: {}\n", target.label()).as_bytes())
            .await?;
    }

    let unresolved: Vec<_> = state.unresolved_threads().collect();
    out.write_all(format!("Unresolved comments: {}\n\n", unresolved.len()).as_bytes())
        .await?;

    if unresolved.is_empty() {
        out.write_all(b"No unresolved comments.\n").await?;
        return Ok(());
    }

    for thread in unresolved {
        out.write_all(format!("## {}\n\n", thread.anchor.label()).as_bytes())
            .await?;
        out.write_all(format!("Thread: `{}`\n\n", thread.id).as_bytes())
            .await?;
        for comment in &thread.comments {
            out.write_all(
                format!(
                    "- {} ({:?}) at {}: {}\n",
                    comment.author.display_name,
                    comment.author.kind,
                    comment.created_at,
                    comment.visible_body().replace('\n', "\n  ")
                )
                .as_bytes(),
            )
            .await?;
        }
        out.write_all(b"\n").await?;
    }

    Ok(())
}

pub async fn render_review_markdown(
    state: &PeersState,
    target: Option<&ReviewTarget>,
    mut out: impl AsyncWrite + Unpin,
) -> Result<()> {
    out.write_all(b"# Peers Review\n\n").await?;

    if let Some(target) = target {
        out.write_all(format!("Target: `{}`\n\n", target.label()).as_bytes())
            .await?;
    }

    out.write_all(format!("Threads: {}\n\n", state.threads.len()).as_bytes())
        .await?;
    for thread in state.threads.values() {
        let status = if thread.resolved {
            "resolved"
        } else {
            "unresolved"
        };
        out.write_all(
            format!(
                "## {} ({status})\n\nThread: `{}`\n\n",
                thread.anchor.label(),
                thread.id
            )
            .as_bytes(),
        )
        .await?;
        for comment in &thread.comments {
            out.write_all(
                format!(
                    "- `{}` by {} at {}: {}\n",
                    comment.id,
                    comment.author.display_name,
                    comment.created_at,
                    comment.visible_body().replace('\n', "\n  ")
                )
                .as_bytes(),
            )
            .await?;
        }
        out.write_all(b"\n").await?;
    }

    Ok(())
}

pub fn hash_text(input: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileSide, LineAnchor};

    fn author() -> Author {
        Author {
            kind: AuthorKind::Human,
            display_name: "Jonas".to_string(),
            email: Some("jonas@example.com".to_string()),
        }
    }

    fn thread_id() -> ThreadId {
        ThreadId::new("thr_test").unwrap()
    }

    fn comment_id() -> CommentId {
        CommentId::new("cmt_test").unwrap()
    }

    fn timestamp(input: &str) -> PeersTimestamp {
        PeersTimestamp::new(input).unwrap()
    }

    #[tokio::test]
    async fn event_roundtrip_replays_payload_state() {
        let anchor = CommentAnchor::Line {
            line: LineAnchor::new("src/main.rs".to_string(), FileSide::New, 4, 6),
        };
        let thread = ThreadPayload {
            id: thread_id(),
            status: ThreadStatus::Open,
            anchor,
            created_at: timestamp("2026-05-28T12:01:00Z"),
            updated_at: timestamp("2026-05-28T12:01:00Z"),
            provenance: CreationProvenance::from_target(&ReviewTarget::WorkingTree),
            archived_at: None,
            pruned_at: None,
        };
        let comment = Comment {
            id: comment_id(),
            thread_id: thread_id(),
            author: author(),
            body: "Needs a testable event log.".to_string(),
            created_at: timestamp("2026-05-28T12:01:00Z"),
            edited_at: None,
            deleted_at: None,
        };
        let payloads = PayloadStore {
            threads: BTreeMap::from([(thread.id.clone(), thread)]),
            comments: BTreeMap::from([(comment.id.clone(), comment)]),
        };
        let events = vec![
            PeersEvent::ThreadCreated {
                thread_id: thread_id(),
                comment_id: comment_id(),
                created_at: timestamp("2026-05-28T12:01:00Z"),
                author: author(),
            },
            PeersEvent::ThreadResolved {
                thread_id: thread_id(),
                resolved_at: timestamp("2026-05-28T12:02:00Z"),
                author: author(),
            },
        ];

        let input = events
            .iter()
            .map(encode_event)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        let decoded = parse_events(&input).await.unwrap();
        let state = replay_events(&decoded, &payloads).unwrap();

        assert!(state.threads[&thread_id()].resolved);
        assert_eq!(
            state.threads[&thread_id()].comments[0].body,
            "Needs a testable event log."
        );
    }

    #[test]
    fn replay_deletes_comment_payload_with_latest_deleted_state() {
        let anchor = CommentAnchor::Line {
            line: LineAnchor::new("src/main.rs".to_string(), FileSide::New, 4, 4),
        };
        let thread = ThreadPayload {
            id: thread_id(),
            status: ThreadStatus::Open,
            anchor,
            created_at: timestamp("2026-05-28T12:01:00Z"),
            updated_at: timestamp("2026-05-28T12:02:00Z"),
            provenance: CreationProvenance::from_target(&ReviewTarget::WorkingTree),
            archived_at: None,
            pruned_at: None,
        };
        let comment = Comment {
            id: comment_id(),
            thread_id: thread_id(),
            author: author(),
            body: "Delete this.".to_string(),
            created_at: timestamp("2026-05-28T12:01:00Z"),
            edited_at: None,
            deleted_at: Some(timestamp("2026-05-28T12:02:00Z")),
        };
        let payloads = PayloadStore {
            threads: BTreeMap::from([(thread.id.clone(), thread)]),
            comments: BTreeMap::from([(comment.id.clone(), comment)]),
        };
        let events = vec![
            PeersEvent::ThreadCreated {
                thread_id: thread_id(),
                comment_id: comment_id(),
                created_at: timestamp("2026-05-28T12:01:00Z"),
                author: author(),
            },
            PeersEvent::CommentDeleted {
                thread_id: thread_id(),
                comment_id: comment_id(),
                deleted_at: timestamp("2026-05-28T12:02:00Z"),
                author: author(),
            },
        ];

        let state = replay_events(&events, &payloads).unwrap();

        assert!(!state.threads.contains_key(&thread_id()));
    }

    #[test]
    fn payload_roundtrip() {
        let payload = Comment {
            id: comment_id(),
            thread_id: thread_id(),
            author: author(),
            body: "hello".to_string(),
            created_at: timestamp("2026-05-28T12:01:00Z"),
            edited_at: None,
            deleted_at: None,
        };
        let encoded = encode_comment_payload(&payload).unwrap();
        assert_eq!(decode_comment_payload(&encoded).unwrap(), payload);
    }
}
