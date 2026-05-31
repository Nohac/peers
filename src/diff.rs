use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use facet::Facet;
use tokio::process::Command;

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
    pub selected_text_hash: Option<String>,
    pub nearby_context_hash: Option<String>,
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
            selected_text_hash: None,
            nearby_context_hash: None,
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
    pub viewed: bool,
    pub comment_count: u32,
    pub added_lines: u32,
    pub removed_lines: u32,
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
    Commit(String),
}

pub async fn load_review_diff(
    repo_root: &Path,
    target: &ReviewTarget,
) -> Result<ReviewDiffPayload> {
    // Keep Git repository access thin here. gix remains the repo discovery layer; Git's
    // porcelain diff format is then normalized into the compact UI model.
    let _repo = gix::discover(repo_root).context("failed to open Git repository for diff")?;
    let resolved = ResolvedTarget::resolve(repo_root, target).await?;
    let raw_diff = run_git(repo_root, resolved.diff_args("--unified=3")).await?;
    let mut file_diffs = parse_unified_diff(&raw_diff)?;
    let name_status = run_git(repo_root, resolved.diff_args("--name-status")).await?;
    let status_by_path = parse_name_status(&name_status);

    let mut files = Vec::new();
    let mut file_contents_by_path = BTreeMap::new();
    let mut file_diffs_by_path = BTreeMap::new();

    for file_diff in &mut file_diffs {
        let path = file_diff.path.clone();
        let parsed_status = status_by_path.get(&path);
        let old_path = parsed_status
            .and_then(|entry| entry.old_path.clone())
            .or_else(|| file_diff_old_path(file_diff));
        let status = parsed_status
            .map(|entry| entry.status)
            .unwrap_or(FileStatus::Modified);
        let added_lines = added_lines(file_diff);
        let removed_lines = removed_lines(file_diff);
        let old_content = read_content(
            repo_root,
            &resolved.old_source,
            old_path.as_deref().unwrap_or(&path),
        )
        .await?;
        let new_content = read_content(repo_root, &resolved.new_source, &path).await?;

        file_contents_by_path.insert(
            path.clone(),
            FileContent {
                old: if matches!(status, FileStatus::Added) {
                    None
                } else {
                    old_content
                },
                new: if matches!(status, FileStatus::Deleted) {
                    None
                } else {
                    new_content
                },
            },
        );
        file_diffs_by_path.insert(path.clone(), file_diff.clone());
        files.push(ReviewFile {
            path,
            old_path,
            status,
            is_changed: true,
            viewed: false,
            comment_count: 0,
            added_lines,
            removed_lines,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(ReviewDiffPayload {
        files,
        file_contents_by_path,
        file_diffs_by_path,
    })
}

struct ResolvedTarget {
    diff_base: Option<String>,
    diff_head: Option<String>,
    cached: bool,
    old_source: ContentSource,
    new_source: ContentSource,
}

impl ResolvedTarget {
    async fn resolve(repo_root: &Path, target: &ReviewTarget) -> Result<Self> {
        match target {
            ReviewTarget::WorkingTree => Ok(Self {
                diff_base: None,
                diff_head: None,
                cached: false,
                old_source: ContentSource::Index,
                new_source: ContentSource::Worktree,
            }),
            ReviewTarget::Cached => Ok(Self {
                diff_base: None,
                diff_head: None,
                cached: true,
                old_source: ContentSource::Commit("HEAD".to_string()),
                new_source: ContentSource::Index,
            }),
            ReviewTarget::All => Ok(Self {
                diff_base: Some("HEAD".to_string()),
                diff_head: None,
                cached: false,
                old_source: ContentSource::Commit("HEAD".to_string()),
                new_source: ContentSource::Worktree,
            }),
            ReviewTarget::Branch { base, head } => {
                let merge_base =
                    run_git(repo_root, ["merge-base", base.as_str(), head.as_str()]).await?;
                let merge_base = merge_base.trim().to_string();
                Ok(Self {
                    diff_base: Some(merge_base.clone()),
                    diff_head: Some(head.clone()),
                    cached: false,
                    old_source: ContentSource::Commit(merge_base),
                    new_source: ContentSource::Commit(head.clone()),
                })
            }
        }
    }

    fn diff_args<'a>(&'a self, format_arg: &'a str) -> Vec<&'a str> {
        let mut args = vec![
            "diff",
            "--find-renames",
            "--no-color",
            "--no-ext-diff",
            format_arg,
        ];
        if self.cached {
            args.push("--cached");
        }
        if let Some(base) = &self.diff_base {
            args.push(base);
        }
        if let Some(head) = &self.diff_head {
            args.push(head);
        }
        args
    }
}

async fn run_git<I, S>(repo_root: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .await
        .context("failed to run git")?;

    if !output.status.success() {
        return Err(anyhow!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn read_content(
    repo_root: &Path,
    source: &ContentSource,
    path: &str,
) -> Result<Option<Vec<String>>> {
    let bytes = match source {
        ContentSource::Worktree => {
            let full_path = repo_root.join(path);
            match tokio::fs::read(full_path).await {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            }
        }
        ContentSource::Index => {
            match run_git(repo_root, ["show".to_string(), format!(":{path}")]).await {
                Ok(output) => output.into_bytes(),
                Err(_) => return Ok(None),
            }
        }
        ContentSource::Commit(commit) => {
            match run_git(repo_root, ["show".to_string(), format!("{commit}:{path}")]).await {
                Ok(output) => output.into_bytes(),
                Err(_) => return Ok(None),
            }
        }
    };

    if bytes.contains(&0) {
        return Ok(None);
    }

    Ok(Some(split_lines(&String::from_utf8_lossy(&bytes))))
}

fn split_lines(input: &str) -> Vec<String> {
    input
        .strip_suffix('\n')
        .unwrap_or(input)
        .split('\n')
        .map(str::to_string)
        .collect()
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

fn parse_unified_diff(input: &str) -> Result<Vec<FileDiff>> {
    let mut files = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut current_section: Option<SectionBuilder> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush_section(&mut current_hunk, &mut current_section);
            flush_hunk(&mut current_file, &mut current_hunk);
            if let Some(file) = current_file.take() {
                files.push(file);
            }
            let path = rest
                .split_once(" b/")
                .map(|(_, path)| path.to_string())
                .unwrap_or_default();
            current_file = Some(FileDiff {
                path,
                hunks: Vec::new(),
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ b/") {
            if let Some(file) = &mut current_file {
                file.path = path.to_string();
            }
            continue;
        }

        if let Some(header) = line.strip_prefix("@@ ") {
            flush_section(&mut current_hunk, &mut current_section);
            flush_hunk(&mut current_file, &mut current_hunk);
            let (old_range, new_range) = parse_hunk_header(header)?;
            old_line = old_range.start;
            new_line = new_range.start;
            current_hunk = Some(DiffHunk {
                old: Some(old_range),
                new: Some(new_range),
                sections: Vec::new(),
            });
            continue;
        }

        let Some(first) = line.as_bytes().first().copied() else {
            continue;
        };
        if current_hunk.is_none() {
            continue;
        }

        match first {
            b' ' => {
                append_section(
                    &mut current_hunk,
                    &mut current_section,
                    SectionKind::Context,
                    Some(old_line),
                    Some(new_line),
                );
                old_line += 1;
                new_line += 1;
            }
            b'+' => {
                append_section(
                    &mut current_hunk,
                    &mut current_section,
                    SectionKind::Added,
                    None,
                    Some(new_line),
                );
                new_line += 1;
            }
            b'-' => {
                append_section(
                    &mut current_hunk,
                    &mut current_section,
                    SectionKind::Removed,
                    Some(old_line),
                    None,
                );
                old_line += 1;
            }
            b'\\' => {}
            _ => {}
        }
    }

    flush_section(&mut current_hunk, &mut current_section);
    flush_hunk(&mut current_file, &mut current_hunk);
    if let Some(file) = current_file {
        files.push(file);
    }

    Ok(files)
}

fn append_section(
    current_hunk: &mut Option<DiffHunk>,
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

    flush_section(current_hunk, current_section);
    *current_section = Some(match kind {
        SectionKind::Context => {
            SectionBuilder::context(old_line.unwrap_or(1), new_line.unwrap_or(1))
        }
        SectionKind::Added => SectionBuilder::added(new_line.unwrap_or(1)),
        SectionKind::Removed => SectionBuilder::removed(old_line.unwrap_or(1)),
    });
}

fn flush_section(
    current_hunk: &mut Option<DiffHunk>,
    current_section: &mut Option<SectionBuilder>,
) {
    if let (Some(hunk), Some(section)) = (current_hunk.as_mut(), current_section.take()) {
        hunk.sections.push(section.finish());
    }
}

fn flush_hunk(current_file: &mut Option<FileDiff>, current_hunk: &mut Option<DiffHunk>) {
    if let (Some(file), Some(hunk)) = (current_file.as_mut(), current_hunk.take()) {
        file.hunks.push(hunk);
    }
}

fn parse_hunk_header(header: &str) -> Result<(LineRange, LineRange)> {
    let mut parts = header.split_whitespace();
    let old = parts
        .next()
        .ok_or_else(|| anyhow!("hunk header missing old range"))?;
    let new = parts
        .next()
        .ok_or_else(|| anyhow!("hunk header missing new range"))?;
    Ok((parse_hunk_range(old, '-')?, parse_hunk_range(new, '+')?))
}

fn parse_hunk_range(input: &str, prefix: char) -> Result<LineRange> {
    let input = input
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("invalid hunk range `{input}`"))?;
    let (start, count) = match input.split_once(',') {
        Some((start, count)) => (start.parse::<u32>()?, count.parse::<u32>()?),
        None => (input.parse::<u32>()?, 1),
    };
    Ok(LineRange {
        start,
        end: start.saturating_add(count).saturating_sub(1),
    })
}

struct NameStatusEntry {
    status: FileStatus,
    old_path: Option<String>,
}

fn parse_name_status(input: &str) -> BTreeMap<String, NameStatusEntry> {
    let mut entries = BTreeMap::new();
    for line in input.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() < 2 {
            continue;
        }
        let code = fields[0];
        let status = match code.as_bytes().first().copied() {
            Some(b'A') => FileStatus::Added,
            Some(b'D') => FileStatus::Deleted,
            Some(b'R') => FileStatus::Renamed,
            Some(b'M') => FileStatus::Modified,
            Some(b'T') => FileStatus::Modified,
            _ => FileStatus::Modified,
        };
        if code.starts_with('R') && fields.len() >= 3 {
            entries.insert(
                fields[2].to_string(),
                NameStatusEntry {
                    status,
                    old_path: Some(fields[1].to_string()),
                },
            );
        } else {
            entries.insert(
                fields[1].to_string(),
                NameStatusEntry {
                    status,
                    old_path: None,
                },
            );
        }
    }
    entries
}

fn file_diff_old_path(_file_diff: &FileDiff) -> Option<String> {
    None
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
    range.end.saturating_sub(range.start).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unified_diff_sections() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 use anyhow::Result;
-fn old() {}
+fn new() {}
+fn added() {}
 fn keep() {}
";
        let files = parse_unified_diff(input).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!(removed_lines(&files[0]), 1);
        assert_eq!(added_lines(&files[0]), 2);
    }
}
