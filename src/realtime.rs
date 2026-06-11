use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use facet::Facet;
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{broadcast, mpsc};
use tokio::time;
use tracing::{debug, instrument};

use crate::review::{peers_paths, regenerate_outputs};

const REVIEW_CHANGED: &str = "review_changed";
const DIFF_CHANGED: &str = "diff_changed";
const BROADCAST_CAPACITY: usize = 256;
const WATCH_CHANNEL_CAPACITY: usize = 256;
const WATCH_DEBOUNCE_MS: u64 = 120;
const LOCAL_REVIEW_WATCH_SUPPRESSION_MS: u64 = 750;
const WATCHER_CREATE_ERROR: &str = "failed to create Peers realtime watcher";
const WATCH_REPO_ERROR: &str = "failed to watch repository for Peers realtime updates";
const WATCH_EVENTS_ERROR: &str = "failed to watch Peers review event log";
const WATCH_EVENT_WARNING: &str = "Peers realtime watch event failed";
const WATCH_REFRESH_WARNING: &str = "Peers realtime watch refresh failed";
const WATCHER_START_ERROR: &str = "Peers realtime notify watcher unavailable";
const GITIGNORE_BUILD_ERROR: &str = "failed to build Peers realtime gitignore matcher";
const REGENERATE_OUTPUTS_ERROR: &str = "failed to regenerate review outputs after event log change";
const DOT_GIT_DIR: &str = ".git";
const DOT_PEERS_DIR: &str = ".peers";
const GITIGNORE_FILE: &str = ".gitignore";
const GIT_INDEX_FILE: &str = "index";
const PATH_UTF8_ERROR: &str = "repository path is not valid UTF-8";

#[derive(Clone, Debug, Facet)]
pub struct ReviewUpdate {
    pub kind: String,
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct ReviewUpdateBroadcaster {
    sender: broadcast::Sender<ReviewUpdate>,
    sequence: Arc<AtomicU64>,
    last_local_review_changed_ms: Arc<AtomicU64>,
}

impl ReviewUpdateBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            sequence: Arc::new(AtomicU64::new(0)),
            last_local_review_changed_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ReviewUpdate> {
        self.sender.subscribe()
    }

    pub fn notify_review_changed(&self) {
        self.notify(REVIEW_CHANGED);
    }

    pub fn mark_local_review_changed(&self) {
        self.last_local_review_changed_ms
            .store(now_millis(), Ordering::Relaxed);
    }

    pub fn notify_diff_changed(&self) {
        self.notify(DIFF_CHANGED);
    }

    fn should_suppress_watched_review_changed(&self) -> bool {
        let last = self.last_local_review_changed_ms.load(Ordering::Relaxed);
        last != 0 && now_millis().saturating_sub(last) <= LOCAL_REVIEW_WATCH_SUPPRESSION_MS
    }

