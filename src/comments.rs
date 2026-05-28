use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::diff::{LineAnchor, ReviewTarget};

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
pub enum ReviewEvent {
    ReviewCreated {
        review_id: String,
        created_at: String,
        author: Author,
        target: ReviewTarget,
    },
    ReviewMetadataUpdated {
        review_id: String,
        updated_at: String,
        author: Author,
        title: Option<String>,
    },
    ThreadCreated {
        thread_id: String,
        comment_id: String,
        created_at: String,
        author: Author,
        anchor: LineAnchor,
        body: String,
    },
    CommentAdded {
        thread_id: String,
        comment_id: String,
        created_at: String,
        author: Author,
        body: String,
    },
    CommentEdited {
        comment_id: String,
        edited_at: String,
        author: Author,
        body: String,
    },
    CommentDeleted {
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
    ThreadAnchored {
        thread_id: String,
        anchored_at: String,
        author: Author,
        anchor: LineAnchor,
    },
    FileMarkedViewed {
        path: String,
        viewed: bool,
        marked_at: String,
        author: Author,
    },
    ReviewSubmitted {
        review_id: String,
        submitted_at: String,
        author: Author,
        body: Option<String>,
    },
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

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentThread {
    pub id: String,
    pub anchor: LineAnchor,
    pub comments: Vec<Comment>,
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewState {
    pub review_id: Option<String>,
    pub target: Option<ReviewTarget>,
    pub created_at: Option<String>,
    pub author: Option<Author>,
    pub title: Option<String>,
    pub threads: BTreeMap<String, CommentThread>,
    pub viewed_files: BTreeMap<String, bool>,
    pub submitted_at: Option<String>,
}

impl ReviewState {
    pub fn unresolved_threads(&self) -> impl Iterator<Item = &CommentThread> {
        self.threads.values().filter(|thread| !thread.resolved)
    }
}

#[cfg(test)]
async fn parse_events(input: &str) -> Result<Vec<ReviewEvent>> {
    let mut events = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event = facet_json::from_str::<ReviewEvent>(line)
            .with_context(|| format!("failed to parse review event on line {}", index + 1))?;
        events.push(event);
    }
    Ok(events)
}

pub async fn parse_events_from_reader(
    reader: impl AsyncBufRead + Unpin,
) -> Result<Vec<ReviewEvent>> {
    let mut lines = reader.lines();
    let mut events = Vec::new();
    let mut line_number = 0usize;

    while let Some(line) = lines.next_line().await? {
        line_number += 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event = facet_json::from_str::<ReviewEvent>(line)
            .with_context(|| format!("failed to parse review event on line {line_number}"))?;
        events.push(event);
    }

    Ok(events)
}

pub fn encode_event(event: &ReviewEvent) -> Result<String> {
    facet_json::to_string(event).context("failed to encode review event")
}

pub fn replay_events(events: &[ReviewEvent]) -> Result<ReviewState> {
    let mut state = ReviewState::default();

    for event in events {
        apply_event(&mut state, event)?;
    }

    Ok(state)
}

fn apply_event(state: &mut ReviewState, event: &ReviewEvent) -> Result<()> {
    match event {
        ReviewEvent::ReviewCreated {
            review_id,
            created_at,
            author,
            target,
        } => {
            state.review_id = Some(review_id.clone());
            state.created_at = Some(created_at.clone());
            state.author = Some(author.clone());
            state.target = Some(target.clone());
        }
        ReviewEvent::ReviewMetadataUpdated { title, .. } => {
            state.title.clone_from(title);
        }
        ReviewEvent::ThreadCreated {
            thread_id,
            comment_id,
            created_at,
            author,
            anchor,
            body,
        } => {
            let comment = Comment {
                id: comment_id.clone(),
                thread_id: thread_id.clone(),
                author: author.clone(),
                body: body.clone(),
                created_at: created_at.clone(),
                edited_at: None,
                deleted_at: None,
            };
            state.threads.insert(
                thread_id.clone(),
                CommentThread {
                    id: thread_id.clone(),
                    anchor: anchor.clone(),
                    comments: vec![comment],
                    resolved: false,
                    created_at: created_at.clone(),
                    updated_at: created_at.clone(),
                },
            );
        }
        ReviewEvent::CommentAdded {
            thread_id,
            comment_id,
            created_at,
            author,
            body,
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("comment references unknown thread `{thread_id}`"))?;
            thread.comments.push(Comment {
                id: comment_id.clone(),
                thread_id: thread_id.clone(),
                author: author.clone(),
                body: body.clone(),
                created_at: created_at.clone(),
                edited_at: None,
                deleted_at: None,
            });
            thread.updated_at = created_at.clone();
        }
        ReviewEvent::CommentEdited {
            comment_id,
            edited_at,
            body,
            ..
        } => {
            let comment = find_comment_mut(state, comment_id)?;
            comment.body = body.clone();
            comment.edited_at = Some(edited_at.clone());
        }
        ReviewEvent::CommentDeleted {
            comment_id,
            deleted_at,
            ..
        } => {
            let comment = find_comment_mut(state, comment_id)?;
            comment.deleted_at = Some(deleted_at.clone());
        }
        ReviewEvent::ThreadResolved {
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
        ReviewEvent::ThreadReopened {
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
        ReviewEvent::ThreadAnchored {
            thread_id,
            anchored_at,
            anchor,
            ..
        } => {
            let thread = state
                .threads
                .get_mut(thread_id)
                .ok_or_else(|| anyhow!("anchor update references unknown thread `{thread_id}`"))?;
            thread.anchor = anchor.clone();
            thread.updated_at = anchored_at.clone();
        }
        ReviewEvent::FileMarkedViewed { path, viewed, .. } => {
            state.viewed_files.insert(path.clone(), *viewed);
        }
        ReviewEvent::ReviewSubmitted { submitted_at, .. } => {
            state.submitted_at = Some(submitted_at.clone());
        }
    }

    Ok(())
}

fn find_comment_mut<'a>(state: &'a mut ReviewState, comment_id: &str) -> Result<&'a mut Comment> {
    state
        .threads
        .values_mut()
        .flat_map(|thread| thread.comments.iter_mut())
        .find(|comment| comment.id == comment_id)
        .ok_or_else(|| anyhow!("unknown comment `{comment_id}`"))
}

pub async fn render_agent_context(
    state: &ReviewState,
    mut out: impl AsyncWrite + Unpin,
) -> Result<()> {
    let title = state
        .review_id
        .as_deref()
        .map_or("Peers Review", |review_id| review_id);
    out.write_all(format!("# Peers Review\n\nReview: {title}\n").as_bytes())
        .await?;

    if let Some(target) = &state.target {
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
        out.write_all(format!("## {}\n\n", thread.anchor.line_label()).as_bytes())
            .await?;
        out.write_all(format!("Thread: `{}`\n\n", thread.id).as_bytes())
            .await?;
        for comment in thread
            .comments
            .iter()
            .filter(|comment| comment.deleted_at.is_none())
        {
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
    state: &ReviewState,
    mut out: impl AsyncWrite + Unpin,
) -> Result<()> {
    let title = state
        .review_id
        .as_deref()
        .map_or("Peers Review", |review_id| review_id);
    out.write_all(format!("# {title}\n\n").as_bytes()).await?;

    if let Some(target) = &state.target {
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
                thread.anchor.line_label(),
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
    use crate::diff::FileSide;

    fn author() -> Author {
        Author {
            kind: AuthorKind::Human,
            display_name: "Jonas".to_string(),
            email: Some("jonas@example.com".to_string()),
        }
    }

    #[tokio::test]
    async fn event_roundtrip_replays_thread_state() {
        let anchor = LineAnchor::new("src/main.rs".to_string(), FileSide::New, 4, 6);
        let events = vec![
            ReviewEvent::ReviewCreated {
                review_id: "rev_test".to_string(),
                created_at: "2026-05-28T12:00:00Z".to_string(),
                author: author(),
                target: ReviewTarget::WorkingTree,
            },
            ReviewEvent::ThreadCreated {
                thread_id: "thr_test".to_string(),
                comment_id: "cmt_test".to_string(),
                created_at: "2026-05-28T12:01:00Z".to_string(),
                author: author(),
                anchor,
                body: "Needs a testable event log.".to_string(),
            },
            ReviewEvent::ThreadResolved {
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
        let state = replay_events(&decoded).unwrap();

        assert_eq!(state.review_id.as_deref(), Some("rev_test"));
        assert!(state.threads["thr_test"].resolved);
    }
}
