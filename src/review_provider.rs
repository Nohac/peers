use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use facet::Facet;
use thiserror::Error;

use crate::comments::{
    Author, Comment, CommentId, CommentThread, CreationProvenance, PeersEvent, PeersState,
    ThreadId, ThreadPayload, ThreadStatus,
};
use crate::diff::{
    CommentAnchor, FileContent, FileDiff, FileSide, LineAnchor, ReviewDiffPayload, ReviewFile,
    ReviewTarget, load_review_diff,
};
use crate::realtime::ReviewUpdateBroadcaster;
use crate::review::{
    append_peers_event, load_comment_payload, load_peers_state, load_thread_payload,
    new_comment_id, new_thread_id, now_rfc3339, regenerate_outputs, write_comment_payload,
    write_thread_payload,
};

const LINE_SCOPE: &str = "line";
const FILE_SCOPE: &str = "file";
const REVIEW_SCOPE: &str = "review";
const OLD_FILE_SIDE: &str = "old";
const NEW_FILE_SIDE: &str = "new";

#[derive(Debug, Error)]
enum ReviewProviderError {
    #[error("line thread requires path")]
    LineThreadMissingPath,
    #[error("line thread requires start_line")]
    LineThreadMissingStartLine,
    #[error("file thread requires path")]
    FileThreadMissingPath,
    #[error("unknown thread scope `{scope}`")]
    UnknownThreadScope { scope: String },
    #[error("comment body cannot be empty")]
    EmptyCommentBody,
    #[error("unknown thread `{thread_id}`")]
    UnknownThread { thread_id: String },
    #[error("unknown comment `{comment_id}`")]
    UnknownComment { comment_id: String },
}

#[derive(Clone, Debug)]
pub struct ReviewProvider {
    repo_root: PathBuf,
    target: ReviewTarget,
    author: Author,
    updates: ReviewUpdateBroadcaster,
}

impl ReviewProvider {
    pub fn new(
        repo_root: PathBuf,
        target: ReviewTarget,
        author: Author,
        updates: ReviewUpdateBroadcaster,
    ) -> Self {
        Self {
            repo_root,
            target,
            author,
            updates,
        }
    }

