use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use facet::Facet;
use gix_diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};

const DIFF_CONTEXT_LINES: u32 = 3;
const HEAD_REF: &str = "HEAD";
const PATH_SEPARATOR: char = '/';
const BINARY_NUL: u8 = 0;
const EMPTY_PATH_ERROR: &str = "repository path is not valid UTF-8";
const GIT_DISCOVER_ERROR: &str = "failed to open Git repository for diff";
const MERGE_BASE_ERROR: &str = "failed to resolve branch merge base";
const DIFF_TASK_ERROR: &str = "failed to join Git diff loader";
const BRANCH_BASE_RESOLVE_ERROR: &str = "failed to resolve branch base";
const BRANCH_HEAD_RESOLVE_ERROR: &str = "failed to resolve branch head";
const REV_RESOLVE_ERROR: &str = "failed to resolve revision";

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
#[facet(rename_all = "snake_case")]
pub enum ReviewTarget {
    WorkingTree,
    Cached,
    All,
    Branch { base: String, head: String },
}

impl ReviewTarget {
    pub fn label(&self) -> String {
        match self {
            Self::WorkingTree => "working tree".to_string(),
            Self::Cached => "cached".to_string(),
            Self::All => "all current changes".to_string(),
            Self::Branch { base, head } => format!("{base}..{head}"),
        }
    }

    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch { .. })
    }

    pub fn is_local_diff(&self) -> bool {
        matches!(self, Self::WorkingTree | Self::Cached | Self::All)
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum FileSide {
    Old,
    New,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct LineAnchor {
    pub path: String,
    pub old_path: Option<String>,
    pub side: FileSide,
    pub start_line: u32,
    pub end_line: u32,
    pub hunk_header: Option<String>,
    pub selected_text: Option<String>,
    pub selected_text_hash: Option<String>,
    pub selected_range_hash: Option<String>,
    pub per_line_hashes: Vec<String>,
    pub context_before: Vec<String>,
    pub context_before_hash: Option<String>,
    pub context_after: Vec<String>,
    pub context_after_hash: Option<String>,
    pub nearby_context_hash: Option<String>,
    pub view_kind: Option<String>,
    pub branch: Option<String>,
    pub merge_base_oid: Option<String>,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
}

impl LineAnchor {
    pub fn new(path: String, side: FileSide, start_line: u32, end_line: u32) -> Self {
        Self {
            path,
            old_path: None,
            side,
            start_line,
            end_line,
            hunk_header: None,
            selected_text: None,
            selected_text_hash: None,
            selected_range_hash: None,
            per_line_hashes: Vec::new(),
            context_before: Vec::new(),
            context_before_hash: None,
            context_after: Vec::new(),
            context_after_hash: None,
            nearby_context_hash: None,
            view_kind: None,
            branch: None,
            merge_base_oid: None,
            base_oid: None,
            head_oid: None,
        }
    }

    pub fn line_label(&self) -> String {
        if self.start_line == self.end_line {
            format!("{}:{}", self.path, self.start_line)
        } else {
            format!("{}:{}-{}", self.path, self.start_line, self.end_line)
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
#[facet(tag = "scope", rename_all = "snake_case")]
pub enum CommentAnchor {
    Line { line: LineAnchor },
    File { path: String },
    Review,
}

impl CommentAnchor {
    pub fn label(&self) -> String {
        match self {
            Self::Line { line } => line.line_label(),
            Self::File { path } => format!("{path} file"),
            Self::Review => "Review".to_string(),
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewDiffPayload {
    pub files: Vec<ReviewFile>,
    pub file_contents_by_path: BTreeMap<String, FileContent>,
    pub file_diffs_by_path: BTreeMap<String, FileDiff>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ReviewFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub is_changed: bool,
    pub comment_count: u32,
    pub added_lines: u32,
    pub removed_lines: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileContextRequest {
    pub path: String,
    pub old_path: Option<String>,
    pub side: Option<FileSide>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Clone, Copy, Debug, Facet, PartialEq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Unchanged,
    Binary,
}

#[derive(Clone, Debug, Default, Facet, PartialEq)]
pub struct FileContent {
    pub old: Option<Vec<String>>,
    pub new: Option<Vec<String>>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct DiffHunk {
    pub old: Option<LineRange>,
    pub new: Option<LineRange>,
    pub sections: Vec<DiffSection>,
}

#[derive(Clone, Copy, Debug, Facet, PartialEq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
#[facet(tag = "kind", rename_all = "snake_case")]
pub enum DiffSection {
    Context { context: PairedRange },
    Added { added: NewRange },
    Removed { removed: OldRange },
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct PairedRange {
    pub old: LineRange,
    pub new: LineRange,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct NewRange {
    pub new: LineRange,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct OldRange {
    pub old: LineRange,
}

#[derive(Clone)]
enum ContentSource {
    Worktree,
    Index,
    Commit { rev: String, allow_missing: bool },
}

pub async fn load_review_diff(
    repo_root: &Path,
    target: &ReviewTarget,
) -> Result<ReviewDiffPayload> {
    load_review_diff_with_contexts(repo_root, target, &[]).await
}

pub async fn load_review_diff_with_contexts(
    repo_root: &Path,
    target: &ReviewTarget,
    contexts: &[FileContextRequest],
) -> Result<ReviewDiffPayload> {
    let repo_root = repo_root.to_path_buf();
    let target = target.clone();
    let contexts = contexts.to_vec();
    tokio::task::spawn_blocking(move || load_review_diff_sync(&repo_root, &target, &contexts, None))
        .await
        .context(DIFF_TASK_ERROR)?
}

pub async fn load_review_diff_for_context(
    repo_root: &Path,
    target: &ReviewTarget,
    context: &FileContextRequest,
) -> Result<ReviewDiffPayload> {
    let repo_root = repo_root.to_path_buf();
    let target = target.clone();
    let context = context.clone();
    tokio::task::spawn_blocking(move || {
        let contexts = vec![context];
        let paths = context_paths(&contexts);
        load_review_diff_sync(&repo_root, &target, &contexts, Some(&paths))
    })
    .await
    .context(DIFF_TASK_ERROR)?
}

fn load_review_diff_sync(
    repo_root: &Path,
    target: &ReviewTarget,
    contexts: &[FileContextRequest],
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<ReviewDiffPayload> {
    let repo = gix::discover(repo_root).context(GIT_DISCOVER_ERROR)?;
    let resolved = ResolvedTarget::resolve(&repo, target)?;
    let context_paths = context_paths(contexts);
    let old_snapshot = snapshot_for_source(
        repo_root,
        &repo,
        &resolved.old_source,
        &context_paths,
        selected_paths,
    )?;
    let old_paths = old_snapshot
        .keys()
        .chain(context_paths.iter())
        .cloned()
        .collect();
    let new_snapshot = snapshot_for_source(
        repo_root,
        &repo,
        &resolved.new_source,
        &old_paths,
        selected_paths,
    )?;
    let entries = diff_entries(&old_snapshot, &new_snapshot)?;
    let mut files = Vec::new();
    let mut file_contents_by_path = BTreeMap::new();
    let mut file_diffs_by_path = BTreeMap::new();

    for entry in entries {
        let path = entry.path;
        let old_path = entry.old_path;
        let mut status = entry.status;
        let binary = is_binary(entry.old.as_deref()) || is_binary(entry.new.as_deref());
        if binary {
            status = FileStatus::Binary;
        }
        let file_diff = if binary {
            FileDiff {
                path: path.clone(),
                hunks: Vec::new(),
            }
        } else {
            build_file_diff(&path, entry.old.as_deref(), entry.new.as_deref())?
        };
        let added_lines = added_lines(&file_diff);
        let removed_lines = removed_lines(&file_diff);

        file_contents_by_path.insert(
            path.clone(),
            FileContent {
                old: if matches!(status, FileStatus::Added) {
                    None
                } else {
                    entry.old.as_deref().and_then(bytes_to_lines)
                },
                new: if matches!(status, FileStatus::Deleted) {
                    None
                } else {
                    entry.new.as_deref().and_then(bytes_to_lines)
                },
            },
        );
        file_diffs_by_path.insert(path.clone(), file_diff.clone());
        files.push(ReviewFile {
            path,
            old_path,
            status,
            is_changed: true,
            comment_count: 0,
            added_lines,
            removed_lines,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut payload = ReviewDiffPayload {
        files,
        file_contents_by_path,
        file_diffs_by_path,
    };
    apply_file_contexts(&mut payload, &old_snapshot, &new_snapshot, contexts);
    Ok(payload)
}

fn context_paths(contexts: &[FileContextRequest]) -> BTreeSet<String> {
    contexts
        .iter()
        .flat_map(|context| std::iter::once(context.path.clone()).chain(context.old_path.clone()))
        .collect()
}

struct ResolvedTarget {
    old_source: ContentSource,
    new_source: ContentSource,
}

impl ResolvedTarget {
    fn resolve(repo: &gix::Repository, target: &ReviewTarget) -> Result<Self> {
        match target {
            ReviewTarget::WorkingTree => Ok(Self {
                old_source: ContentSource::Index,
                new_source: ContentSource::Worktree,
            }),
            ReviewTarget::Cached => Ok(Self {
                old_source: ContentSource::Commit {
                    rev: HEAD_REF.to_string(),
                    allow_missing: true,
                },
                new_source: ContentSource::Index,
            }),
            ReviewTarget::All => Ok(Self {
                old_source: ContentSource::Commit {
                    rev: HEAD_REF.to_string(),
                    allow_missing: true,
                },
                new_source: ContentSource::Worktree,
            }),
            ReviewTarget::Branch { base, head } => {
                let base_id = repo
                    .rev_parse_single(base.as_str())
                    .with_context(|| format!("{BRANCH_BASE_RESOLVE_ERROR} `{base}`"))?;
                let head_id = repo
                    .rev_parse_single(head.as_str())
                    .with_context(|| format!("{BRANCH_HEAD_RESOLVE_ERROR} `{head}`"))?;
                let merge_base = repo
                    .merge_base(base_id.detach(), head_id.detach())
                    .context(MERGE_BASE_ERROR)?
                    .to_string();
                Ok(Self {
                    old_source: ContentSource::Commit {
                        rev: merge_base,
                        allow_missing: false,
                    },
                    new_source: ContentSource::Commit {
                        rev: head.clone(),
                        allow_missing: false,
                    },
                })
            }
        }
    }
}

fn snapshot_for_source(
    repo_root: &Path,
    repo: &gix::Repository,
    source: &ContentSource,
    extra_worktree_paths: &BTreeSet<String>,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    match source {
        ContentSource::Worktree => {
            worktree_snapshot(repo_root, repo, extra_worktree_paths, selected_paths)
        }
        ContentSource::Index => index_snapshot(repo, selected_paths),
        ContentSource::Commit { rev, allow_missing } => {
            commit_snapshot(repo, rev, *allow_missing, selected_paths)
        }
    }
}

fn index_snapshot(
    repo: &gix::Repository,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let index = repo.index_or_empty()?;
    let mut snapshot = BTreeMap::new();
    for entry in index.entries() {
        if entry.stage_raw() != 0 {
            continue;
        }
        let path = bstr_path_to_string(entry.path(&index))?;
        if selected_paths.is_some_and(|selected_paths| !selected_paths.contains(&path)) {
            continue;
        }
        let blob = repo.find_blob(entry.id)?;
        snapshot.insert(path, blob.data.to_vec());
    }
    Ok(snapshot)
}

fn commit_snapshot(
    repo: &gix::Repository,
    rev: &str,
    allow_missing: bool,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let id = match repo.rev_parse_single(rev) {
        Ok(id) => id,
        Err(error) if allow_missing => {
            let _ = error;
            return Ok(BTreeMap::new());
        }
        Err(error) => return Err(error).with_context(|| format!("{REV_RESOLVE_ERROR} `{rev}`")),
    };
    let tree_id = id.object()?.peel_to_commit()?.tree_id()?;
    let index = repo.index_from_tree(&tree_id)?;
    let mut snapshot = BTreeMap::new();
    for entry in index.entries() {
        if entry.stage_raw() != 0 {
            continue;
        }
        let path = bstr_path_to_string(entry.path(&index))?;
        if selected_paths.is_some_and(|selected_paths| !selected_paths.contains(&path)) {
            continue;
        }
        let blob = repo.find_blob(entry.id)?;
        snapshot.insert(path, blob.data.to_vec());
    }
    Ok(snapshot)
}

fn worktree_snapshot(
    repo_root: &Path,
    repo: &gix::Repository,
    extra_paths: &BTreeSet<String>,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let paths = worktree_candidate_paths(repo, extra_paths, selected_paths)?;
    let mut snapshot = BTreeMap::new();
    for path in paths {
        let full_path = repo_root.join(path_to_platform(&path));
        match std::fs::read(full_path) {
            Ok(bytes) => {
                snapshot.insert(path, bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(snapshot)
}

fn worktree_candidate_paths(
    repo: &gix::Repository,
    extra_paths: &BTreeSet<String>,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<BTreeSet<String>> {
    let indexed_paths = index_paths(repo)?;
    Ok(worktree_candidate_paths_from_index(
        indexed_paths,
        extra_paths,
        selected_paths,
    ))
}

fn worktree_candidate_paths_from_index(
    indexed_paths: BTreeSet<String>,
    extra_paths: &BTreeSet<String>,
    selected_paths: Option<&BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut paths = match selected_paths {
        Some(selected_paths) => selected_paths.clone(),
        None => indexed_paths,
    };
    paths.extend(extra_paths.iter().cloned());
    paths
}

fn index_paths(repo: &gix::Repository) -> Result<BTreeSet<String>> {
    let index = repo.index_or_empty()?;
    let mut paths = BTreeSet::new();
    for entry in index.entries() {
        if entry.stage_raw() != 0 {
            continue;
        }
        paths.insert(bstr_path_to_string(entry.path(&index))?);
    }
    Ok(paths)
}

fn path_to_platform(path: &str) -> PathBuf {
    path.split(PATH_SEPARATOR).collect()
}

fn bstr_path_to_string(path: &gix::bstr::BStr) -> Result<String> {
    String::from_utf8(path.to_vec()).context(EMPTY_PATH_ERROR)
}

fn bytes_to_lines(bytes: &[u8]) -> Option<Vec<String>> {
    if is_binary(Some(bytes)) {
        return None;
    }
    Some(split_lines(&String::from_utf8_lossy(bytes)))
}

fn split_lines(input: &str) -> Vec<String> {
    input
        .strip_suffix('\n')
        .unwrap_or(input)
        .split('\n')
        .map(str::to_string)
        .collect()
}

struct DiffEntry {
    path: String,
    old_path: Option<String>,
    status: FileStatus,
    old: Option<Vec<u8>>,
    new: Option<Vec<u8>>,
}

fn diff_entries(
    old_snapshot: &BTreeMap<String, Vec<u8>>,
    new_snapshot: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<DiffEntry>> {
    let paths = old_snapshot
        .keys()
        .chain(new_snapshot.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();

    for path in paths {
        let old = old_snapshot.get(&path).cloned();
        let new = new_snapshot.get(&path).cloned();
        if old == new {
            continue;
        }
        let status = match (&old, &new) {
            (None, Some(_)) => FileStatus::Added,
            (Some(_), None) => FileStatus::Deleted,
            (Some(_), Some(_)) => FileStatus::Modified,
            (None, None) => continue,
        };
        entries.push(DiffEntry {
            path,
            old_path: None,
            status,
            old,
            new,
        });
    }

    Ok(coalesce_exact_renames(entries))
}

fn apply_file_contexts(
    payload: &mut ReviewDiffPayload,
    old_snapshot: &BTreeMap<String, Vec<u8>>,
    new_snapshot: &BTreeMap<String, Vec<u8>>,
    contexts: &[FileContextRequest],
) {
    for context in contexts {
        ensure_context_file(payload, old_snapshot, new_snapshot, context);

        let (Some(side), Some(start_line)) = (&context.side, context.start_line) else {
            continue;
        };
        let end_line = context.end_line.unwrap_or(start_line).max(start_line);
        let Some(diff) = payload.file_diffs_by_path.get_mut(&context.path) else {
            continue;
        };
        if diff_covers_range(diff, side, start_line, end_line) {
            continue;
        }

        let line_count = match side {
            FileSide::Old => old_snapshot
                .get(context.old_path.as_ref().unwrap_or(&context.path))
                .and_then(|bytes| bytes_to_lines(bytes))
                .map_or(0, |lines| lines.len() as u32),
            FileSide::New => new_snapshot
                .get(&context.path)
                .and_then(|bytes| bytes_to_lines(bytes))
                .map_or(0, |lines| lines.len() as u32),
        };
        let window = context_window(start_line, end_line, line_count);
        diff.hunks.push(context_hunk(side.clone(), window));
    }

    payload
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
}

fn ensure_context_file(
    payload: &mut ReviewDiffPayload,
    old_snapshot: &BTreeMap<String, Vec<u8>>,
    new_snapshot: &BTreeMap<String, Vec<u8>>,
    context: &FileContextRequest,
) {
    if payload.file_diffs_by_path.contains_key(&context.path) {
        return;
    }

    let old_bytes = context
        .old_path
        .as_ref()
        .and_then(|path| old_snapshot.get(path))
        .or_else(|| old_snapshot.get(&context.path));
    let new_bytes = new_snapshot.get(&context.path);
    let content = FileContent {
        old: old_bytes.and_then(|bytes| bytes_to_lines(bytes)),
        new: new_bytes.and_then(|bytes| bytes_to_lines(bytes)),
    };
    let status = match (old_bytes, new_bytes) {
        (Some(_), None) => FileStatus::Deleted,
        (None, Some(_)) => FileStatus::Added,
        _ => FileStatus::Unchanged,
    };

    payload
        .file_contents_by_path
        .insert(context.path.clone(), content);
    payload.file_diffs_by_path.insert(
        context.path.clone(),
        FileDiff {
            path: context.path.clone(),
            hunks: Vec::new(),
        },
    );
    payload.files.push(ReviewFile {
        path: context.path.clone(),
        old_path: context.old_path.clone(),
        status,
        is_changed: false,
        comment_count: 0,
        added_lines: 0,
        removed_lines: 0,
    });
}

fn diff_covers_range(diff: &FileDiff, side: &FileSide, start_line: u32, end_line: u32) -> bool {
    (start_line..=end_line).all(|line| diff_covers_line(diff, side, line))
}

fn diff_covers_line(diff: &FileDiff, side: &FileSide, line: u32) -> bool {
    diff.hunks.iter().any(|hunk| {
        hunk.sections.iter().any(|section| match (side, section) {
            (FileSide::New, DiffSection::Context { context }) => range_contains(context.new, line),
            (FileSide::New, DiffSection::Added { added }) => range_contains(added.new, line),
            (FileSide::Old, DiffSection::Context { context }) => range_contains(context.old, line),
            (FileSide::Old, DiffSection::Removed { removed }) => range_contains(removed.old, line),
            _ => false,
        })
    })
}

fn range_contains(range: LineRange, line: u32) -> bool {
    line >= range.start && line <= range.end
}

fn context_window(start_line: u32, end_line: u32, line_count: u32) -> LineRange {
    let start_line = start_line.max(1);
    let end_line = end_line.max(start_line);
    let window_start = start_line.saturating_sub(DIFF_CONTEXT_LINES).max(1);
    let window_end = end_line.saturating_add(DIFF_CONTEXT_LINES).max(end_line);
    let window_end = if line_count == 0 {
        window_end
    } else {
        window_end.min(line_count.max(end_line))
    };
    LineRange {
        start: window_start,
        end: window_end,
    }
}

fn context_hunk(side: FileSide, window: LineRange) -> DiffHunk {
    match side {
        FileSide::New => DiffHunk {
            old: Some(window),
            new: Some(window),
            sections: vec![DiffSection::Context {
                context: PairedRange {
                    old: window,
                    new: window,
                },
            }],
        },
        FileSide::Old => DiffHunk {
            old: Some(window),
            new: None,
            sections: vec![DiffSection::Removed {
                removed: OldRange { old: window },
            }],
        },
    }
}

fn coalesce_exact_renames(entries: Vec<DiffEntry>) -> Vec<DiffEntry> {
    let mut additions = Vec::new();
    let mut deletions = Vec::new();
    let mut others = Vec::new();

    for entry in entries {
        match entry.status {
            FileStatus::Added => additions.push(entry),
            FileStatus::Deleted => deletions.push(entry),
            _ => others.push(entry),
        }
    }

    let mut used_additions = BTreeSet::new();
    let mut coalesced = Vec::new();
    for deletion in deletions {
        let Some(old_bytes) = deletion.old.as_ref() else {
            coalesced.push(deletion);
            continue;
        };
        let addition = additions
            .iter()
            .enumerate()
            .find(|(index, addition)| {
                !used_additions.contains(index) && addition.new.as_ref() == Some(old_bytes)
            })
            .map(|(index, addition)| (index, addition));
        if let Some((index, addition)) = addition {
            used_additions.insert(index);
            coalesced.push(DiffEntry {
                path: addition.path.clone(),
                old_path: Some(deletion.path),
                status: FileStatus::Renamed,
                old: deletion.old,
                new: addition.new.clone(),
            });
        } else {
            coalesced.push(deletion);
        }
    }

    for (index, addition) in additions.into_iter().enumerate() {
        if !used_additions.contains(&index) {
            coalesced.push(addition);
        }
    }

    coalesced.extend(others);
    coalesced.sort_by(|left, right| left.path.cmp(&right.path));
    coalesced
}

fn build_file_diff(path: &str, old: Option<&[u8]>, new: Option<&[u8]>) -> Result<FileDiff> {
    let old = old.unwrap_or_default();
    let new = new.unwrap_or_default();
    let input = gix_diff::blob::InternedInput::new(
        gix_diff::blob::sources::byte_lines(old),
        gix_diff::blob::sources::byte_lines(new),
    );
    let diff =
        gix_diff::blob::diff_with_slider_heuristics(gix_diff::blob::Algorithm::Histogram, &input);
    let collector = HunkCollector::default();
    let hunks = gix_diff::blob::UnifiedDiff::new(
        &diff,
        &input,
        collector,
        ContextSize::symmetrical(DIFF_CONTEXT_LINES),
    )
    .consume()?;

    Ok(FileDiff {
        path: path.to_string(),
        hunks,
    })
}

#[derive(Default)]
struct HunkCollector {
    hunks: Vec<DiffHunk>,
}

impl ConsumeHunk for HunkCollector {
    type Out = Vec<DiffHunk>;

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        let mut hunk = DiffHunk {
            old: Some(hunk_range(header.before_hunk_start, header.before_hunk_len)),
            new: Some(hunk_range(header.after_hunk_start, header.after_hunk_len)),
            sections: Vec::new(),
        };
        let mut section = None;
        let mut old_line = header.before_hunk_start;
        let mut new_line = header.after_hunk_start;

        for (kind, _) in lines {
            match kind {
                DiffLineKind::Context => {
                    append_section_to_hunk(
                        &mut hunk,
                        &mut section,
                        SectionKind::Context,
                        Some(old_line),
                        Some(new_line),
                    );
                    old_line += 1;
                    new_line += 1;
                }
                DiffLineKind::Add => {
                    append_section_to_hunk(
                        &mut hunk,
                        &mut section,
                        SectionKind::Added,
                        None,
                        Some(new_line),
                    );
                    new_line += 1;
                }
                DiffLineKind::Remove => {
                    append_section_to_hunk(
                        &mut hunk,
                        &mut section,
                        SectionKind::Removed,
                        Some(old_line),
                        None,
                    );
                    old_line += 1;
                }
            }
        }

        if let Some(section) = section.take() {
            hunk.sections.push(section.finish());
        }
        self.hunks.push(hunk);
        Ok(())
    }

    fn finish(self) -> Self::Out {
        self.hunks
    }
}

fn append_section_to_hunk(
    hunk: &mut DiffHunk,
    current_section: &mut Option<SectionBuilder>,
    kind: SectionKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
) {
    let same_kind = current_section.as_ref().is_some_and(|section| {
        std::mem::discriminant(&section.kind) == std::mem::discriminant(&kind)
    });
    if same_kind {
        if let Some(section) = current_section {
            section.extend(old_line, new_line);
        }
        return;
    }

    if let Some(section) = current_section.take() {
        hunk.sections.push(section.finish());
    }
    *current_section = Some(match kind {
        SectionKind::Context => {
            SectionBuilder::context(old_line.unwrap_or(1), new_line.unwrap_or(1))
        }
        SectionKind::Added => SectionBuilder::added(new_line.unwrap_or(1)),
        SectionKind::Removed => SectionBuilder::removed(old_line.unwrap_or(1)),
    });
}

fn hunk_range(start: u32, len: u32) -> LineRange {
    LineRange {
        start,
        end: start.saturating_add(len).saturating_sub(1),
    }
}

fn is_binary(bytes: Option<&[u8]>) -> bool {
    bytes.is_some_and(|bytes| bytes.contains(&BINARY_NUL))
}

#[derive(Clone, Copy, Debug)]
enum SectionKind {
    Context,
    Added,
    Removed,
}

struct SectionBuilder {
    kind: SectionKind,
    old_start: Option<u32>,
    old_end: Option<u32>,
    new_start: Option<u32>,
    new_end: Option<u32>,
}

impl SectionBuilder {
    fn context(old_line: u32, new_line: u32) -> Self {
        Self {
            kind: SectionKind::Context,
            old_start: Some(old_line),
            old_end: Some(old_line),
            new_start: Some(new_line),
            new_end: Some(new_line),
        }
    }

    fn added(new_line: u32) -> Self {
        Self {
            kind: SectionKind::Added,
            old_start: None,
            old_end: None,
            new_start: Some(new_line),
            new_end: Some(new_line),
        }
    }

    fn removed(old_line: u32) -> Self {
        Self {
            kind: SectionKind::Removed,
            old_start: Some(old_line),
            old_end: Some(old_line),
            new_start: None,
            new_end: None,
        }
    }

    fn extend(&mut self, old_line: Option<u32>, new_line: Option<u32>) {
        if let Some(old_line) = old_line {
            self.old_end = Some(old_line);
        }
        if let Some(new_line) = new_line {
            self.new_end = Some(new_line);
        }
    }

    fn finish(self) -> DiffSection {
        match self.kind {
            SectionKind::Context => DiffSection::Context {
                context: PairedRange {
                    old: LineRange {
                        start: self.old_start.unwrap_or(1),
                        end: self.old_end.unwrap_or(1),
                    },
                    new: LineRange {
                        start: self.new_start.unwrap_or(1),
                        end: self.new_end.unwrap_or(1),
                    },
                },
            },
            SectionKind::Added => DiffSection::Added {
                added: NewRange {
                    new: LineRange {
                        start: self.new_start.unwrap_or(1),
                        end: self.new_end.unwrap_or(1),
                    },
                },
            },
            SectionKind::Removed => DiffSection::Removed {
                removed: OldRange {
                    old: LineRange {
                        start: self.old_start.unwrap_or(1),
                        end: self.old_end.unwrap_or(1),
                    },
                },
            },
        }
    }
}

fn added_lines(file_diff: &FileDiff) -> u32 {
    file_diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.sections)
        .map(|section| match section {
            DiffSection::Added { added } => range_len(added.new),
            _ => 0,
        })
        .sum()
}

fn removed_lines(file_diff: &FileDiff) -> u32 {
    file_diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.sections)
        .map(|section| match section {
            DiffSection::Removed { removed } => range_len(removed.old),
            _ => 0,
        })
        .sum()
}

fn range_len(range: LineRange) -> u32 {
    if range.end < range.start {
        return 0;
    }
    range.end - range.start + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_diff_sections() {
        let old = b"use anyhow::Result;\nfn old() {}\nfn keep() {}\n";
        let new = b"use anyhow::Result;\nfn new() {}\nfn added() {}\nfn keep() {}\n";
        let file = build_file_diff("src/lib.rs", Some(old), Some(new)).unwrap();

        assert_eq!(file.path, "src/lib.rs");
        assert_eq!(removed_lines(&file), 1);
        assert_eq!(added_lines(&file), 2);
    }

    #[test]
    fn diff_entries_reports_added_file() {
        let old_snapshot = BTreeMap::new();
        let new_snapshot =
            BTreeMap::from([("src/main.rs".to_string(), b"fn main() {}\n".to_vec())]);

        let entries = diff_entries(&old_snapshot, &new_snapshot).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/main.rs");
        assert_eq!(entries[0].status, FileStatus::Added);
    }

    #[test]
    fn applies_unchanged_file_context_for_comment() {
        let old_snapshot =
            BTreeMap::from([("src/main.rs".to_string(), b"one\ntwo\nthree\n".to_vec())]);
        let new_snapshot = old_snapshot.clone();
        let mut diff = ReviewDiffPayload {
            files: Vec::new(),
            file_contents_by_path: BTreeMap::new(),
            file_diffs_by_path: BTreeMap::new(),
        };

        apply_file_contexts(
            &mut diff,
            &old_snapshot,
            &new_snapshot,
            &[FileContextRequest {
                path: "src/main.rs".to_string(),
                old_path: None,
                side: Some(FileSide::New),
                start_line: Some(2),
                end_line: Some(2),
            }],
        );

        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "src/main.rs");
        assert_eq!(diff.files[0].status, FileStatus::Unchanged);
        assert!(!diff.files[0].is_changed);
        assert_eq!(diff.file_diffs_by_path["src/main.rs"].hunks.len(), 1);

        let hunk = &diff.file_diffs_by_path["src/main.rs"].hunks[0];
        assert_eq!(hunk.new, Some(LineRange { start: 1, end: 3 }));
        assert_eq!(
            diff.file_contents_by_path["src/main.rs"]
                .new
                .as_ref()
                .unwrap()[1],
            "two"
        );
    }
}
