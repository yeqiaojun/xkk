# Protocol Tools

These legacy Windows binaries are retained only for compatibility with older development
environments:

- `protoc.exe` is an old Windows protobuf compiler.
- `protoc-gen-xmongo-trait.exe` is an old Windows xmongo generator.
- `protoc-gen-go-grpc.exe` is retained for future Go gRPC generation and is not currently invoked.

Current Cargo builds and generation scripts do not execute these files. They resolve a host-native
compiler through `protoc-bin-vendored` and call the shared xmongo generator as a Rust library.
