use std::fs::OpenOptions;
use std::path::Path;
use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

const PEERS_LOG_ENV: &str = "PEERS_LOG";
const RUST_LOG_ENV: &str = "RUST_LOG";
const DEFAULT_FILTER: &str = "warn";

static INIT: OnceLock<()> = OnceLock::new();

pub fn init() {
    INIT.get_or_init(|| {
        let filter = env_filter();
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(FmtSpan::CLOSE)
            .try_init();
    });
}

pub fn init_file(path: &Path) {
    INIT.get_or_init(|| {
        let filter = env_filter();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(file) = OpenOptions::new().create(true).append(true).open(path) else {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_span_events(FmtSpan::CLOSE)
                .try_init();
            return;
        };
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(FmtSpan::CLOSE)
            .with_ansi(false)
            .with_writer(file)
            .try_init();
    });
}

fn env_filter() -> EnvFilter {
    let filter = std::env::var(PEERS_LOG_ENV)
        .or_else(|_| std::env::var(RUST_LOG_ENV))
        .unwrap_or_else(|_| DEFAULT_FILTER.to_string());
    EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}
