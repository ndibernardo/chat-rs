{
  description = "chat-rs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        {
          self,
          system,
          ...
        }:
        let
          overlays = [ (import inputs.rust-overlay) ];
          pkgs = import inputs.nixpkgs {
            inherit system overlays;
          };

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
            ];
          };

          buildInputs =
            with pkgs;
            [
              rustToolchain
              pkg-config
              cmake
              openssl
              postgresql
              openssl.dev
              protobuf
              nixd
              nixfmt
              sqlx-cli
              openapi-generator-cli
              jq
              websocat
            ]
            ++ lib.optionals stdenv.isDarwin [
              apple-sdk_12
            ];

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        in
        {
          devShells.default = pkgs.mkShell {
            inherit buildInputs nativeBuildInputs;

            shellHook = ''
              echo "Rust development environment loaded"
              echo "Rust version: $(rustc --version)"
              echo "Cargo version: $(cargo --version)"
            '';
          };
        };
    };
}