    fn notify(&self, kind: &str) {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        debug!(kind, sequence, "broadcasting review update");
        let _ = self.sender.send(ReviewUpdate {
            kind: kind.to_string(),
            sequence,
        });
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

impl Default for ReviewUpdateBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_realtime_watcher(
    repo_root: PathBuf,
    updates: ReviewUpdateBroadcaster,
) -> Result<()> {
    let paths = peers_paths(&repo_root);
    let events_path = paths.events;
    let threads_path = paths.threads;
    let peers_dir = repo_root.join(DOT_PEERS_DIR);
    let git_dir = repo_root.join(DOT_GIT_DIR);
    let gitignore = build_gitignore(&repo_root).context(GITIGNORE_BUILD_ERROR)?;
    let (mut watcher, mut watch_rx, mut watched_dirs) =
        start_notify_watcher(&repo_root, &events_path, &threads_path)
            .context(WATCHER_START_ERROR)?;

    let mut snapshot = WatchSnapshot::capture(&repo_root, &events_path);

    loop {
        let mut pending = PendingUpdate::default();

        let Some(event) = watch_rx.recv().await else {
            return Ok(());
        };
        let mut should_capture_snapshot = classify_watch_result(
            event,
            &events_path,
            &threads_path,
            &repo_root,
            &peers_dir,
            &git_dir,
            &gitignore,
            &mut pending,
        );

        time::sleep(Duration::from_millis(WATCH_DEBOUNCE_MS)).await;
        while let Ok(event) = watch_rx.try_recv() {
            should_capture_snapshot |= classify_watch_result(
                event,
                &events_path,
                &threads_path,
                &repo_root,
                &peers_dir,
                &git_dir,
                &gitignore,
                &mut pending,
            );
        }
        if should_capture_snapshot {
            snapshot.capture_changes(&repo_root, &events_path, &mut pending);
            if pending.diff_changed {
                if let Err(error) = watch_repo_paths(&mut watcher, &repo_root, &mut watched_dirs) {
                    eprintln!("{WATCH_REFRESH_WARNING}: {error:#}");
                }
            }
        }
        if !pending.review_changed && !pending.diff_changed {
            continue;
        }

        publish_pending(&repo_root, &updates, pending).await?;
    }
}

#[instrument(
    name = "realtime.publish_pending",
    skip_all,
    fields(
        review_changed = pending.review_changed,
        diff_changed = pending.diff_changed
    )
)]
async fn publish_pending(
    repo_root: &Path,
    updates: &ReviewUpdateBroadcaster,
    pending: PendingUpdate,
) -> Result<()> {
    if pending.review_changed {
        regenerate_outputs(repo_root, None)
            .await
            .context(REGENERATE_OUTPUTS_ERROR)?;
        if updates.should_suppress_watched_review_changed() {
            debug!("suppressing watched review update after local provider update");
        } else {
            updates.notify_review_changed();
        }
    }
    if pending.diff_changed {
        updates.notify_diff_changed();
    }
    Ok(())
}

fn start_notify_watcher(
    repo_root: &Path,
    events_path: &Path,
    threads_path: &Path,
) -> Result<(
    RecommendedWatcher,
    mpsc::Receiver<notify::Result<Event>>,
    BTreeSet<PathBuf>,
)> {
    let (watch_tx, watch_rx) = mpsc::channel(WATCH_CHANNEL_CAPACITY);
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = watch_tx.blocking_send(event);
    })
    .context(WATCHER_CREATE_ERROR)?;
    let mut watched_dirs = BTreeSet::new();
    watch_repo_paths(&mut watcher, repo_root, &mut watched_dirs).context(WATCH_REPO_ERROR)?;
    watcher
        .watch(events_path, RecursiveMode::NonRecursive)
        .context(WATCH_EVENTS_ERROR)?;
    if threads_path.exists() {
        watcher
            .watch(threads_path, RecursiveMode::Recursive)
            .context(WATCH_EVENTS_ERROR)?;
    }
    Ok((watcher, watch_rx, watched_dirs))
}

fn watch_repo_paths(
    watcher: &mut RecommendedWatcher,
    repo_root: &Path,
    watched_dirs: &mut BTreeSet<PathBuf>,
) -> notify::Result<()> {
    let directories = git_visible_directories(repo_root);
    if directories.is_empty() {
        watcher.watch(repo_root, RecursiveMode::Recursive)?;
        return Ok(());
    }

    for directory in directories {
        if !directory.is_dir() || watched_dirs.contains(&directory) {
            continue;
        }
        watcher.watch(&directory, RecursiveMode::NonRecursive)?;
        watched_dirs.insert(directory);
    }
    Ok(())
}

fn git_visible_directories(repo_root: &Path) -> BTreeSet<PathBuf> {
    let mut directories = BTreeSet::from([repo_root.to_path_buf()]);
    for relative_path in git_visible_paths(repo_root) {
        let mut path = repo_root.join(relative_path);
        while path.pop() && path.starts_with(repo_root) {
            directories.insert(path.clone());
            if path == repo_root {
                break;
            }
        }
    }
    directories
}

#[derive(Clone, Debug, Default, PartialEq)]
struct WatchSnapshot {
    tree: RepoFingerprint,
    events: FileFingerprint,
}

