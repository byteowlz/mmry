fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "service")]
    {
        // Compile proto files for service communication
        let proto_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../mmry-service/proto/embeddings.proto");

        if proto_path.exists() {
            tonic_build::compile_protos(proto_path)?;
        }
    }
    Ok(())
}
