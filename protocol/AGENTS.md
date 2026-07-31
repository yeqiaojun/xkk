# Protocol

- `proto/xkk.proto` owns all wire messages and the globally unique `MsgId` enum.
- `proto/model.proto` owns Mongo persistence models only.
- Rust must reuse the prost-generated `MsgId`; never add a handwritten numeric message ID table.
- Keep control IDs in `1..99` and business IDs at `100+`.
- Generate with the pinned `tools/protoc.exe` and `tools/protoc-gen-xmongo-trait.exe` binaries in
  this repository only.
- After changing either proto or the xmongo generator, run `bash scripts/gen-proto.sh` and commit
  the refreshed files under `protocol/generated/`.
- Normal Cargo builds must fail when the checked-in generated sources are missing or stale.
