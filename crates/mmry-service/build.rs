fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Prefer a vendored protoc so cross-platform release builds need no system
    // protobuf-compiler (byt linux container + macOS runner). A caller-provided
    // PROTOC still wins.
    if std::env::var_os("PROTOC").is_none() {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    }
    tonic_build::compile_protos("proto/embeddings.proto")?;
    Ok(())
}