    pub fn target(&self) -> &ReviewTarget {
        &self.target
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn author(&self) -> &Author {
        &self.author
    }

    pub fn updates(&self) -> ReviewUpdateBroadcaster {
        self.updates.clone()
    }

    pub async fn get_review(&self) -> Result<ReviewProjection> {
        let state = load_peers_state(&self.repo_root).await?;
        let diff = load_review_diff(&self.repo_root, &self.target).await?;
        Ok(review_payload(&state, diff, &self.target, &self.author))
    }

    pub async fn refresh_diff(&self) -> Result<ReviewProjection> {
        regenerate_outputs(&self.repo_root, Some(&self.target)).await?;
        let review = self.get_review().await?;
        self.updates.notify_diff_changed();
        Ok(review)
    }

    pub async fn create_thread(&self, request: CreateThreadRequest) -> Result<ReviewProjection> {
        let anchor = request.clone().into_anchor()?;
        let body = required_body(request.body)?;
        let thread_id = new_thread_id();
        let comment_id = new_comment_id();
        let now = now_rfc3339()?;
        write_thread_payload(
            &self.repo_root,
            &ThreadPayload {
                id: thread_id.clone(),
                status: ThreadStatus::Open,
                anchor,
                created_at: now.clone(),
                updated_at: now.clone(),
                provenance: CreationProvenance::from_target(&self.target),
                archived_at: None,
                pruned_at: None,
            },
        )
        .await?;
        write_comment_payload(
            &self.repo_root,
            &Comment {
                id: comment_id.clone(),
                thread_id: thread_id.clone(),
                author: self.author.clone(),
                body,
                created_at: now.clone(),
                edited_at: None,
                deleted_at: None,
            },
        )
        .await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::ThreadCreated {
                thread_id,
                comment_id,
                created_at: now,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn reply_to_thread(&self, request: ThreadBodyRequest) -> Result<ReviewProjection> {
        let body = required_body(request.body)?;
        let thread_id = ThreadId::new(request.thread_id)?;
        ensure_thread_exists(&self.repo_root, &thread_id).await?;
        let comment_id = new_comment_id();
        let now = now_rfc3339()?;
        write_comment_payload(
            &self.repo_root,
            &Comment {
                id: comment_id.clone(),
                thread_id: thread_id.clone(),
                author: self.author.clone(),
                body,
                created_at: now.clone(),
                edited_at: None,
                deleted_at: None,
            },
        )
        .await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::CommentAdded {
                thread_id,
                comment_id,
                created_at: now,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn edit_comment(&self, request: EditCommentRequest) -> Result<ReviewProjection> {
        let body = required_body(request.body)?;
        let comment_id = CommentId::new(request.comment_id)?;
        let (thread_id, mut comment) = find_comment_payload(&self.repo_root, &comment_id).await?;
        let edited_at = now_rfc3339()?;
        comment.body = body;
        comment.edited_at = Some(edited_at.clone());
        write_comment_payload(&self.repo_root, &comment).await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::CommentEdited {
                thread_id,
                comment_id,
                edited_at,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn delete_comment(&self, request: CommentRequest) -> Result<ReviewProjection> {
        let comment_id = CommentId::new(request.comment_id)?;
        let (thread_id, mut comment) = find_comment_payload(&self.repo_root, &comment_id).await?;
        let deleted_at = now_rfc3339()?;
        comment.deleted_at = Some(deleted_at.clone());
        write_comment_payload(&self.repo_root, &comment).await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::CommentDeleted {
                thread_id,
                comment_id,
                deleted_at,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn delete_thread(&self, request: ThreadRequest) -> Result<ReviewProjection> {
        let thread_id = ThreadId::new(request.thread_id)?;
        let mut thread = load_thread_payload(&self.repo_root, &thread_id).await?;
        let archived_at = now_rfc3339()?;
        thread.archived_at = Some(archived_at.clone());
        thread.updated_at = archived_at.clone();
        write_thread_payload(&self.repo_root, &thread).await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::ThreadArchived {
                thread_id,
                archived_at,
                author: self.author.clone(),
                reason: Some("deleted through RPC".to_string()),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn resolve_thread(&self, request: ThreadRequest) -> Result<ReviewProjection> {
        let thread_id = ThreadId::new(request.thread_id)?;
        let mut thread = load_thread_payload(&self.repo_root, &thread_id).await?;
        let resolved_at = now_rfc3339()?;
        thread.status = ThreadStatus::Resolved;
        thread.updated_at = resolved_at.clone();
        write_thread_payload(&self.repo_root, &thread).await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::ThreadResolved {
                thread_id,
                resolved_at,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn reopen_thread(&self, request: ThreadRequest) -> Result<ReviewProjection> {
        let thread_id = ThreadId::new(request.thread_id)?;
        let mut thread = load_thread_payload(&self.repo_root, &thread_id).await?;
        let reopened_at = now_rfc3339()?;
        thread.status = ThreadStatus::Open;
        thread.updated_at = reopened_at.clone();
        write_thread_payload(&self.repo_root, &thread).await?;
        append_peers_event(
            &self.repo_root,
            &PeersEvent::ThreadReopened {
                thread_id,
                reopened_at,
                author: self.author.clone(),
            },
            Some(&self.target),
        )
        .await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }

    pub async fn mark_file_viewed(
        &self,
        _request: MarkFileViewedRequest,
    ) -> Result<ReviewProjection> {
        let review = self.get_review().await?;
        Ok(review)
    }

    pub async fn submit_review(&self, _request: SubmitReviewRequest) -> Result<ReviewProjection> {
        let review = self.get_review().await?;
        Ok(review)
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewProjection {
    pub review_id: String,
    pub target_label: String,
    pub is_branch_review: bool,
    pub files: Vec<ReviewFile>,
    pub file_contents_by_path: BTreeMap<String, FileContent>,
    pub file_diffs_by_path: BTreeMap<String, FileDiff>,
    pub threads: Vec<ReviewThread>,
    pub review_threads: Vec<ReviewThread>,
    pub commits: Vec<ReviewCommit>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewCommit {
    pub oid: String,
    pub summary: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewThread {
    pub id: String,
    pub scope: String,
    pub path: Option<String>,
    pub line_label: String,
    pub anchor: ReviewThreadAnchor,
    pub resolved: bool,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewThreadAnchor {
    pub side: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewComment {
    pub comment: Comment,
    pub can_edit: bool,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CreateThreadRequest {
    pub scope: String,
    pub path: Option<String>,
    pub side: Option<FileSide>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub body: String,
}

impl CreateThreadRequest {
    fn into_anchor(self) -> Result<CommentAnchor> {
        match self.scope.as_str() {
            LINE_SCOPE => {
                let path = self
                    .path
                    .ok_or(ReviewProviderError::LineThreadMissingPath)?;
                let side = self.side.unwrap_or(FileSide::New);
                let start_line = self
                    .start_line
                    .ok_or(ReviewProviderError::LineThreadMissingStartLine)?;
                let end_line = self.end_line.unwrap_or(start_line);
                Ok(CommentAnchor::Line {
                    line: LineAnchor::new(path, side, start_line, end_line),
                })
            }
            FILE_SCOPE => Ok(CommentAnchor::File {
                path: self
                    .path
                    .ok_or(ReviewProviderError::FileThreadMissingPath)?,
            }),
            REVIEW_SCOPE => Ok(CommentAnchor::Review),
            _ => Err(ReviewProviderError::UnknownThreadScope { scope: self.scope }.into()),
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ThreadBodyRequest {
    pub thread_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct EditCommentRequest {
    pub comment_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentRequest {
    pub comment_id: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ThreadRequest {
    pub thread_id: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct MarkFileViewedRequest {
    pub path: String,
    pub viewed: bool,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct SubmitReviewRequest {
    pub body: Option<String>,
}

fn review_payload(
    state: &PeersState,
    mut diff: ReviewDiffPayload,
    target: &ReviewTarget,
    current_author: &Author,
) -> ReviewProjection {
    let threads: Vec<_> = state
        .threads
        .values()
        .filter(|thread| thread.pruned_at.is_none() && thread.archived_at.is_none())
        .map(|thread| review_thread(thread, current_author))
        .collect();
    let mut comment_counts = BTreeMap::<String, u32>::new();
    for thread in &threads {
        if !thread.resolved
            && let Some(path) = &thread.path
        {
            *comment_counts.entry(path.clone()).or_default() += 1;
        }
    }
    for file in &mut diff.files {
        file.viewed = false;
        file.comment_count = comment_counts.get(&file.path).copied().unwrap_or(0);
    }

    let review_threads = threads
        .iter()
        .filter(|thread| thread.scope == REVIEW_SCOPE)
        .cloned()
        .collect();

    ReviewProjection {
        review_id: "repo".to_string(),
        target_label: target.label(),
        is_branch_review: target.is_branch(),
        files: diff.files,
        file_contents_by_path: diff.file_contents_by_path,
        file_diffs_by_path: diff.file_diffs_by_path,
        threads,
        review_threads,
        commits: Vec::new(),
    }
}

fn review_thread(thread: &CommentThread, current_author: &Author) -> ReviewThread {
    let (scope, path, anchor) = match &thread.anchor {
        CommentAnchor::Line { line } => (
            LINE_SCOPE.to_string(),
            Some(line.path.clone()),
            ReviewThreadAnchor {
                side: Some(file_side_name(&line.side).to_string()),
                start_line: Some(line.start_line),
                end_line: Some(line.end_line),
            },
        ),
        CommentAnchor::File { path } => (
            FILE_SCOPE.to_string(),
            Some(path.clone()),
            ReviewThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
            },
        ),
        CommentAnchor::Review => (
            REVIEW_SCOPE.to_string(),
            None,
            ReviewThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
            },
        ),
    };

    ReviewThread {
        id: thread.id.to_string(),
        scope,
        path,
        line_label: thread.anchor.label(),
        anchor,
        resolved: thread.resolved,
        comments: thread
            .comments
            .iter()
            .filter(|comment| comment.deleted_at.is_none())
            .map(|comment| review_comment(comment, current_author))
            .collect(),
    }
}

fn review_comment(comment: &Comment, current_author: &Author) -> ReviewComment {
    ReviewComment {
        comment: comment.clone(),
        can_edit: comment.author.kind == current_author.kind
            && comment.author.display_name == current_author.display_name,
    }
}

fn file_side_name(side: &FileSide) -> &'static str {
    match side {
        FileSide::Old => OLD_FILE_SIDE,
        FileSide::New => NEW_FILE_SIDE,
    }
}

fn required_body(body: String) -> Result<String> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err(ReviewProviderError::EmptyCommentBody.into());
    }
    Ok(body)
}

async fn ensure_thread_exists(repo_root: &Path, thread_id: &ThreadId) -> Result<()> {
    load_thread_payload(repo_root, thread_id)
        .await
        .map(|_| ())
        .map_err(|_| {
            ReviewProviderError::UnknownThread {
                thread_id: thread_id.to_string(),
            }
            .into()
        })
}

async fn find_comment_payload(
    repo_root: &Path,
    comment_id: &CommentId,
) -> Result<(ThreadId, Comment)> {
    let state = load_peers_state(repo_root).await?;
    for thread in state.threads.values() {
        if thread
            .comments
            .iter()
            .any(|comment| &comment.id == comment_id)
        {
            let payload = load_comment_payload(repo_root, &thread.id, comment_id).await?;
            return Ok((thread.id.clone(), payload));
        }
    }
    Err(ReviewProviderError::UnknownComment {
        comment_id: comment_id.to_string(),
    }
    .into())
}
