#![allow(dead_code)]

use std::path::{Path, PathBuf};

use anyhow::Result;

pub struct UiAsset {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

pub async fn load_asset(path: &str) -> Result<Option<UiAsset>> {
    let Some(dist_root) = frontend_dist_root() else {
        return Ok(None);
    };
    let relative = if path == "/" {
        Path::new("index.html").to_path_buf()
    } else {
        Path::new(path.trim_start_matches('/')).to_path_buf()
    };
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Ok(None);
    }

    let full_path = dist_root.join(relative);
    match tokio::fs::read(&full_path).await {
        Ok(bytes) => Ok(Some(UiAsset {
            content_type: content_type(&full_path),
            bytes,
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn frontend_dist_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dist = manifest_dir.join("frontend").join("dist");
    dist.is_dir().then_some(dist)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn fallback_index() -> &'static str {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Peers</title>
    <style>
      body {
        margin: 0;
        font-family: system-ui, sans-serif;
        background: #0f1115;
        color: #f5f7fb;
      }
      main {
        display: grid;
        min-height: 100vh;
        place-items: center;
        padding: 2rem;
      }
      section {
        max-width: 42rem;
        border: 1px solid #2c3340;
        border-radius: 8px;
        background: #171a21;
        padding: 1.25rem;
      }
      code {
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      }
    </style>
  </head>
  <body>
    <main>
      <section>
        <h1>Peers UI assets are not built</h1>
        <p>Run <code>cd frontend && bun run build</code>, then start the review command again.</p>
      </section>
    </main>
  </body>
</html>"#
}