impl WatchSnapshot {
    fn capture(repo_root: &Path, events_path: &Path) -> Self {
        Self {
            tree: RepoFingerprint::capture(repo_root),
            events: FileFingerprint::capture(events_path),
        }
    }

    fn capture_changes(
        &mut self,
        repo_root: &Path,
        events_path: &Path,
        pending: &mut PendingUpdate,
    ) {
        let next = Self::capture(repo_root, events_path);
        if self.events != next.events {
            debug!("watch snapshot detected event log change");
            pending.review_changed = true;
        }
        if self.tree != next.tree {
            debug!(
                previous_hash = self.tree.hash,
                next_hash = next.tree.hash,
                "watch snapshot detected repo diff change"
            );
            pending.diff_changed = true;
        }
        *self = next;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct RepoFingerprint {
    hash: u64,
}

impl RepoFingerprint {
    fn capture(repo_root: &Path) -> Self {
        let mut hasher = DefaultHasher::new();

        FileFingerprint::capture(&repo_root.join(DOT_GIT_DIR).join(GIT_INDEX_FILE))
            .hash(&mut hasher);

        let visible_paths = git_visible_paths(repo_root);
        visible_paths.hash(&mut hasher);
        for relative_path in visible_paths {
            FileFingerprint::capture(&repo_root.join(relative_path)).hash(&mut hasher);
        }

        Self {
            hash: hasher.finish(),
        }
    }
}

fn git_visible_paths(repo_root: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    collect_index_paths(repo_root, &mut paths);
    collect_worktree_paths(repo_root, &mut paths);
    paths.into_iter().collect()
}

fn collect_index_paths(repo_root: &Path, paths: &mut BTreeSet<PathBuf>) {
    let Ok(repo) = gix::discover(repo_root) else {
        return;
    };
    let Ok(index) = repo.index_or_empty() else {
        return;
    };
    for entry in index.entries() {
        if entry.stage_raw() != 0 {
            continue;
        }
        let path = entry.path(&index);
        let Ok(path) = String::from_utf8(path.to_vec()).context(PATH_UTF8_ERROR) else {
            continue;
        };
        let path = PathBuf::from(path);
        if !path.starts_with(DOT_PEERS_DIR) {
            paths.insert(path);
        }
    }
}

fn collect_worktree_paths(repo_root: &Path, paths: &mut BTreeSet<PathBuf>) {
    for entry in WalkBuilder::new(repo_root)
        .hidden(false)
        .filter_entry(|entry| !is_internal_entry(entry.path()))
        .build()
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let path = entry.path().strip_prefix(repo_root).unwrap_or(entry.path());
        if path.starts_with(DOT_PEERS_DIR) {
            continue;
        }
        paths.insert(path.to_path_buf());
    }
}

fn is_internal_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == DOT_GIT_DIR || name == DOT_PEERS_DIR)
}

#[derive(Clone, Debug, Default, Hash, PartialEq)]
struct FileFingerprint {
    exists: bool,
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

impl FileFingerprint {
    fn capture(path: &Path) -> Self {
        let Ok(metadata) = path.metadata() else {
            return Self::default();
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        Self {
            exists: true,
            len: metadata.len(),
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
        }
    }
}

#[derive(Default)]
struct PendingUpdate {
    review_changed: bool,
    diff_changed: bool,
}

fn classify_watch_result(
    event: notify::Result<Event>,
    events_path: &Path,
    threads_path: &Path,
    repo_root: &Path,
    peers_dir: &Path,
    git_dir: &Path,
    gitignore: &Gitignore,
    pending: &mut PendingUpdate,
) -> bool {
    match event {
        Ok(event) => classify_event(
            event,
            events_path,
            threads_path,
            repo_root,
            peers_dir,
            git_dir,
            gitignore,
            pending,
        ),
        Err(error) => {
            eprintln!("{WATCH_EVENT_WARNING}: {error:#}");
            false
        }
    }
}

fn classify_event(
    event: Event,
    events_path: &Path,
    threads_path: &Path,
    repo_root: &Path,
    peers_dir: &Path,
    git_dir: &Path,
    gitignore: &Gitignore,
    pending: &mut PendingUpdate,
) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        debug!(kind = ?event.kind, "ignoring access watch event");
        return false;
    }

