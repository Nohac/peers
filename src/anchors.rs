use std::collections::BTreeMap;

use crate::diff::LineAnchor;

const HASH_KIND: gix::hash::Kind = gix::hash::Kind::Sha1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorPlacement {
    Exact,
    PerLineHash,
    Context,
    MovedExact,
    Window,
    LineFallback,
    FileFallback,
    Detached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorLinePlacement {
    Exact,
    Content,
    Context,
    Changed,
    Missing,
    LineFallback,
    Detached,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelocatedAnchorLine {
    pub current_line: Option<u32>,
    pub placement: AnchorLinePlacement,
    pub original_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelocatedLineAnchor {
    pub path: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub placement: AnchorPlacement,
    pub line_placements: Vec<RelocatedAnchorLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineMatch {
    path: String,
    start_line: u32,
    end_line: u32,
}

#[derive(Clone, Debug)]
pub struct AnchorIndex {
    files: BTreeMap<String, AnchorFileIndex>,
}

#[derive(Clone, Debug)]
pub struct AnchorFileIndex {
    lines: Vec<String>,
    line_hashes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScoredLineMatch {
    line_match: LineMatch,
    score: usize,
}

impl AnchorIndex {
    pub fn new(files: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(path, lines)| (path, AnchorFileIndex::new(lines)))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl AnchorFileIndex {
    fn new(lines: Vec<String>) -> Self {
        let line_hashes = lines.iter().map(|line| stable_hash(line)).collect();
        Self { lines, line_hashes }
    }
}

pub fn capture_line_anchor_evidence(
    anchor: &mut LineAnchor,
    lines: &[String],
    context_lines: usize,
) {
    let Some((start, end)) = zero_based_range(anchor.start_line, anchor.end_line, lines.len())
    else {
        return;
    };

    let selected = lines[start..=end].to_vec();
    let before_start = start.saturating_sub(context_lines);
    let after_end = (end + 1 + context_lines).min(lines.len());
    anchor.selected_text = Some(selected.join("\n"));
    anchor.selected_text_hash = anchor.selected_text.as_ref().map(|text| stable_hash(text));
    anchor.selected_range_hash = Some(hash_lines(&selected));
    anchor.per_line_hashes = selected.iter().map(|line| stable_hash(line)).collect();
    anchor.context_before = lines[before_start..start].to_vec();
    anchor.context_before_hash = if anchor.context_before.is_empty() {
        None
    } else {
        Some(hash_lines(&anchor.context_before))
    };
    anchor.context_after = lines[end + 1..after_end].to_vec();
    anchor.context_after_hash = if anchor.context_after.is_empty() {
        None
    } else {
        Some(hash_lines(&anchor.context_after))
    };

    let mut nearby = anchor.context_before.clone();
    nearby.extend(selected);
    nearby.extend(anchor.context_after.clone());
    anchor.nearby_context_hash = if nearby.is_empty() {
        None
    } else {
        Some(hash_lines(&nearby))
    };
}

pub fn relocate_line_anchor(
    anchor: &LineAnchor,
    files: &BTreeMap<String, Vec<String>>,
) -> RelocatedLineAnchor {
    let index = AnchorIndex::new(files.clone());
    relocate_line_anchor_in_index(anchor, &index)
}

pub fn relocate_line_anchor_in_index(
    anchor: &LineAnchor,
    index: &AnchorIndex,
) -> RelocatedLineAnchor {
    let same_paths = same_path_candidates(anchor, index);
    let selection_len = selected_line_count(anchor);

    if let Some(line_match) = unique_match_with_context(
        exact_hash_matches(&same_paths, index, selection_len, &anchor.per_line_hashes),
        index,
        anchor,
    ) {
        return relocated(line_match, AnchorPlacement::Exact, anchor, index);
    }

    if let Some(line_match) = unique_match_with_context(
        per_line_hash_matches(&same_paths, index, &anchor.per_line_hashes),
        index,
        anchor,
    ) {
        return relocated(line_match, AnchorPlacement::PerLineHash, anchor, index);
    }

    if let Some(line_match) = unique_match(context_matches(&same_paths, index, anchor)) {
        return relocated(line_match, AnchorPlacement::Context, anchor, index);
    }

    let moved_paths: Vec<_> = index
        .files
        .keys()
        .filter(|path| !same_paths.iter().any(|candidate| candidate == *path))
        .cloned()
        .collect();
    if let Some(line_match) = unique_match_with_context(
        exact_hash_matches(&moved_paths, index, selection_len, &anchor.per_line_hashes),
        index,
        anchor,
    ) {
        return relocated(line_match, AnchorPlacement::MovedExact, anchor, index);
    }

    if let Some(line_match) = unique_scored_match(window_matches(&same_paths, index, anchor)) {
        return relocated(line_match, AnchorPlacement::Window, anchor, index);
    }

    if let Some(line_match) = unique_scored_match(window_matches(&moved_paths, index, anchor)) {
        return relocated(line_match, AnchorPlacement::Window, anchor, index);
    }

    if let Some(file) = index.files.get(&anchor.path) {
        if anchor.start_line >= 1 && anchor.start_line as usize <= file.lines.len() {
            let end_line = anchor
                .end_line
                .max(anchor.start_line)
                .min(file.lines.len() as u32);
            return RelocatedLineAnchor {
                path: Some(anchor.path.clone()),
                start_line: Some(anchor.start_line),
                end_line: Some(end_line),
                placement: AnchorPlacement::LineFallback,
                line_placements: fallback_line_placements(
                    anchor,
                    Some(anchor.start_line),
                    Some(end_line),
                    AnchorLinePlacement::LineFallback,
                ),
            };
        }

        return RelocatedLineAnchor {
            path: Some(anchor.path.clone()),
            start_line: None,
            end_line: None,
            placement: AnchorPlacement::FileFallback,
            line_placements: fallback_line_placements(
                anchor,
                None,
                None,
                AnchorLinePlacement::Missing,
            ),
        };
    }

    RelocatedLineAnchor {
        path: None,
        start_line: None,
        end_line: None,
        placement: AnchorPlacement::Detached,
        line_placements: fallback_line_placements(
            anchor,
            None,
            None,
            AnchorLinePlacement::Detached,
        ),
    }
}

fn same_path_candidates(anchor: &LineAnchor, index: &AnchorIndex) -> Vec<String> {
    let mut paths = Vec::new();
    if index.files.contains_key(&anchor.path) {
        paths.push(anchor.path.clone());
    }
    if let Some(old_path) = &anchor.old_path
        && old_path != &anchor.path
        && index.files.contains_key(old_path)
    {
        paths.push(old_path.clone());
    }
    paths
}

fn selected_line_count(anchor: &LineAnchor) -> usize {
    anchor.end_line.max(anchor.start_line) as usize - anchor.start_line as usize + 1
}

fn exact_hash_matches(
    paths: &[String],
    index: &AnchorIndex,
    selection_len: usize,
    per_line_hashes: &[String],
) -> Vec<LineMatch> {
    if selection_len == 0 || per_line_hashes.len() != selection_len {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for path in paths {
        let Some(file) = index.files.get(path) else {
            continue;
        };
        if file.lines.len() < selection_len {
            continue;
        }
        for start in 0..=file.lines.len() - selection_len {
            let end = start + selection_len - 1;
            if file.line_hashes[start..=end] == *per_line_hashes {
                matches.push(LineMatch {
                    path: path.clone(),
                    start_line: (start + 1) as u32,
                    end_line: (end + 1) as u32,
                });
            }
        }
    }
    matches
}

fn per_line_hash_matches(
    paths: &[String],
    index: &AnchorIndex,
    per_line_hashes: &[String],
) -> Vec<LineMatch> {
    if per_line_hashes.is_empty() {
        return Vec::new();
    }

    let selection_len = per_line_hashes.len();
    let mut matches = Vec::new();
    for path in paths {
        let Some(file) = index.files.get(path) else {
            continue;
        };
        if file.lines.len() < selection_len {
            continue;
        }
        for start in 0..=file.line_hashes.len() - selection_len {
            let end = start + selection_len - 1;
            if file.line_hashes[start..=end] == *per_line_hashes {
                matches.push(LineMatch {
                    path: path.clone(),
                    start_line: (start + 1) as u32,
                    end_line: (end + 1) as u32,
                });
            }
        }
    }
    matches
}

fn context_matches(paths: &[String], index: &AnchorIndex, anchor: &LineAnchor) -> Vec<LineMatch> {
    if anchor.context_before.is_empty() && anchor.context_after.is_empty() {
        return Vec::new();
    }

    let selection_len = selected_line_count(anchor);
    let mut matches = Vec::new();
    for path in paths {
        let Some(file) = index.files.get(path) else {
            continue;
        };
        if file.lines.len() < selection_len {
            continue;
        }
        for start in 0..=file.lines.len() - selection_len {
            let end = start + selection_len - 1;
            if context_matches_range(&file.lines, start, end, anchor) {
                matches.push(LineMatch {
                    path: path.clone(),
                    start_line: (start + 1) as u32,
                    end_line: (end + 1) as u32,
                });
            }
        }
    }
    matches
}

fn context_matches_range(lines: &[String], start: usize, end: usize, anchor: &LineAnchor) -> bool {
    let before_len = anchor.context_before.len();
    if before_len > 0 {
        if start < before_len {
            return false;
        }
        if lines[start - before_len..start] != anchor.context_before {
            return false;
        }
    }

    let after_len = anchor.context_after.len();
    if after_len > 0 {
        if end + 1 + after_len > lines.len() {
            return false;
        }
        if lines[end + 1..end + 1 + after_len] != anchor.context_after {
            return false;
        }
    }

    true
}

fn unique_match_with_context(
    matches: Vec<LineMatch>,
    index: &AnchorIndex,
    anchor: &LineAnchor,
) -> Option<LineMatch> {
    if matches.len() <= 1 {
        return unique_match(matches);
    }

    let context_matches: Vec<_> = matches
        .iter()
        .filter(|line_match| {
            index.files.get(&line_match.path).is_some_and(|file| {
                context_matches_range(
                    &file.lines,
                    line_match.start_line as usize - 1,
                    line_match.end_line as usize - 1,
                    anchor,
                )
            })
        })
        .cloned()
        .collect();
    unique_match(context_matches)
}

fn window_matches(
    paths: &[String],
    index: &AnchorIndex,
    anchor: &LineAnchor,
) -> Vec<ScoredLineMatch> {
    let selection_len = selected_line_count(anchor);
    if selection_len <= 1 || anchor.per_line_hashes.len() != selection_len {
        return Vec::new();
    }
    let minimum_score = minimum_window_score(selection_len);

    let mut matches = Vec::new();
    for path in paths {
        let Some(file) = index.files.get(path) else {
            continue;
        };
        if file.lines.len() < selection_len {
            continue;
        }
        for start in 0..=file.lines.len() - selection_len {
            let end = start + selection_len - 1;
            let score = file.line_hashes[start..=end]
                .iter()
                .zip(&anchor.per_line_hashes)
                .filter(|(current, original)| *current == *original)
                .count();
            if score >= minimum_score {
                matches.push(ScoredLineMatch {
                    line_match: LineMatch {
                        path: path.clone(),
                        start_line: (start + 1) as u32,
                        end_line: (end + 1) as u32,
                    },
                    score,
                });
            }
        }
    }
    matches
}

fn minimum_window_score(selection_len: usize) -> usize {
    if selection_len <= 2 {
        selection_len
    } else {
        selection_len - 1
    }
}

fn unique_match(matches: Vec<LineMatch>) -> Option<LineMatch> {
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn unique_scored_match(matches: Vec<ScoredLineMatch>) -> Option<LineMatch> {
    let highest_score = matches.iter().map(|line_match| line_match.score).max()?;
    let mut best = matches
        .into_iter()
        .filter(|line_match| line_match.score == highest_score);
    let first = best.next()?;
    if best.next().is_some() {
        None
    } else {
        Some(first.line_match)
    }
}

fn relocated(
    line_match: LineMatch,
    placement: AnchorPlacement,
    anchor: &LineAnchor,
    index: &AnchorIndex,
) -> RelocatedLineAnchor {
    let line_placements = index
        .files
        .get(&line_match.path)
        .map(|file| mapped_line_placements(anchor, file, line_match.start_line, placement))
        .unwrap_or_else(|| {
            fallback_line_placements(anchor, None, None, AnchorLinePlacement::Detached)
        });

    RelocatedLineAnchor {
        path: Some(line_match.path),
        start_line: Some(line_match.start_line),
        end_line: Some(line_match.end_line),
        placement,
        line_placements,
    }
}

fn mapped_line_placements(
    anchor: &LineAnchor,
    file: &AnchorFileIndex,
    relocated_start_line: u32,
    range_placement: AnchorPlacement,
) -> Vec<RelocatedAnchorLine> {
    (0..selected_line_count(anchor))
        .map(|offset| {
            let original_line = anchor.start_line + offset as u32;
            let current_line = relocated_start_line + offset as u32;
            let current_index = current_line as usize - 1;
            let placement = if current_index >= file.lines.len() {
                AnchorLinePlacement::Missing
            } else {
                mapped_line_placement(anchor, file, offset, current_index, range_placement)
            };
            RelocatedAnchorLine {
                original_line,
                current_line: (placement != AnchorLinePlacement::Missing).then_some(current_line),
                placement,
            }
        })
        .collect()
}

fn mapped_line_placement(
    anchor: &LineAnchor,
    file: &AnchorFileIndex,
    offset: usize,
    current_index: usize,
    range_placement: AnchorPlacement,
) -> AnchorLinePlacement {
    match range_placement {
        AnchorPlacement::Exact => AnchorLinePlacement::Exact,
        AnchorPlacement::PerLineHash => AnchorLinePlacement::Content,
        AnchorPlacement::MovedExact => AnchorLinePlacement::Exact,
        AnchorPlacement::Context | AnchorPlacement::Window => {
            if anchor
                .per_line_hashes
                .get(offset)
                .is_some_and(|hash| file.line_hashes[current_index] == *hash)
            {
                AnchorLinePlacement::Content
            } else {
                AnchorLinePlacement::Changed
            }
        }
        AnchorPlacement::LineFallback => AnchorLinePlacement::LineFallback,
        AnchorPlacement::FileFallback => AnchorLinePlacement::Missing,
        AnchorPlacement::Detached => AnchorLinePlacement::Detached,
    }
}

fn fallback_line_placements(
    anchor: &LineAnchor,
    start_line: Option<u32>,
    end_line: Option<u32>,
    placement: AnchorLinePlacement,
) -> Vec<RelocatedAnchorLine> {
    (0..selected_line_count(anchor))
        .map(|offset| {
            let original_line = anchor.start_line + offset as u32;
            let current_line = start_line
                .map(|start_line| start_line + offset as u32)
                .filter(|line| end_line.is_some_and(|end_line| *line <= end_line));
            RelocatedAnchorLine {
                original_line,
                current_line,
                placement: if current_line.is_some() {
                    placement
                } else if placement == AnchorLinePlacement::Detached {
                    AnchorLinePlacement::Detached
                } else {
                    AnchorLinePlacement::Missing
                },
            }
        })
        .collect()
}

fn zero_based_range(start_line: u32, end_line: u32, line_count: usize) -> Option<(usize, usize)> {
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let start = start_line as usize - 1;
    let end = end_line as usize - 1;
    if start >= line_count || end >= line_count {
        return None;
    }
    Some((start, end))
}

fn hash_lines(lines: &[String]) -> String {
    stable_hash(&lines.join("\n"))
}

fn stable_hash(input: &str) -> String {
    let mut hasher = gix::hash::hasher(HASH_KIND);
    hasher.update(input.as_bytes());
    hasher
        .try_finalize()
        .expect("hashing in-memory anchor content should not fail")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileSide, LineAnchor};

    struct Case {
        name: &'static str,
        anchor: LineAnchor,
        files: BTreeMap<String, Vec<String>>,
        expected_path: Option<&'static str>,
        expected_start: Option<u32>,
        expected_end: Option<u32>,
        expected_placement: AnchorPlacement,
        expected_line_placements: Vec<(u32, Option<u32>, AnchorLinePlacement)>,
    }

    #[test]
    fn relocates_line_anchors_with_content_first_fallbacks() {
        let base = lines(&[
            "mod tests {",
            "    fn helper() {}",
            "    target_call();",
            "    assert!(true);",
            "}",
        ]);
        let mut anchor = LineAnchor::new("src/lib.rs".to_string(), FileSide::New, 3, 3);
        capture_line_anchor_evidence(&mut anchor, &base, 1);
        let mut anchor_without_context =
            LineAnchor::new("src/lib.rs".to_string(), FileSide::New, 3, 3);
        capture_line_anchor_evidence(&mut anchor_without_context, &base, 0);
        let multiline_base = lines(&[
            "fn configure() {",
            "    let first = load();",
            "    let second = prepare();",
            "    apply(first, second);",
            "    finish();",
            "}",
        ]);
        let mut multiline_anchor =
            LineAnchor::new("src/config.rs".to_string(), FileSide::New, 2, 4);
        capture_line_anchor_evidence(&mut multiline_anchor, &multiline_base, 1);

        let cases = vec![
            Case {
                name: "exact same range",
                anchor: anchor.clone(),
                files: files(&[("src/lib.rs", &base)]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(3),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::Exact,
                expected_line_placements: vec![(3, Some(3), AnchorLinePlacement::Exact)],
            },
            Case {
                name: "exact range after inserted line",
                anchor: anchor.clone(),
                files: files(&[(
                    "src/lib.rs",
                    &lines(&[
                        "mod tests {",
                        "    fn helper() {}",
                        "    let inserted = true;",
                        "    target_call();",
                        "    assert!(true);",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(4),
                expected_end: Some(4),
                expected_placement: AnchorPlacement::Exact,
                expected_line_placements: vec![(3, Some(4), AnchorLinePlacement::Exact)],
            },
            Case {
                name: "changed selected line with same context",
                anchor: anchor.clone(),
                files: files(&[(
                    "src/lib.rs",
                    &lines(&[
                        "mod tests {",
                        "    fn helper() {}",
                        "    renamed_call();",
                        "    assert!(true);",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(3),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::Context,
                expected_line_placements: vec![(3, Some(3), AnchorLinePlacement::Changed)],
            },
            Case {
                name: "moved exact range to another file",
                anchor: anchor.clone(),
                files: files(&[
                    (
                        "src/lib.rs",
                        &lines(&[
                            "mod tests {",
                            "    fn helper() {}",
                            "    assert!(true);",
                            "}",
                        ]),
                    ),
                    (
                        "src/moved.rs",
                        &lines(&["fn moved() {", "    target_call();", "}"]),
                    ),
                ]),
                expected_path: Some("src/moved.rs"),
                expected_start: Some(2),
                expected_end: Some(2),
                expected_placement: AnchorPlacement::MovedExact,
                expected_line_placements: vec![(3, Some(2), AnchorLinePlacement::Exact)],
            },
            Case {
                name: "ambiguous exact text resolved by context",
                anchor: anchor.clone(),
                files: files(&[(
                    "src/lib.rs",
                    &lines(&[
                        "mod tests {",
                        "    target_call();",
                        "    fn helper() {}",
                        "    target_call();",
                        "    assert!(true);",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(4),
                expected_end: Some(4),
                expected_placement: AnchorPlacement::Exact,
                expected_line_placements: vec![(3, Some(4), AnchorLinePlacement::Exact)],
            },
            Case {
                name: "ambiguous exact text without context does not guess",
                anchor: anchor_without_context,
                files: files(&[(
                    "src/lib.rs",
                    &lines(&[
                        "mod tests {",
                        "    target_call();",
                        "    fn helper() {}",
                        "    target_call();",
                        "    assert!(true);",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(3),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::LineFallback,
                expected_line_placements: vec![(3, Some(3), AnchorLinePlacement::LineFallback)],
            },
            Case {
                name: "line fallback when content and context drift",
                anchor: anchor.clone(),
                files: files(&[(
                    "src/lib.rs",
                    &lines(&[
                        "mod tests {",
                        "    fn unrelated() {}",
                        "    different_call();",
                        "    assert_eq!(1, 1);",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(3),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::LineFallback,
                expected_line_placements: vec![(3, Some(3), AnchorLinePlacement::LineFallback)],
            },
            Case {
                name: "file fallback when original line is gone",
                anchor: anchor.clone(),
                files: files(&[("src/lib.rs", &lines(&["mod tests {", "}"]))]),
                expected_path: Some("src/lib.rs"),
                expected_start: None,
                expected_end: None,
                expected_placement: AnchorPlacement::FileFallback,
                expected_line_placements: vec![(3, None, AnchorLinePlacement::Missing)],
            },
            Case {
                name: "detached when file is gone",
                anchor: anchor.clone(),
                files: files(&[("src/other.rs", &lines(&["fn other() {}"]))]),
                expected_path: None,
                expected_start: None,
                expected_end: None,
                expected_placement: AnchorPlacement::Detached,
                expected_line_placements: vec![(3, None, AnchorLinePlacement::Detached)],
            },
            Case {
                name: "multiline exact range after inserted line",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let inserted = true;",
                        "    let first = load();",
                        "    let second = prepare();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(3),
                expected_end: Some(5),
                expected_placement: AnchorPlacement::Exact,
                expected_line_placements: vec![
                    (2, Some(3), AnchorLinePlacement::Exact),
                    (3, Some(4), AnchorLinePlacement::Exact),
                    (4, Some(5), AnchorLinePlacement::Exact),
                ],
            },
            Case {
                name: "multiline context range keeps changed line in contiguous block",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    let second = recompute();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(4),
                expected_placement: AnchorPlacement::Context,
                expected_line_placements: vec![
                    (2, Some(2), AnchorLinePlacement::Content),
                    (3, Some(3), AnchorLinePlacement::Changed),
                    (4, Some(4), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline scattered matching lines do not split the range",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    let unrelated = true;",
                        "    let second = prepare();",
                        "    let other = true;",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(4),
                expected_placement: AnchorPlacement::LineFallback,
                expected_line_placements: vec![
                    (2, Some(2), AnchorLinePlacement::LineFallback),
                    (3, Some(3), AnchorLinePlacement::LineFallback),
                    (4, Some(4), AnchorLinePlacement::LineFallback),
                ],
            },
            Case {
                name: "multiline partial window after inserted line",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let inserted = true;",
                        "    let first = load();",
                        "    let second = recompute();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(3),
                expected_end: Some(5),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (2, Some(3), AnchorLinePlacement::Content),
                    (3, Some(4), AnchorLinePlacement::Changed),
                    (4, Some(5), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline partial window moved to another file",
                anchor: multiline_anchor,
                files: files(&[
                    (
                        "src/config.rs",
                        &lines(&["fn configure() {", "    finish();", "}"]),
                    ),
                    (
                        "src/moved.rs",
                        &lines(&[
                            "fn moved() {",
                            "    let first = load();",
                            "    let second = recompute();",
                            "    apply(first, second);",
                            "}",
                        ]),
                    ),
                ]),
                expected_path: Some("src/moved.rs"),
                expected_start: Some(2),
                expected_end: Some(4),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (2, Some(2), AnchorLinePlacement::Content),
                    (3, Some(3), AnchorLinePlacement::Changed),
                    (4, Some(4), AnchorLinePlacement::Content),
                ],
            },
        ];

        for case in cases {
            let relocated = relocate_line_anchor(&case.anchor, &case.files);
            assert_eq!(
                relocated.path.as_deref(),
                case.expected_path,
                "{}: path",
                case.name
            );
            assert_eq!(
                relocated.start_line, case.expected_start,
                "{}: start",
                case.name
            );
            assert_eq!(relocated.end_line, case.expected_end, "{}: end", case.name);
            assert_eq!(
                relocated.placement, case.expected_placement,
                "{}: placement",
                case.name
            );
            let line_placements: Vec<_> = relocated
                .line_placements
                .iter()
                .map(|line| (line.original_line, line.current_line, line.placement))
                .collect();
            assert_eq!(
                line_placements, case.expected_line_placements,
                "{}: line placements",
                case.name
            );
        }
    }

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|line| line.to_string()).collect()
    }

    fn files(input: &[(&str, &Vec<String>)]) -> BTreeMap<String, Vec<String>> {
        input
            .iter()
            .map(|(path, lines)| ((*path).to_string(), (*lines).clone()))
            .collect()
    }
}
