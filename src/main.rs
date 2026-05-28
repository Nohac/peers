mod cli;
mod comments;
mod diff;
mod review;
mod rpc;
mod server;
mod ui_assets;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