    let kind = event.kind.clone();
    for path in event.paths {
        if same_path(&path, events_path) {
            debug!(kind = ?kind, path = %path.display(), "classified watch path as review event log");
            pending.review_changed = true;
            continue;
        }
        if path.starts_with(peers_dir) {
            if path.starts_with(threads_path) {
                debug!(kind = ?kind, path = %path.display(), "classified watch path as review thread payload");
                pending.review_changed = true;
            } else {
                debug!(kind = ?kind, path = %path.display(), "ignoring internal peers watch path");
            }
            continue;
        }
        if path.starts_with(git_dir) {
            debug!(kind = ?kind, path = %path.display(), "ignoring git internal watch path");
            continue;
        }
        let gitignore_path = path.strip_prefix(repo_root).unwrap_or(&path);
        if gitignore
            .matched_path_or_any_parents(gitignore_path, path.is_dir())
            .is_ignore()
        {
            debug!(kind = ?kind, path = %path.display(), "ignoring gitignored watch path");
            continue;
        }
        debug!(kind = ?kind, path = %path.display(), "classified watch path as possible diff change");
    }
    true
}

fn build_gitignore(repo_root: &Path) -> Result<Gitignore, ignore::Error> {
    let mut builder = GitignoreBuilder::new(repo_root);
    let gitignore_path = repo_root.join(GITIGNORE_FILE);
    if gitignore_path.is_file() {
        if let Some(error) = builder.add(gitignore_path) {
            return Err(error);
        }
    }
    builder.build()
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) else {
        return false;
    };
    left == right
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use notify::event::{AccessKind, AccessMode};

    const TEST_FILE: &str = "src/cli.rs";
    const TEST_EVENTS: &str = ".peers/events.jsonl";
    const TEST_WATCHER_STARTUP_MS: u64 = 1000;

    #[test]
    fn local_review_updates_suppress_immediate_watched_review_update() {
        let updates = ReviewUpdateBroadcaster::new();

        assert!(!updates.should_suppress_watched_review_changed());
        updates.mark_local_review_changed();

        assert!(updates.should_suppress_watched_review_changed());
    }

    #[test]
    fn access_watch_events_do_not_request_snapshot_refresh() {
        let root = test_root("access_event");
        let events_path = root.join(TEST_EVENTS);
        let threads_path = root.join(".peers/threads");
        let peers_dir = root.join(DOT_PEERS_DIR);
        let git_dir = root.join(DOT_GIT_DIR);
        let gitignore = build_gitignore(&root).unwrap();
        let event = Event::new(EventKind::Access(AccessKind::Open(AccessMode::Any)))
            .add_path(root.join("src"));
        let mut pending = PendingUpdate::default();

        let should_capture_snapshot = classify_watch_result(
            Ok(event),
            &events_path,
            &threads_path,
            &root,
            &peers_dir,
            &git_dir,
            &gitignore,
            &mut pending,
        );

        assert!(!should_capture_snapshot);
        assert!(!pending.review_changed);
        assert!(!pending.diff_changed);
    }

    #[tokio::test]
    async fn realtime_watcher_broadcasts_diff_change_after_snapshot() {
        let root = test_root("diff_change");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".peers")).unwrap();
        gix::init(&root).unwrap();
        fs::write(root.join(TEST_FILE), "before\n").unwrap();
        fs::write(root.join(TEST_EVENTS), "").unwrap();

        let updates = ReviewUpdateBroadcaster::new();
        let mut receiver = updates.subscribe();
        let watcher_updates = updates.clone();
        let watcher_root = root.clone();
        let watcher = tokio::spawn(async move {
            let _ = run_realtime_watcher(watcher_root, watcher_updates).await;
        });

        time::sleep(Duration::from_millis(TEST_WATCHER_STARTUP_MS)).await;
        fs::write(root.join(TEST_FILE), "after\n").unwrap();

        let update = time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.kind, DIFF_CHANGED);

        watcher.abort();
        let _ = fs::remove_dir_all(root);
    }

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("peers_realtime_{name}_{nonce}"))
    }
}
