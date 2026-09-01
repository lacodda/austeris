//! Generates Rust from the `.proto` files that define the service contracts.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The contracts live at the repository root rather than inside this crate:
    // they are the interface between services, and `buf breaking` in CI will
    // check them from there against `main`.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proto");

    let files = ["identity/v1/identity.proto"];
    for file in files {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&files.map(|f| root.join(f)), &[root])?;

    Ok(())
}
