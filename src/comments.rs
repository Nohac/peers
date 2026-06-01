use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result, anyhow};
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
        thread_id: String,
        comment_id: String,
        created_at: String,
        author: Author,
    },
    CommentAdded {
        thread_id: String,
        comment_id: String,
        created_at: String,
        author: Author,
    },
    CommentEdited {
        thread_id: String,
        comment_id: String,
        edited_at: String,
        author: Author,
    },
    CommentDeleted {
        thread_id: String,
        comment_id: String,
        deleted_at: String,
        author: Author,
    },
    ThreadResolved {
        thread_id: String,
        resolved_at: String,
        author: Author,
    },
    ThreadReopened {
        thread_id: String,
        reopened_at: String,
        author: Author,
    },
    ThreadArchived {
        thread_id: String,
        archived_at: String,
        author: Author,
        reason: Option<String>,
    },
    ThreadPruned {
        thread_id: String,
        pruned_at: String,
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
    pub id: String,
    pub status: ThreadStatus,
    pub anchor: CommentAnchor,
    pub created_at: String,
    pub updated_at: String,
    pub provenance: CreationProvenance,
    pub archived_at: Option<String>,
    pub pruned_at: Option<String>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentPayload {
    pub id: String,
    pub thread_id: String,
    pub author: Author,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PayloadStore {
    pub threads: BTreeMap<String, ThreadPayload>,
    pub comments: BTreeMap<String, CommentPayload>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct Comment {
    pub id: String,
    pub thread_id: String,
    pub author: Author,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub deleted_at: Option<String>,
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

impl From<CommentPayload> for Comment {
    fn from(payload: CommentPayload) -> Self {
        Self {
            id: payload.id,
            thread_id: payload.thread_id,
            author: payload.author,
            body: payload.body,
            created_at: payload.created_at,
            edited_at: payload.edited_at,
            deleted_at: payload.deleted_at,
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentThread {
    pub id: String,
    pub anchor: CommentAnchor,
    pub comments: Vec<Comment>,
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub pruned_at: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeersState {
    pub threads: BTreeMap<String, CommentThread>,
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

pub fn encode_comment_payload(payload: &CommentPayload) -> Result<String> {
    facet_json::to_string(payload).context("failed to encode comment payload")
}

pub fn decode_comment_payload(input: &str) -> Result<CommentPayload> {
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
                    comments: visible_comments(vec![comment.clone().into()]),
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
            thread.comments.push(comment.clone().into());
            thread.comments = visible_comments(std::mem::take(&mut thread.comments));
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
            thread.comments[comment_index] = payload.clone().into();
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
                thread.comments[comment_index] = payload.clone().into();
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

    #[tokio::test]
    async fn event_roundtrip_replays_payload_state() {
        let anchor = CommentAnchor::Line {
            line: LineAnchor::new("src/main.rs".to_string(), FileSide::New, 4, 6),
        };
        let thread = ThreadPayload {
            id: "thr_test".to_string(),
            status: ThreadStatus::Open,
            anchor,
            created_at: "2026-05-28T12:01:00Z".to_string(),
            updated_at: "2026-05-28T12:01:00Z".to_string(),
            provenance: CreationProvenance::from_target(&ReviewTarget::WorkingTree),
            archived_at: None,
            pruned_at: None,
        };
        let comment = CommentPayload {
            id: "cmt_test".to_string(),
            thread_id: "thr_test".to_string(),
            author: author(),
            body: "Needs a testable event log.".to_string(),
            created_at: "2026-05-28T12:01:00Z".to_string(),
            edited_at: None,
            deleted_at: None,
        };
        let payloads = PayloadStore {
            threads: BTreeMap::from([(thread.id.clone(), thread)]),
            comments: BTreeMap::from([(comment.id.clone(), comment)]),
        };
        let events = vec![
            PeersEvent::ThreadCreated {
                thread_id: "thr_test".to_string(),
                comment_id: "cmt_test".to_string(),
                created_at: "2026-05-28T12:01:00Z".to_string(),
                author: author(),
            },
            PeersEvent::ThreadResolved {
                thread_id: "thr_test".to_string(),
                resolved_at: "2026-05-28T12:02:00Z".to_string(),
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

        assert!(state.threads["thr_test"].resolved);
        assert_eq!(
            state.threads["thr_test"].comments[0].body,
            "Needs a testable event log."
        );
    }

    #[test]
    fn payload_roundtrip() {
        let payload = CommentPayload {
            id: "cmt_test".to_string(),
            thread_id: "thr_test".to_string(),
            author: author(),
            body: "hello".to_string(),
            created_at: "2026-05-28T12:01:00Z".to_string(),
            edited_at: None,
            deleted_at: None,
        };
        let encoded = encode_comment_payload(&payload).unwrap();
        assert_eq!(decode_comment_payload(&encoded).unwrap(), payload);
    }
}
