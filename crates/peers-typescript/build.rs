use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=../../src/rpc.rs");
    println!("cargo:rerun-if-changed=../../src/diff.rs");
    println!("cargo:rerun-if-changed=../../src/comments.rs");

    let service = peers::rpc::peers_review_service_descriptor();
    let ts = vox_codegen::targets::typescript::generate_service(service);
    let output = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?)
        .join("..")
        .join("..")
        .join("frontend")
        .join("src")
        .join("features")
        .join("review")
        .join("peersReviewClient.gen.ts");
    std::fs::write(output, ts)?;
    Ok(())
}
