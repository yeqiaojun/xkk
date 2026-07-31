# Protocol Tools

These Windows binaries are checked in so protocol generation does not depend on a sibling
`deps-rust` checkout or a network download:

- `protoc.exe` compiles protobuf descriptors and is used by `prost-build`.
- `protoc-gen-xmongo-trait.exe` generates the xmongo BSON implementations.
- `protoc-gen-go-grpc.exe` is retained for future Go gRPC generation and is not currently invoked.

`protocol/build.rs` and `scripts/gen-proto.sh` must use these local binaries directly. Do not fall
back to `PATH`.
