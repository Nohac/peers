use std::path::PathBuf;

const TX_RX_IMPORT: &str = "import { Tx, Rx, argElementRefsForMethod, bindChannelsForTypeRefs, finalizeBoundChannelsForTypeRefs } from \"@bearcove/vox-core\";";
const TX_IMPORT: &str = "import { Tx, argElementRefsForMethod, bindChannelsForTypeRefs, finalizeBoundChannelsForTypeRefs } from \"@bearcove/vox-core\";";
const RX_TYPE_PREFIX: &str = "Rx<";

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=../../src/rpc.rs");
    println!("cargo:rerun-if-changed=../../src/diff.rs");
    println!("cargo:rerun-if-changed=../../src/comments.rs");

    let service = peers::rpc::peers_review_service_descriptor();
    let mut ts = vox_codegen::targets::typescript::generate_service(service);
    if !ts.contains(RX_TYPE_PREFIX) {
        ts = ts.replace(TX_RX_IMPORT, TX_IMPORT);
    }
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
