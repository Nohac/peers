use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use facet::Facet;
use thiserror::Error;

use crate::comments::{Author, AuthorKind, Comment, CommentThread, ReviewEvent, ReviewState};
use crate::diff::{
    CommentAnchor, FileContent, FileDiff, FileSide, LineAnchor, ReviewDiffPayload, ReviewFile,
    ReviewTarget, load_review_diff,
};
use crate::review::{
    append_review_event, load_review_state, new_comment_id, new_thread_id, now_rfc3339,
    regenerate_outputs,
};

const LINE_SCOPE: &str = "line";
const FILE_SCOPE: &str = "file";
const REVIEW_SCOPE: &str = "review";
const HUMAN_AUTHOR_KIND: &str = "human";
const AGENT_AUTHOR_KIND: &str = "agent";
const OLD_FILE_SIDE: &str = "old";
const NEW_FILE_SIDE: &str = "new";

#[derive(Debug, Error)]
enum ReviewProviderError {
    #[error("review `{review_id}` has no target")]
    MissingTarget { review_id: String },
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
}

#[derive(Clone, Debug)]
pub struct ReviewProvider {
    repo_root: PathBuf,
    review_id: String,
    author: Author,
}

impl ReviewProvider {
    pub fn new(repo_root: PathBuf, review_id: String, author: Author) -> Self {
        Self {
            repo_root,
            review_id,
            author,
        }
    }

