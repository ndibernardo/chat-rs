fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate gRPC code from proto files
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["../proto/user.proto"], &["../proto"])?;

    Ok(())
}
