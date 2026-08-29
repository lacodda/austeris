// Build script for generating gRPC code
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::compile_protos("proto/asset.proto")?;
    Ok(())
}