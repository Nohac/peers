use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use facet::Facet;
use futures::future::join_all;
use thiserror::Error;
use tracing::instrument;

use crate::anchors::{
    AnchorIndex, AnchorLinePlacement, AnchorPlacement, RelocatedLineAnchor,
    capture_line_anchor_evidence, relocate_line_anchor_in_index,
};
use crate::comments::{
    Author, Comment, CommentId, CommentThread, CreationProvenance, PeersEvent, PeersState,
    ThreadId, ThreadPayload, ThreadStatus,
};
use crate::diff::{
    CommentAnchor, FileContent, FileContextRequest, FileDiff, FileSide, LineAnchor,
    ReviewDiffPayload, ReviewFile, ReviewTarget, load_review_diff_with_contexts,
};
use crate::realtime::ReviewUpdateBroadcaster;
use crate::review::{
    append_peers_event, current_head_oid as repo_current_head_oid, load_comment_payload,
    load_peers_state, load_thread_payload, new_comment_id, new_thread_id, now_rfc3339,
    regenerate_outputs, write_comment_payload, write_thread_payload,
};

const LINE_SCOPE: &str = "line";
const FILE_SCOPE: &str = "file";
const REVIEW_SCOPE: &str = "review";
const OLD_FILE_SIDE: &str = "old";
const NEW_FILE_SIDE: &str = "new";
const ANCHOR_CONTEXT_LINES: usize = 3;

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

    #[instrument(name = "provider.get_review", skip_all, fields(target = %self.target.label()))]
    pub async fn get_review(&self) -> Result<ReviewProjection> {
        let state = load_peers_state(&self.repo_root).await?;
        let current_head_oid = repo_current_head_oid(&self.repo_root).await?;
        let contexts = open_comment_contexts(&state, current_head_oid.as_deref());
        let initial_diff = load_initial_diff(&self.repo_root, &self.target, &contexts).await?;
        let projected_threads = review_threads(&state, &self.author, &initial_diff).await?;
        let relocated_contexts =
            review_thread_contexts(&projected_threads, current_head_oid.as_deref());
        let diff = if relocated_contexts == contexts {
            initial_diff
        } else {
            load_relocated_diff(&self.repo_root, &self.target, &relocated_contexts).await?
        };
        review_payload(&state, diff, &self.target, &self.author, current_head_oid).await
    }

    pub async fn refresh_diff(&self) -> Result<ReviewProjection> {
        regenerate_outputs(&self.repo_root, Some(&self.target)).await?;
        let review = self.get_review().await?;
        self.updates.notify_diff_changed();
        Ok(review)
    }

    pub async fn create_thread(&self, request: CreateThreadRequest) -> Result<ReviewProjection> {
        let anchor = self
            .capture_anchor_evidence(request.clone().into_anchor()?)
            .await?;
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
                resolved_head_oid: None,
                collapsed: false,
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

    async fn capture_anchor_evidence(&self, anchor: CommentAnchor) -> Result<CommentAnchor> {
        let CommentAnchor::Line { mut line } = anchor else {
            return Ok(anchor);
        };
        let context = FileContextRequest {
            path: line.path.clone(),
            old_path: line.old_path.clone(),
            side: Some(line.side.clone()),
            start_line: Some(line.start_line),
            end_line: Some(line.end_line),
        };
        let diff =
            load_review_diff_with_contexts(&self.repo_root, &self.target, &[context]).await?;
        capture_line_anchor_for_diff(&mut line, &diff);
        Ok(CommentAnchor::Line { line })
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
        thread.resolved_head_oid = repo_current_head_oid(&self.repo_root).await?;
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
        thread.resolved_head_oid = None;
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

    pub async fn toggle_thread_collapsed(
        &self,
        request: ThreadRequest,
    ) -> Result<ReviewProjection> {
        let thread_id = ThreadId::new(request.thread_id)?;
        let mut thread = load_thread_payload(&self.repo_root, &thread_id).await?;
        let now = now_rfc3339()?;
        thread.collapsed = !thread.collapsed;
        thread.updated_at = now.clone();
        write_thread_payload(&self.repo_root, &thread).await?;
        let event = PeersEvent::ThreadCollapseUpdated {
            thread_id,
            updated_at: now,
            author: self.author.clone(),
            collapsed: thread.collapsed,
        };
        append_peers_event(&self.repo_root, &event, Some(&self.target)).await?;
        let review = self.get_review().await?;
        self.updates.notify_review_changed();
        Ok(review)
    }
}

#[instrument(name = "load_initial_diff", skip_all, fields(contexts = contexts.len()))]
async fn load_initial_diff(
    repo_root: &Path,
    target: &ReviewTarget,
    contexts: &[FileContextRequest],
) -> Result<ReviewDiffPayload> {
    load_review_diff_with_contexts(repo_root, target, contexts).await
}

#[instrument(name = "load_relocated_diff", skip_all, fields(contexts = contexts.len()))]
async fn load_relocated_diff(
    repo_root: &Path,
    target: &ReviewTarget,
    contexts: &[FileContextRequest],
) -> Result<ReviewDiffPayload> {
    load_review_diff_with_contexts(repo_root, target, contexts).await
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewProjection {
    pub review_id: String,
    pub target_label: String,
    pub current_head_oid: Option<String>,
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
    pub resolved_head_oid: Option<String>,
    pub collapsed: bool,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewThreadAnchor {
    pub side: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub placement: Option<String>,
    pub line_placements: Vec<ReviewThreadLinePlacement>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewThreadLinePlacement {
    pub original_line: Option<u32>,
    pub current_line: Option<u32>,
    pub placement: String,
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

async fn review_payload(
    state: &PeersState,
    mut diff: ReviewDiffPayload,
    target: &ReviewTarget,
    current_author: &Author,
    current_head_oid: Option<String>,
) -> Result<ReviewProjection> {
    let threads = review_threads(state, current_author, &diff).await?;
    let mut comment_counts = BTreeMap::<String, u32>::new();
    for thread in &threads {
        if thread_visible_in_default_projection(
            thread.resolved,
            thread.resolved_head_oid.as_deref(),
            current_head_oid.as_deref(),
        ) && let Some(path) = &thread.path
        {
            *comment_counts.entry(path.clone()).or_default() += 1;
        }
    }
    for file in &mut diff.files {
        file.comment_count = comment_counts.get(&file.path).copied().unwrap_or(0);
    }

    let review_threads = threads
        .iter()
        .filter(|thread| thread.scope == REVIEW_SCOPE)
        .cloned()
        .collect();

    Ok(ReviewProjection {
        review_id: "repo".to_string(),
        target_label: target.label(),
        current_head_oid,
        is_branch_review: target.is_branch(),
        files: diff.files,
        file_contents_by_path: diff.file_contents_by_path,
        file_diffs_by_path: diff.file_diffs_by_path,
        threads,
        review_threads,
        commits: Vec::new(),
    })
}

fn open_comment_contexts(
    state: &PeersState,
    current_head_oid: Option<&str>,
) -> Vec<FileContextRequest> {
    state
        .threads
        .values()
        .filter(|thread| {
            thread_visible_in_default_projection(
                thread.resolved,
                thread.resolved_head_oid.as_deref(),
                current_head_oid,
            ) && thread.archived_at.is_none()
                && thread.pruned_at.is_none()
        })
        .filter_map(|thread| match &thread.anchor {
            CommentAnchor::Line { line } => Some(FileContextRequest {
                path: line.path.clone(),
                old_path: line.old_path.clone(),
                side: Some(line.side.clone()),
                start_line: Some(line.start_line),
                end_line: Some(line.end_line),
            }),
            CommentAnchor::File { path } => Some(FileContextRequest {
                path: path.clone(),
                old_path: None,
                side: None,
                start_line: None,
                end_line: None,
            }),
            CommentAnchor::Review => None,
        })
        .collect()
}

fn capture_line_anchor_for_diff(line: &mut LineAnchor, diff: &ReviewDiffPayload) {
    if let Some(file) = diff.files.iter().find(|file| file.path == line.path)
        && line.old_path.is_none()
    {
        line.old_path = file.old_path.clone();
    }

    let lines = diff
        .file_contents_by_path
        .get(&line.path)
        .or_else(|| {
            line.old_path
                .as_ref()
                .and_then(|old_path| diff.file_contents_by_path.get(old_path))
        })
        .and_then(|content| match line.side {
            FileSide::Old => content.old.as_ref(),
            FileSide::New => content.new.as_ref(),
        });
    if let Some(lines) = lines {
        capture_line_anchor_evidence(line, lines, ANCHOR_CONTEXT_LINES);
    }
}

fn review_thread_contexts(
    threads: &[ReviewThread],
    current_head_oid: Option<&str>,
) -> Vec<FileContextRequest> {
    threads
        .iter()
        .filter(|thread| {
            thread_visible_in_default_projection(
                thread.resolved,
                thread.resolved_head_oid.as_deref(),
                current_head_oid,
            )
        })
        .filter_map(|thread| match thread.scope.as_str() {
            LINE_SCOPE => Some(FileContextRequest {
                path: thread.path.clone()?,
                old_path: None,
                side: thread.anchor.side.as_deref().and_then(file_side_from_name),
                start_line: thread.anchor.start_line,
                end_line: thread.anchor.end_line,
            }),
            FILE_SCOPE => Some(FileContextRequest {
                path: thread.path.clone()?,
                old_path: None,
                side: None,
                start_line: None,
                end_line: None,
            }),
            _ => None,
        })
        .collect()
}

#[instrument(
    name = "provider.review_threads",
    skip_all,
    fields(threads = state.threads.len(), files = diff.files.len())
)]
async fn review_threads(
    state: &PeersState,
    current_author: &Author,
    diff: &ReviewDiffPayload,
) -> Result<Vec<ReviewThread>> {
    let anchor_indexes = build_anchor_indexes(diff);
    relocate_threads(state, current_author, anchor_indexes).await
}

#[instrument(name = "anchor_indexes", skip_all)]
fn build_anchor_indexes(diff: &ReviewDiffPayload) -> ReviewAnchorIndexes {
    ReviewAnchorIndexes::new(diff)
}

#[instrument(name = "relocate_threads", skip_all)]
async fn relocate_threads(
    state: &PeersState,
    current_author: &Author,
    anchor_indexes: ReviewAnchorIndexes,
) -> Result<Vec<ReviewThread>> {
    let current_author = Arc::new(current_author.clone());
    let anchor_indexes = Arc::new(anchor_indexes);
    let handles = state
        .threads
        .values()
        .filter(|thread| thread.pruned_at.is_none() && thread.archived_at.is_none())
        .cloned()
        .map(|thread| {
            let current_author = Arc::clone(&current_author);
            let anchor_indexes = Arc::clone(&anchor_indexes);
            tokio::spawn(async move { review_thread(&thread, &current_author, &anchor_indexes) })
        })
        .collect::<Vec<_>>();

    let joined = join_all(handles).await;
    let mut threads = Vec::with_capacity(joined.len());
    for thread in joined {
        threads.push(thread?);
    }
    Ok(threads)
}

fn review_thread(
    thread: &CommentThread,
    current_author: &Author,
    anchor_indexes: &ReviewAnchorIndexes,
) -> ReviewThread {
    let (scope, path, anchor) = match &thread.anchor {
        CommentAnchor::Line { line } => {
            let relocated = relocate_review_line_anchor(line, anchor_indexes);
            (
                LINE_SCOPE.to_string(),
                relocated.path.clone(),
                ReviewThreadAnchor {
                    side: Some(file_side_name(&line.side).to_string()),
                    start_line: relocated.start_line,
                    end_line: relocated.end_line,
                    placement: Some(anchor_placement_name(relocated.placement).to_string()),
                    line_placements: relocated
                        .line_placements
                        .iter()
                        .map(review_thread_line_placement)
                        .collect(),
                },
            )
        }
        CommentAnchor::File { path } => (
            FILE_SCOPE.to_string(),
            Some(path.clone()),
            ReviewThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
                placement: Some("file".to_string()),
                line_placements: Vec::new(),
            },
        ),
        CommentAnchor::Review => (
            REVIEW_SCOPE.to_string(),
            None,
            ReviewThreadAnchor {
                side: None,
                start_line: None,
                end_line: None,
                placement: Some("review".to_string()),
                line_placements: Vec::new(),
            },
        ),
    };

    ReviewThread {
        id: thread.id.to_string(),
        scope,
        line_label: relocated_thread_label(&thread.anchor, path.as_deref(), &anchor),
        path,
        anchor,
        resolved: thread.resolved,
        resolved_head_oid: thread.resolved_head_oid.clone(),
        collapsed: thread.collapsed,
        comments: thread
            .comments
            .iter()
            .filter(|comment| comment.deleted_at.is_none())
            .map(|comment| review_comment(comment, current_author))
            .collect(),
    }
}

fn relocate_review_line_anchor(
    line: &LineAnchor,
    anchor_indexes: &ReviewAnchorIndexes,
) -> RelocatedLineAnchor {
    relocate_line_anchor_in_index(line, anchor_indexes.for_side(&line.side))
}

#[derive(Clone)]
struct ReviewAnchorIndexes {
    old: AnchorIndex,
    new: AnchorIndex,
}

impl ReviewAnchorIndexes {
    fn new(diff: &ReviewDiffPayload) -> Self {
        let mut old_files = BTreeMap::new();
        let mut new_files = BTreeMap::new();
        for file in &diff.files {
            let Some(content) = diff.file_contents_by_path.get(&file.path) else {
                continue;
            };
            if let Some(lines) = &content.old {
                old_files.insert(file.path.clone(), lines.clone());
                if let Some(old_path) = &file.old_path {
                    old_files.insert(old_path.clone(), lines.clone());
                }
            }
            if let Some(lines) = &content.new {
                new_files.insert(file.path.clone(), lines.clone());
            }
        }
        Self {
            old: AnchorIndex::new(old_files),
            new: AnchorIndex::new(new_files),
        }
    }

    fn for_side(&self, side: &FileSide) -> &AnchorIndex {
        match side {
            FileSide::Old => &self.old,
            FileSide::New => &self.new,
        }
    }
}

fn review_thread_line_placement(
    line: &crate::anchors::RelocatedAnchorLine,
) -> ReviewThreadLinePlacement {
    ReviewThreadLinePlacement {
        original_line: line.original_line,
        current_line: line.current_line,
        placement: anchor_line_placement_name(line.placement).to_string(),
    }
}

fn relocated_thread_label(
    original: &CommentAnchor,
    relocated_path: Option<&str>,
    anchor: &ReviewThreadAnchor,
) -> String {
    let (Some(path), Some(start_line), Some(end_line)) =
        (relocated_path, anchor.start_line, anchor.end_line)
    else {
        return original.label();
    };
    if start_line == end_line {
        format!("{path}:{start_line}")
    } else {
        format!("{path}:{start_line}-{end_line}")
    }
}

fn anchor_placement_name(placement: AnchorPlacement) -> &'static str {
    match placement {
        AnchorPlacement::Exact => "exact",
        AnchorPlacement::PerLineHash => "per_line_hash",
        AnchorPlacement::Context => "context",
        AnchorPlacement::MovedExact => "moved_exact",
        AnchorPlacement::Window => "window",
        AnchorPlacement::LineFallback => "line_fallback",
        AnchorPlacement::FileFallback => "file_fallback",
        AnchorPlacement::Detached => "detached",
    }
}

fn anchor_line_placement_name(placement: AnchorLinePlacement) -> &'static str {
    match placement {
        AnchorLinePlacement::Exact => "exact",
        AnchorLinePlacement::Content => "content",
        AnchorLinePlacement::Context => "context",
        AnchorLinePlacement::Changed => "changed",
        AnchorLinePlacement::Gap => "gap",
        AnchorLinePlacement::LineFallback => "line_fallback",
        AnchorLinePlacement::Missing => "missing",
        AnchorLinePlacement::Detached => "detached",
    }
}