    pub fn review_id(&self) -> &str {
        &self.review_id
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn author(&self) -> &Author {
        &self.author
    }

    pub async fn get_review(&self) -> Result<ApiReviewPayload> {
        let state = load_review_state(&self.repo_root, &self.review_id).await?;
        let target = state
            .target
            .clone()
            .ok_or_else(|| ReviewProviderError::MissingTarget {
                review_id: self.review_id.clone(),
            })?;
        let diff = load_review_diff(&self.repo_root, &target).await?;
        Ok(review_payload(&state, diff, &self.author))
    }

    pub async fn refresh_diff(&self) -> Result<ApiReviewPayload> {
        regenerate_outputs(&self.repo_root, &self.review_id).await?;
        self.get_review().await
    }

    pub async fn create_thread(&self, request: CreateThreadRequest) -> Result<ApiReviewPayload> {
        let anchor = request.clone().into_anchor()?;
        let body = required_body(request.body)?;
        let thread_id = new_thread_id();
        let comment_id = new_comment_id();
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::ThreadCreated {
                thread_id,
                comment_id,
                created_at: now_rfc3339()?,
                author: self.author.clone(),
                anchor,
                body,
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn reply_to_thread(&self, request: ThreadBodyRequest) -> Result<ApiReviewPayload> {
        let body = required_body(request.body)?;
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::CommentAdded {
                thread_id: request.thread_id,
                comment_id: new_comment_id(),
                created_at: now_rfc3339()?,
                author: self.author.clone(),
                body,
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn edit_comment(&self, request: EditCommentRequest) -> Result<ApiReviewPayload> {
        let body = required_body(request.body)?;
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::CommentEdited {
                comment_id: request.comment_id,
                edited_at: now_rfc3339()?,
                author: self.author.clone(),
                body,
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn delete_comment(&self, request: CommentRequest) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::CommentDeleted {
                comment_id: request.comment_id,
                deleted_at: now_rfc3339()?,
                author: self.author.clone(),
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn delete_thread(&self, request: ThreadRequest) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::ThreadDeleted {
                thread_id: request.thread_id,
                deleted_at: now_rfc3339()?,
                author: self.author.clone(),
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn resolve_thread(&self, request: ThreadRequest) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::ThreadResolved {
                thread_id: request.thread_id,
                resolved_at: now_rfc3339()?,
                author: self.author.clone(),
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn reopen_thread(&self, request: ThreadRequest) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::ThreadReopened {
                thread_id: request.thread_id,
                reopened_at: now_rfc3339()?,
                author: self.author.clone(),
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn mark_file_viewed(
        &self,
        request: MarkFileViewedRequest,
    ) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::FileMarkedViewed {
                path: request.path,
                viewed: request.viewed,
                marked_at: now_rfc3339()?,
                author: self.author.clone(),
            },
        )
        .await?;
        self.get_review().await
    }

    pub async fn submit_review(&self, request: SubmitReviewRequest) -> Result<ApiReviewPayload> {
        append_review_event(
            &self.repo_root,
            &self.review_id,
            &ReviewEvent::ReviewSubmitted {
                review_id: self.review_id.clone(),
                submitted_at: now_rfc3339()?,
                author: self.author.clone(),
                body: request.body,
            },
        )
        .await?;
        self.get_review().await
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ApiReviewPayload {
    pub review_id: String,
    pub target_label: String,
    pub is_branch_review: bool,
    pub files: Vec<ReviewFile>,
    pub file_contents_by_path: BTreeMap<String, FileContent>,
    pub file_diffs_by_path: BTreeMap<String, FileDiff>,
    pub threads: Vec<ApiCommentThread>,
    pub review_threads: Vec<ApiCommentThread>,
    pub commits: Vec<ApiCommit>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ApiCommit {
    pub oid: String,
    pub summary: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ApiCommentThread {
    pub id: String,
    pub scope: String,
    pub path: Option<String>,
    pub line_label: String,
    pub anchor: ApiThreadAnchor,
    pub resolved: bool,
    pub comments: Vec<ApiReviewComment>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ApiThreadAnchor {
    pub side: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ApiReviewComment {
    pub id: String,
    pub author_name: String,
    pub author_kind: String,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
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
    state: &ReviewState,
    mut diff: ReviewDiffPayload,
    current_author: &Author,
) -> ApiReviewPayload {
    let threads: Vec<_> = state
        .threads
        .values()
        .map(|thread| api_thread(thread, current_author))
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
        file.viewed = state.viewed_files.get(&file.path).copied().unwrap_or(false);
        file.comment_count = comment_counts.get(&file.path).copied().unwrap_or(0);
    }

    let target = state.target.clone().unwrap_or(ReviewTarget::WorkingTree);
    let review_threads = threads
        .iter()
        .filter(|thread| thread.scope == REVIEW_SCOPE)
        .cloned()
        .collect();

    ApiReviewPayload {
        review_id: state.review_id.clone().unwrap_or_default(),
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

fn api_thread(thread: &CommentThread, current_author: &Author) -> ApiCommentThread {
    let (scope, path, anchor) = match &thread.anchor {
        CommentAnchor::Line { line } => (
            LINE_SCOPE.to_string(),
            Some(line.path.clone()),
            ApiThreadAnchor {
                side: Some(file_side_name(&line.side).to_string()),
                start_line: Some(line.start_line),
                end_line: Some(line.end_line),
            },
        ),
        CommentAnchor::File { path } => (
            FILE_SCOPE.to_string(),
            Some(path.clone()),
            ApiThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
            },
        ),
        CommentAnchor::Review => (
            REVIEW_SCOPE.to_string(),
            None,
            ApiThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
            },
        ),
    };

    ApiCommentThread {
        id: thread.id.clone(),
        scope,
        path,
        line_label: thread.anchor.label(),
        anchor,
        resolved: thread.resolved,
        comments: thread
            .comments
            .iter()
            .filter(|comment| comment.deleted_at.is_none())
            .map(|comment| api_comment(comment, current_author))
            .collect(),
    }
}

fn api_comment(comment: &Comment, current_author: &Author) -> ApiReviewComment {
    ApiReviewComment {
        id: comment.id.clone(),
        author_name: comment.author.display_name.clone(),
        author_kind: author_kind_name(&comment.author.kind).to_string(),
        body: comment.body.clone(),
        created_at: comment.created_at.clone(),
        edited_at: comment.edited_at.clone(),
        can_edit: comment.author.kind == current_author.kind
            && comment.author.display_name == current_author.display_name,
    }
}

fn author_kind_name(kind: &AuthorKind) -> &'static str {
    match kind {
        AuthorKind::Human => HUMAN_AUTHOR_KIND,
        AuthorKind::Agent => AGENT_AUTHOR_KIND,
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
