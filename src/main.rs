#[tokio::main]
async fn main() -> anyhow::Result<()> {
    peers::cli::run().await
}