pub(crate) fn thread_visible_in_default_projection(
    resolved: bool,
    resolved_head_oid: Option<&str>,
    current_head_oid: Option<&str>,
) -> bool {
    if !resolved {
        return true;
    }

    resolved_head_oid.is_some()
        && current_head_oid.is_some()
        && resolved_head_oid == current_head_oid
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

fn file_side_from_name(side: &str) -> Option<FileSide> {
    match side {
        OLD_FILE_SIDE => Some(FileSide::Old),
        NEW_FILE_SIDE => Some(FileSide::New),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchors::capture_line_anchor_evidence;
    use crate::comments::{AuthorKind, PeersTimestamp};
    use crate::diff::{DiffHunk, FileContent, FileDiff, FileStatus};

    #[tokio::test]
    async fn projection_uses_relocated_line_anchor_metadata() {
        let base_lines = lines(&[
            "fn configure() {",
            "    let first = load();",
            "    let second = prepare();",
            "    finish();",
            "}",
        ]);
        let mut line_anchor = LineAnchor::new("src/config.rs".to_string(), FileSide::New, 2, 3);
        capture_line_anchor_evidence(&mut line_anchor, &base_lines, 1);

        let thread_id = ThreadId::from_raw("thr_test");
        let comment_id = CommentId::from_raw("cmt_test");
        let timestamp = PeersTimestamp::from_rfc3339_unchecked("2026-05-28T12:00:00Z");
        let author = Author {
            kind: AuthorKind::Human,
            display_name: "jonas".to_string(),
            email: None,
        };
        let state = PeersState {
            threads: BTreeMap::from([(
                thread_id.clone(),
                CommentThread {
                    id: thread_id.clone(),
                    anchor: CommentAnchor::Line { line: line_anchor },
                    comments: vec![Comment {
                        id: comment_id,
                        thread_id,
                        author: author.clone(),
                        body: "Check this range".to_string(),
                        created_at: timestamp.clone(),
                        edited_at: None,
                        deleted_at: None,
                    }],
                    resolved: false,
                    resolved_head_oid: None,
                    collapsed: false,
                    created_at: timestamp.clone(),
                    updated_at: timestamp,
                    archived_at: None,
                    pruned_at: None,
                },
            )]),
        };
        let diff = ReviewDiffPayload {
            files: vec![ReviewFile {
                path: "src/config.rs".to_string(),
                old_path: None,
                status: FileStatus::Unchanged,
                is_changed: false,
                comment_count: 0,
                added_lines: 0,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/config.rs".to_string(),
                FileContent {
                    old: None,
                    new: Some(lines(&[
                        "fn configure() {",
                        "    let inserted = true;",
                        "    let first = load();",
                        "    let second = prepare();",
                        "    finish();",
                        "}",
                    ])),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/config.rs".to_string(),
                FileDiff {
                    path: "src/config.rs".to_string(),
                    hunks: Vec::<DiffHunk>::new(),
                },
            )]),
        };

        let threads = review_threads(&state, &author, &diff).await.unwrap();

        assert_eq!(threads.len(), 1);
        let thread = &threads[0];
        assert_eq!(thread.path.as_deref(), Some("src/config.rs"));
        assert_eq!(thread.line_label, "src/config.rs:3-4");
        assert_eq!(thread.anchor.start_line, Some(3));
        assert_eq!(thread.anchor.end_line, Some(4));
        assert_eq!(thread.anchor.placement.as_deref(), Some("exact"));
        let line_placements: Vec<_> = thread
            .anchor
            .line_placements
            .iter()
            .map(|line| {
                (
                    line.original_line,
                    line.current_line,
                    line.placement.as_str(),
                )
            })
            .collect();
        assert_eq!(
            line_placements,
            vec![(Some(2), Some(3), "exact"), (Some(3), Some(4), "exact")]
        );
    }

    #[test]
    fn captures_line_anchor_evidence_from_current_diff_content() {
        let mut line_anchor = LineAnchor::new("src/config.rs".to_string(), FileSide::New, 2, 3);
        let diff = ReviewDiffPayload {
            files: vec![ReviewFile {
                path: "src/config.rs".to_string(),
                old_path: None,
                status: FileStatus::Unchanged,
                is_changed: false,
                comment_count: 0,
                added_lines: 0,
                removed_lines: 0,
            }],
            file_contents_by_path: BTreeMap::from([(
                "src/config.rs".to_string(),
                FileContent {
                    old: None,
                    new: Some(lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    let second = prepare();",
                        "    finish();",
                        "}",
                    ])),
                },
            )]),
            file_diffs_by_path: BTreeMap::from([(
                "src/config.rs".to_string(),
                FileDiff {
                    path: "src/config.rs".to_string(),
                    hunks: Vec::<DiffHunk>::new(),
                },
            )]),
        };

        capture_line_anchor_for_diff(&mut line_anchor, &diff);

        assert_eq!(
            line_anchor.selected_text.as_deref(),
            Some("    let first = load();\n    let second = prepare();")
        );
        assert_eq!(line_anchor.per_line_hashes.len(), 2);
        assert_eq!(
            line_anchor.context_before,
            vec!["fn configure() {".to_string()]
        );
        assert_eq!(
            line_anchor.context_after,
            vec!["    finish();".to_string(), "}".to_string()]
        );
        assert!(line_anchor.selected_range_hash.is_some());
        assert!(line_anchor.context_before_hash.is_some());
        assert!(line_anchor.context_after_hash.is_some());
    }

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|line| line.to_string()).collect()
    }
}
