fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate gRPC code from proto files
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile(&["../proto/user.proto"], &["../proto"])?;

    Ok(())
}
