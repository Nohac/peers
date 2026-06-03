use std::collections::BTreeMap;

use crate::diff::LineAnchor;

const HASH_KIND: gix::hash::Kind = gix::hash::Kind::Sha1;
const MAX_WINDOW_GAP_LINES: usize = 100;
const MAX_CANDIDATE_HASH_OCCURRENCES: usize = 16;
const MIN_CANDIDATE_LINE_SIGNAL: usize = 8;

type LineHash = gix::ObjectId;

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
    Gap,
    Missing,
    LineFallback,
    // test com
    Detached,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelocatedAnchorLine {
    pub current_line: Option<u32>,
    pub placement: AnchorLinePlacement,
    pub original_line: Option<u32>,
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
    line_hashes: Vec<LineHash>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScoredLineMatch {
    line_match: LineMatch,
    score: usize,
    cost: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowAlignment {
    exact_count: usize,
    cost: usize,
    // test
    // test
    line_placements: Vec<RelocatedAnchorLine>,
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
        let line_hashes = lines.iter().map(|line| stable_hash_id(line)).collect();
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
    let anchor_hashes = parsed_line_hashes(&anchor.per_line_hashes);

    if let Some(line_match) = unique_match_with_context(
        anchor_hashes
            .as_deref()
            .map(|hashes| exact_hash_matches(&same_paths, index, selection_len, hashes))
            .unwrap_or_default(),
        index,
        anchor,
    ) {
        return relocated(line_match, AnchorPlacement::Exact, anchor, index);
    }

    if let Some(line_match) = unique_match_with_context(
        anchor_hashes
            .as_deref()
            .map(|hashes| per_line_hash_matches(&same_paths, index, hashes))
            .unwrap_or_default(),
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
        anchor_hashes
            .as_deref()
            .map(|hashes| exact_hash_matches(&moved_paths, index, selection_len, hashes))
            .unwrap_or_default(),
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
    per_line_hashes: &[LineHash],
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
    per_line_hashes: &[LineHash],
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
    let Some(anchor_hashes) = parsed_line_hashes(&anchor.per_line_hashes) else {
        return Vec::new();
    };
    if selection_len <= 1 || anchor_hashes.len() != selection_len {
        return Vec::new();
    }
    let minimum_score = minimum_window_score(selection_len);
    let max_gap = max_window_gap(selection_len);

    let mut matches = Vec::new();
    for path in paths {
        let Some(file) = index.files.get(path) else {
            continue;
        };
        if file.lines.is_empty() {
            continue;
        }
        for candidate in ordered_window_candidates(anchor, &anchor_hashes, file, max_gap) {
            let Some(alignment) = window_alignment_for_hashes(
                anchor,
                &anchor_hashes,
                file,
                candidate.start,
                candidate.end,
            ) else {
                continue;
            };
            if alignment.exact_count >= minimum_score
                && alignment_has_supported_edits(&alignment, max_gap)
                && !window_is_stale(anchor, &candidate, &alignment)
            {
                matches.push(ScoredLineMatch {
                    line_match: LineMatch {
                        path: path.clone(),
                        start_line: (candidate.start + 1) as u32,
                        end_line: (candidate.end + 1) as u32,
                    },
                    score: alignment.exact_count,
                    cost: alignment.cost + candidate.end - candidate.start + 1,
                });
            }
        }
    }
    matches
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowCandidate {
    start: usize,
    end: usize,
}

fn ordered_window_candidates(
    anchor: &LineAnchor,
    anchor_hashes: &[LineHash],
    file: &AnchorFileIndex,
    max_gap: usize,
) -> Vec<WindowCandidate> {
    let mut candidates = Vec::new();
    let current_lines_by_hash = current_lines_by_hash(file);
    let candidate_originals =
        candidate_original_indexes(anchor, anchor_hashes, &current_lines_by_hash);
    for cluster in evidence_clusters(
        &candidate_originals,
        anchor_hashes,
        &current_lines_by_hash,
        max_gap,
    ) {
        if let Some(candidate) = candidate_from_evidence_cluster(
            &cluster,
            anchor_hashes.len(),
            file.lines.len(),
            max_gap,
        ) {
            candidates.push(candidate);
        }
        if let Some(candidate) =
            trailing_missing_candidate_from_evidence_cluster(&cluster, file.lines.len())
        {
            candidates.push(candidate);
        }
    }
    candidates.sort_by_key(|candidate| (candidate.start, candidate.end));
    candidates.dedup();
    candidates
}

fn candidate_original_indexes(
    anchor: &LineAnchor,
    anchor_hashes: &[LineHash],
    current_lines_by_hash: &BTreeMap<LineHash, Vec<usize>>,
) -> Vec<usize> {
    let selected_lines: Vec<_> = anchor
        .selected_text
        .as_deref()
        .map(|text| text.lines().collect())
        .unwrap_or_default();

    anchor_hashes
        .iter()
        .enumerate()
        .filter_map(|(index, hash)| {
            let occurrence_count = current_lines_by_hash.get(hash)?.len();
            if occurrence_count > MAX_CANDIDATE_HASH_OCCURRENCES {
                return None;
            }

            let signal = selected_lines
                .get(index)
                .map_or(MIN_CANDIDATE_LINE_SIGNAL, |line| line_signal(line));
            (signal >= MIN_CANDIDATE_LINE_SIGNAL || occurrence_count <= 2).then_some(index)
        })
        .collect()
}

fn current_lines_by_hash(file: &AnchorFileIndex) -> BTreeMap<LineHash, Vec<usize>> {
    let mut current_lines_by_hash = BTreeMap::new();
    for (index, hash) in file.line_hashes.iter().enumerate() {
        current_lines_by_hash
            .entry(*hash)
            .or_insert_with(Vec::new)
            .push(index);
    }
    current_lines_by_hash
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EvidenceMatch {
    original_index: usize,
    current_index: usize,
}

fn evidence_clusters(
    candidate_originals: &[usize],
    anchor_hashes: &[LineHash],
    current_lines_by_hash: &BTreeMap<LineHash, Vec<usize>>,
    max_gap: usize,
) -> Vec<Vec<EvidenceMatch>> {
    let mut clusters = Vec::new();
    for (start_position, &start_original) in candidate_originals.iter().enumerate() {
        let Some(start_current_lines) = current_lines_by_hash.get(&anchor_hashes[start_original])
        else {
            continue;
        };
        for &start_current in start_current_lines {
            let mut cluster = vec![EvidenceMatch {
                original_index: start_original,
                current_index: start_current,
            }];
            let mut previous_current = start_current;
            for &original_index in candidate_originals.iter().skip(start_position + 1) {
                let Some(min_current) = previous_current.checked_add(1) else {
                    continue;
                };
                let max_current = start_current + (original_index - start_original) + max_gap;
                let Some(current_index) = closest_current_match(
                    current_lines_by_hash.get(&anchor_hashes[original_index]),
                    min_current,
                    max_current,
                ) else {
                    continue;
                };
                cluster.push(EvidenceMatch {
                    original_index,
                    current_index,
                });
                previous_current = current_index;
            }
            if cluster.len() >= 2 {
                clusters.push(cluster);
            }
        }
    }
    clusters
}

fn closest_current_match(
    current_lines: Option<&Vec<usize>>,
    min_current: usize,
    max_current: usize,
) -> Option<usize> {
    let current_lines = current_lines?;
    let start = current_lines.partition_point(|line| *line < min_current);
    current_lines
        .get(start)
        .copied()
        .filter(|line| *line <= max_current)
}

fn line_signal(line: &str) -> usize {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return 0;
    }
    if matches!(trimmed, "{" | "}" | ");" | "," | ";") {
        return 0;
    }
    if trimmed.starts_with("#[derive(") {
        return 1;
    }

    trimmed
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '_')
        .count()
}

fn candidate_from_evidence_cluster(
    cluster: &[EvidenceMatch],
    original_len: usize,
    file_len: usize,
    max_gap: usize,
) -> Option<WindowCandidate> {
    let first = cluster.first()?;
    let last = cluster.last()?;
    let (first_original, first_current) = (first.original_index, first.current_index);
    let (last_original, last_current) = (last.original_index, last.current_index);
    if first_original == last_original {
        return None;
    }

    let current_span = last_current - first_current + 1;
    let original_span = last_original - first_original + 1;
    if current_span > original_span && current_span - original_span > max_gap {
        return None;
    }

    let start = first_current.checked_sub(first_original)?;
    let end = last_current + (original_len - 1 - last_original);
    if end >= file_len {
        return None;
    }

    Some(WindowCandidate { start, end })
}

fn trailing_missing_candidate_from_evidence_cluster(
    cluster: &[EvidenceMatch],
    file_len: usize,
) -> Option<WindowCandidate> {
    let first = cluster.first()?;
    let last = cluster.last()?;
    let start = first.current_index.checked_sub(first.original_index)?;
    let end = last.current_index;
    if end < start || end >= file_len {
        return None;
    }
    Some(WindowCandidate { start, end })
}

fn max_window_gap(selection_len: usize) -> usize {
    MAX_WINDOW_GAP_LINES.max(selection_len / 3)
}

fn alignment_has_supported_edits(alignment: &WindowAlignment, max_gap: usize) -> bool {
    let mut index = 0;
    let mut total_gap_len = 0;
    let mut supported_edit_runs = 0;
    let similarity_is_strong = has_minimum_similarity(
        alignment.exact_count,
        selected_original_line_count(alignment),
    );
    while index < alignment.line_placements.len() {
        if strong_line_placement(alignment.line_placements[index].placement) {
            index += 1;
            continue;
        }

        let edit_start = index;
        let edit_is_leading_edge = edit_start == 0;
        let mut gap_len = 0;
        let mut changed_len = 0;
        let mut missing_len = 0;
        while index < alignment.line_placements.len()
            && !strong_line_placement(alignment.line_placements[index].placement)
        {
            match alignment.line_placements[index].placement {
                AnchorLinePlacement::Gap => gap_len += 1,
                AnchorLinePlacement::Changed => changed_len += 1,
                AnchorLinePlacement::Missing => missing_len += 1,
                _ => return false,
            }
            index += 1;
        }
        let edit_is_trailing_edge = index == alignment.line_placements.len();
        total_gap_len += gap_len;

        if gap_len > MAX_WINDOW_GAP_LINES {
            return false;
        }

        let before = edit_start
            .checked_sub(1)
            .and_then(|before| alignment.line_placements.get(before));
        let after = alignment.line_placements.get(index);
        let edit_is_bracketed = matches!(
            (
                before.map(|line| line.placement),
                after.map(|line| line.placement)
            ),
            (
                Some(AnchorLinePlacement::Content | AnchorLinePlacement::Exact),
                Some(AnchorLinePlacement::Content | AnchorLinePlacement::Exact)
            )
        );
        if !edit_is_bracketed
            && !edge_edit_allowed(
                edit_is_leading_edge,
                edit_is_trailing_edge,
                gap_len,
                changed_len,
                missing_len,
                similarity_is_strong,
            )
        {
            return false;
        }
        supported_edit_runs += 1;
    }

    supported_edit_runs == 0 || total_gap_len <= max_gap
}

fn edge_edit_allowed(
    is_leading_edge: bool,
    is_trailing_edge: bool,
    gap_len: usize,
    changed_len: usize,
    missing_len: usize,
    similarity_is_strong: bool,
) -> bool {
    if !similarity_is_strong {
        return false;
    }
    if gap_len > 0 && changed_len == 0 && missing_len == 0 {
        return is_leading_edge || is_trailing_edge;
    }
    is_trailing_edge && gap_len == 0 && changed_len == 0 && missing_len > 0
}

fn selected_original_line_count(alignment: &WindowAlignment) -> usize {
    alignment
        .line_placements
        .iter()
        .filter(|line| line.original_line.is_some())
        .count()
}

fn strong_line_placement(placement: AnchorLinePlacement) -> bool {
    matches!(
        placement,
        AnchorLinePlacement::Content | AnchorLinePlacement::Exact
    )
}

fn window_is_stale(
    anchor: &LineAnchor,
    candidate: &WindowCandidate,
    alignment: &WindowAlignment,
) -> bool {
    !has_minimum_similarity(alignment.exact_count, selected_line_count(anchor))
        && origin_drift(anchor, candidate) > selected_line_count(anchor)
}

fn has_minimum_similarity(exact_count: usize, selection_len: usize) -> bool {
    exact_count * 2 >= selection_len
}

fn origin_drift(anchor: &LineAnchor, candidate: &WindowCandidate) -> usize {
    let original_start = anchor.start_line.saturating_sub(1) as usize;
    let original_end = anchor.end_line.saturating_sub(1) as usize;
    let start_drift = candidate.start.abs_diff(original_start);
    let end_drift = candidate.end.abs_diff(original_end);
    start_drift.max(end_drift)
}

fn window_alignment(
    anchor: &LineAnchor,
    file: &AnchorFileIndex,
    start: usize,
    end: usize,
) -> Option<WindowAlignment> {
    let anchor_hashes = parsed_line_hashes(&anchor.per_line_hashes)?;
    window_alignment_for_hashes(anchor, &anchor_hashes, file, start, end)
}

fn window_alignment_for_hashes(
    anchor: &LineAnchor,
    anchor_hashes: &[LineHash],
    file: &AnchorFileIndex,
    start: usize,
    end: usize,
) -> Option<WindowAlignment> {
    let current_hashes = &file.line_hashes[start..=end];
    align_window(anchor_hashes, current_hashes).map(|mut alignment| {
        for line in &mut alignment.line_placements {
            if let Some(current_line) = line.current_line.as_mut() {
                *current_line += start as u32;
            }
            if let Some(original_line) = line.original_line.as_mut() {
                *original_line += anchor.start_line - 1;
            }
        }
        alignment
    })
}

fn align_window(
    original_hashes: &[LineHash],
    current_hashes: &[LineHash],
) -> Option<WindowAlignment> {
    if original_hashes.is_empty() {
        return None;
    }
    let mut memo = BTreeMap::new();
    align_window_from(original_hashes, current_hashes, 0, 0, &mut memo)
}

fn align_window_from(
    original_hashes: &[LineHash],
    current_hashes: &[LineHash],
    original_index: usize,
    current_index: usize,
    memo: &mut BTreeMap<(usize, usize), Option<WindowAlignment>>,
) -> Option<WindowAlignment> {
    if let Some(cached) = memo.get(&(original_index, current_index)) {
        return cached.clone();
    }

    if original_index == original_hashes.len() && current_index == current_hashes.len() {
        return Some(WindowAlignment {
            exact_count: 0,
            cost: 0,
            line_placements: Vec::new(),
        });
    }
    if original_index == original_hashes.len() {
        let mut rest = align_window_from(
            original_hashes,
            current_hashes,
            original_index,
            current_index + 1,
            memo,
        )?;
        rest.cost += 1;
        rest.line_placements.insert(
            0,
            RelocatedAnchorLine {
                current_line: Some(current_index as u32 + 1),
                placement: AnchorLinePlacement::Gap,
                original_line: None,
            },
        );
        memo.insert((original_index, current_index), Some(rest.clone()));
        return Some(rest);
    }
    if current_index == current_hashes.len() {
        let mut rest = align_window_from(
            original_hashes,
            current_hashes,
            original_index + 1,
            current_index,
            memo,
        )?;
        rest.cost += 2;
        rest.line_placements.insert(
            0,
            RelocatedAnchorLine {
                current_line: None,
                placement: AnchorLinePlacement::Missing,
                original_line: Some(original_index as u32 + 1),
            },
        );
        memo.insert((original_index, current_index), Some(rest.clone()));
        return Some(rest);
    }

    let mut candidates = Vec::new();
    if let Some(mut aligned) = align_window_from(
        original_hashes,
        current_hashes,
        original_index + 1,
        current_index + 1,
        memo,
    ) {
        let exact = original_hashes[original_index] == current_hashes[current_index];
        aligned.cost += if exact { 0 } else { 2 };
        aligned.exact_count += usize::from(exact);
        aligned.line_placements.insert(
            0,
            RelocatedAnchorLine {
                current_line: Some(current_index as u32 + 1),
                placement: if exact {
                    AnchorLinePlacement::Content
                } else {
                    AnchorLinePlacement::Changed
                },
                original_line: Some(original_index as u32 + 1),
            },
        );
        candidates.push(aligned);
    }

    if let Some(mut aligned) = align_window_from(
        original_hashes,
        current_hashes,
        original_index,
        current_index + 1,
        memo,
    ) {
        aligned.cost += 1;
        aligned.line_placements.insert(
            0,
            RelocatedAnchorLine {
                current_line: Some(current_index as u32 + 1),
                placement: AnchorLinePlacement::Gap,
                original_line: None,
            },
        );
        candidates.push(aligned);
    }

    if let Some(mut aligned) = align_window_from(
        original_hashes,
        current_hashes,
        original_index + 1,
        current_index,
        memo,
    ) {
        aligned.cost += 2;
        aligned.line_placements.insert(
            0,
            RelocatedAnchorLine {
                current_line: None,
                placement: AnchorLinePlacement::Missing,
                original_line: Some(original_index as u32 + 1),
            },
        );
        candidates.push(aligned);
    }

    let best = candidates.into_iter().max_by(|left, right| {
        left.exact_count
            .cmp(&right.exact_count)
            .then_with(|| right.cost.cmp(&left.cost))
    });
    memo.insert((original_index, current_index), best.clone());
    best
}

fn minimum_window_score(selection_len: usize) -> usize {
    1.max(selection_len / 4)
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
    let lowest_cost = matches
        .iter()
        .filter(|line_match| line_match.score == highest_score)
        .map(|line_match| line_match.cost)
        .min()?;
    let mut best = matches
        .into_iter()
        .filter(|line_match| line_match.score == highest_score && line_match.cost == lowest_cost);
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
        .map(|file| {
            mapped_line_placements(
                anchor,
                file,
                line_match.start_line,
                line_match.end_line,
                placement,
            )
        })
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
    relocated_end_line: u32,
    range_placement: AnchorPlacement,
) -> Vec<RelocatedAnchorLine> {
    if range_placement == AnchorPlacement::Window
        && let Some(alignment) = window_alignment(
            anchor,
            file,
            relocated_start_line as usize - 1,
            relocated_end_line as usize - 1,
        )
    {
        return alignment.line_placements;
    }

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
                original_line: Some(original_line),
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
            if anchor_line_hash_matches(anchor, offset, file.line_hashes[current_index]) {
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
                original_line: Some(original_line),
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
    stable_hash_id(input).to_string()
}

fn stable_hash_id(input: &str) -> LineHash {
    let mut hasher = gix::hash::hasher(HASH_KIND);
    hasher.update(input.as_bytes());
    hasher
        .try_finalize()
        .expect("hashing in-memory anchor content should not fail")
}

fn parsed_line_hashes(hashes: &[String]) -> Option<Vec<LineHash>> {
    hashes
        .iter()
        .map(|hash| gix::ObjectId::from_hex(hash.as_bytes()).ok())
        .collect()
}

fn anchor_line_hash_matches(anchor: &LineAnchor, offset: usize, file_hash: LineHash) -> bool {
    anchor
        .per_line_hashes
        .get(offset)
        .and_then(|hash| gix::ObjectId::from_hex(hash.as_bytes()).ok())
        == Some(file_hash)
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
        expected_line_placements: Vec<(Option<u32>, Option<u32>, AnchorLinePlacement)>,
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
        let mut large_gap_lines = lines(&["fn configure() {", "    let first = load();"]);
        for index in 0..60 {
            large_gap_lines.push(format!("    let inserted_{index} = true;"));
        }
        large_gap_lines.extend(lines(&[
            "    let second = prepare();",
            "    apply(first, second);",
            "    finish();",
            "}",
        ]));
        let derive_base = lines(&[
            "#[derive(Debug, Clone)]",
            "pub struct First;",
            "#[derive(Debug, Clone)]",
        ]);
        let mut derive_anchor = LineAnchor::new("src/derive.rs".to_string(), FileSide::New, 1, 3);
        capture_line_anchor_evidence(&mut derive_anchor, &derive_base, 0);
        let similarity_base = lines(&[
            "fn group() {",
            "    keep_one();",
            "    old_two();",
            "    old_three();",
            "    old_four();",
            "    old_five();",
            "    keep_six();",
            "}",
        ]);
        let mut low_similarity_anchor =
            LineAnchor::new("src/similarity.rs".to_string(), FileSide::New, 2, 7);
        capture_line_anchor_evidence(&mut low_similarity_anchor, &similarity_base, 0);
        let candidate_function_base = lines(&[
            "fn ordered_window_candidates() {",
            "    let mut candidates = Vec::new();",
            "    let current_lines_by_hash = current_lines_by_hash(file);",
            "    let candidate_originals = candidate_original_indexes(anchor);",
            "    for cluster in evidence_clusters() {",
            "        if let Some(candidate) = candidate_from_evidence_cluster() {",
            "            candidates.push(candidate);",
            "        }",
            "    }",
            "    candidates.sort_by_key(|candidate| (candidate.start, candidate.end));",
            "    candidates.dedup();",
            "    candidates",
            "}",
        ]);
        let mut candidate_function_anchor =
            LineAnchor::new("src/anchors.rs".to_string(), FileSide::New, 1, 13);
        capture_line_anchor_evidence(&mut candidate_function_anchor, &candidate_function_base, 0);
        let mut candidate_function_tail_anchor =
            LineAnchor::new("src/anchors.rs".to_string(), FileSide::New, 1, 12);
        capture_line_anchor_evidence(
            &mut candidate_function_tail_anchor,
            &candidate_function_base,
            0,
        );
        let candidate_function_with_comments = lines(&[
            "fn ordered_window_candidates() {",
            "    let mut candidates = Vec::new();",
            "    let current_lines_by_hash = current_lines_by_hash(file);",
            "    let candidate_originals = candidate_original_indexes(anchor);",
            "    // gap",
            "    // gap",
            "    for cluster in evidence_clusters() {",
            "        // gap",
            "        if let Some(candidate) = candidate_from_evidence_cluster() {",
            "            candidates.push(candidate);",
            "        }",
            "    }",
            "    candidates.sort_by_key(|candidate| (candidate.start, candidate.end));",
            "    candidates.dedup();",
            "    candidates",
            "}",
        ]);
        let candidate_function_with_comments_without_return = lines(&[
            "fn ordered_window_candidates() {",
            "    let mut candidates = Vec::new();",
            "    let current_lines_by_hash = current_lines_by_hash(file);",
            "    let candidate_originals = candidate_original_indexes(anchor);",
            "    // gap",
            "    // gap",
            "    for cluster in evidence_clusters() {",
            "        // gap",
            "        if let Some(candidate) = candidate_from_evidence_cluster() {",
            "            candidates.push(candidate);",
            "        }",
            "    }",
            "    candidates.sort_by_key(|candidate| (candidate.start, candidate.end));",
            "    candidates.dedup();",
            "}",
        ]);
        let mut large_gap_placements = vec![(Some(2), Some(2), AnchorLinePlacement::Content)];
        for current_line in 3..=62 {
            large_gap_placements.push((None, Some(current_line), AnchorLinePlacement::Gap));
        }
        large_gap_placements.extend([
            (Some(3), Some(63), AnchorLinePlacement::Content),
            (Some(4), Some(64), AnchorLinePlacement::Content),
        ]);

        let cases = vec![
            Case {
                name: "exact same range",
                anchor: anchor.clone(),
                files: files(&[("src/lib.rs", &base)]),
                expected_path: Some("src/lib.rs"),
                expected_start: Some(3),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::Exact,
                expected_line_placements: vec![(Some(3), Some(3), AnchorLinePlacement::Exact)],
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
                expected_line_placements: vec![(Some(3), Some(4), AnchorLinePlacement::Exact)],
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
                expected_line_placements: vec![(Some(3), Some(3), AnchorLinePlacement::Changed)],
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
                expected_line_placements: vec![(Some(3), Some(2), AnchorLinePlacement::Exact)],
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
                expected_line_placements: vec![(Some(3), Some(4), AnchorLinePlacement::Exact)],
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
                expected_line_placements: vec![(
                    Some(3),
                    Some(3),
                    AnchorLinePlacement::LineFallback,
                )],
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
                expected_line_placements: vec![(
                    Some(3),
                    Some(3),
                    AnchorLinePlacement::LineFallback,
                )],
            },
            Case {
                name: "file fallback when original line is gone",
                anchor: anchor.clone(),
                files: files(&[("src/lib.rs", &lines(&["mod tests {", "}"]))]),
                expected_path: Some("src/lib.rs"),
                expected_start: None,
                expected_end: None,
                expected_placement: AnchorPlacement::FileFallback,
                expected_line_placements: vec![(Some(3), None, AnchorLinePlacement::Missing)],
            },
            Case {
                name: "detached when file is gone",
                anchor: anchor.clone(),
                files: files(&[("src/other.rs", &lines(&["fn other() {}"]))]),
                expected_path: None,
                expected_start: None,
                expected_end: None,
                expected_placement: AnchorPlacement::Detached,
                expected_line_placements: vec![(Some(3), None, AnchorLinePlacement::Detached)],
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
                    (Some(2), Some(3), AnchorLinePlacement::Exact),
                    (Some(3), Some(4), AnchorLinePlacement::Exact),
                    (Some(4), Some(5), AnchorLinePlacement::Exact),
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
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), Some(3), AnchorLinePlacement::Changed),
                    (Some(4), Some(4), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline expanded window supports multiple bracketed gaps",
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
                expected_end: Some(6),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (None, Some(3), AnchorLinePlacement::Gap),
                    (Some(3), Some(4), AnchorLinePlacement::Content),
                    (None, Some(5), AnchorLinePlacement::Gap),
                    (Some(4), Some(6), AnchorLinePlacement::Content),
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
                    (Some(2), Some(3), AnchorLinePlacement::Content),
                    (Some(3), Some(4), AnchorLinePlacement::Changed),
                    (Some(4), Some(5), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline expanded window supports gap plus changed line",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    let inserted = true;",
                        "    let second = recompute();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(5),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (None, Some(3), AnchorLinePlacement::Gap),
                    (Some(3), Some(4), AnchorLinePlacement::Changed),
                    (Some(4), Some(5), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline expanded window marks deleted line as missing",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), None, AnchorLinePlacement::Missing),
                    (Some(4), Some(3), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "multiline expanded window marks inserted line as gap",
                anchor: multiline_anchor.clone(),
                files: files(&[(
                    "src/config.rs",
                    &lines(&[
                        "fn configure() {",
                        "    let first = load();",
                        "    let inserted = true;",
                        "    let second = prepare();",
                        "    apply(first, second);",
                        "    finish();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(5),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (None, Some(3), AnchorLinePlacement::Gap),
                    (Some(3), Some(4), AnchorLinePlacement::Content),
                    (Some(4), Some(5), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "low similarity window stays attached near origin",
                anchor: low_similarity_anchor.clone(),
                files: files(&[(
                    "src/similarity.rs",
                    &lines(&[
                        "fn group() {",
                        "    keep_one();",
                        "    new_two();",
                        "    new_three();",
                        "    new_four();",
                        "    new_five();",
                        "    keep_six();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/similarity.rs"),
                expected_start: Some(2),
                expected_end: Some(7),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), Some(3), AnchorLinePlacement::Changed),
                    (Some(4), Some(4), AnchorLinePlacement::Changed),
                    (Some(5), Some(5), AnchorLinePlacement::Changed),
                    (Some(6), Some(6), AnchorLinePlacement::Changed),
                    (Some(7), Some(7), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "low similarity window far from origin is stale",
                anchor: low_similarity_anchor,
                files: files(&[(
                    "src/similarity.rs",
                    &lines(&[
                        "fn group() {",
                        "    unrelated_one();",
                        "    unrelated_two();",
                        "    unrelated_three();",
                        "    unrelated_four();",
                        "    unrelated_five();",
                        "}",
                        "fn moved_group() {",
                        "    keep_one();",
                        "    new_two();",
                        "    new_three();",
                        "    new_four();",
                        "    new_five();",
                        "    keep_six();",
                        "}",
                    ]),
                )]),
                expected_path: Some("src/similarity.rs"),
                expected_start: Some(2),
                expected_end: Some(7),
                expected_placement: AnchorPlacement::LineFallback,
                expected_line_placements: vec![
                    (Some(2), Some(2), AnchorLinePlacement::LineFallback),
                    (Some(3), Some(3), AnchorLinePlacement::LineFallback),
                    (Some(4), Some(4), AnchorLinePlacement::LineFallback),
                    (Some(5), Some(5), AnchorLinePlacement::LineFallback),
                    (Some(6), Some(6), AnchorLinePlacement::LineFallback),
                    (Some(7), Some(7), AnchorLinePlacement::LineFallback),
                ],
            },
            Case {
                name: "comment insertions inside candidate function stay attached",
                anchor: candidate_function_anchor.clone(),
                files: files(&[("src/anchors.rs", &candidate_function_with_comments)]),
                expected_path: Some("src/anchors.rs"),
                expected_start: Some(1),
                expected_end: Some(16),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(1), Some(1), AnchorLinePlacement::Content),
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), Some(3), AnchorLinePlacement::Content),
                    (Some(4), Some(4), AnchorLinePlacement::Content),
                    (None, Some(5), AnchorLinePlacement::Gap),
                    (None, Some(6), AnchorLinePlacement::Gap),
                    (Some(5), Some(7), AnchorLinePlacement::Content),
                    (None, Some(8), AnchorLinePlacement::Gap),
                    (Some(6), Some(9), AnchorLinePlacement::Content),
                    (Some(7), Some(10), AnchorLinePlacement::Content),
                    (Some(8), Some(11), AnchorLinePlacement::Content),
                    (Some(9), Some(12), AnchorLinePlacement::Content),
                    (Some(10), Some(13), AnchorLinePlacement::Content),
                    (Some(11), Some(14), AnchorLinePlacement::Content),
                    (Some(12), Some(15), AnchorLinePlacement::Content),
                    (Some(13), Some(16), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "comment insertions plus deleted selected tail stay attached",
                anchor: candidate_function_tail_anchor,
                files: files(&[(
                    "src/anchors.rs",
                    &candidate_function_with_comments_without_return,
                )]),
                expected_path: Some("src/anchors.rs"),
                expected_start: Some(1),
                expected_end: Some(14),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: vec![
                    (Some(1), Some(1), AnchorLinePlacement::Content),
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), Some(3), AnchorLinePlacement::Content),
                    (Some(4), Some(4), AnchorLinePlacement::Content),
                    (None, Some(5), AnchorLinePlacement::Gap),
                    (None, Some(6), AnchorLinePlacement::Gap),
                    (Some(5), Some(7), AnchorLinePlacement::Content),
                    (None, Some(8), AnchorLinePlacement::Gap),
                    (Some(6), Some(9), AnchorLinePlacement::Content),
                    (Some(7), Some(10), AnchorLinePlacement::Content),
                    (Some(8), Some(11), AnchorLinePlacement::Content),
                    (Some(9), Some(12), AnchorLinePlacement::Content),
                    (Some(10), Some(13), AnchorLinePlacement::Content),
                    (Some(11), Some(14), AnchorLinePlacement::Content),
                    (Some(12), None, AnchorLinePlacement::Missing),
                ],
            },
            Case {
                name: "multiline expanded window allows large gaps between strong matches",
                anchor: multiline_anchor.clone(),
                files: files(&[("src/config.rs", &large_gap_lines)]),
                expected_path: Some("src/config.rs"),
                expected_start: Some(2),
                expected_end: Some(64),
                expected_placement: AnchorPlacement::Window,
                expected_line_placements: large_gap_placements,
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
                    (Some(2), Some(2), AnchorLinePlacement::Content),
                    (Some(3), Some(3), AnchorLinePlacement::Changed),
                    (Some(4), Some(4), AnchorLinePlacement::Content),
                ],
            },
            Case {
                name: "repeated low signal lines do not drive window relocation",
                anchor: derive_anchor,
                files: files(&[(
                    "src/derive.rs",
                    &lines(&[
                        "#[derive(Debug, Clone)]",
                        "pub struct Other;",
                        "#[derive(Debug, Clone)]",
                        "pub struct Gap;",
                        "pub struct First;",
                        "pub struct Gap2;",
                        "#[derive(Debug, Clone)]",
                    ]),
                )]),
                expected_path: Some("src/derive.rs"),
                expected_start: Some(1),
                expected_end: Some(3),
                expected_placement: AnchorPlacement::LineFallback,
                expected_line_placements: vec![
                    (Some(1), Some(1), AnchorLinePlacement::LineFallback),
                    (Some(2), Some(2), AnchorLinePlacement::LineFallback),
                    (Some(3), Some(3), AnchorLinePlacement::LineFallback),
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
