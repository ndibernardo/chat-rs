fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&["../../contracts/proto/user.proto"], &["../../contracts/proto"])?;
    Ok(())
}
