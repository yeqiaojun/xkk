# Protocol

- `proto/xkk.proto` owns all wire messages and the globally unique `MsgId` enum.
- `proto/model.proto` owns Mongo persistence models only.
- Rust must reuse the prost-generated `MsgId`; never add a handwritten numeric message ID table.
- Keep control IDs in `1..99` and business IDs at `100+`.
- Generate with `C:\work\deps-rust\tools\protoc.exe` and `protoc-gen-xmongo-trait.exe` only.
