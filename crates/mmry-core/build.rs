fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "service")]
    {
        // Prefer a vendored protoc so cross-platform release builds need no
        // system protobuf-compiler (byt linux container + macOS runner). A
        // caller-provided PROTOC still wins.
        if std::env::var_os("PROTOC").is_none() {
            std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
        }

        // Compile proto files for service communication
        let proto_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../mmry-service/proto/embeddings.proto");

        if proto_path.exists() {
            tonic_build::compile_protos(proto_path)?;
        }
    }
    Ok(())
}
