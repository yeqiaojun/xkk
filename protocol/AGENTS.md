# Protocol

- `proto/xkk.proto` owns all wire messages and the globally unique `MsgId` enum.
- `proto/model.proto` owns Mongo persistence models only.
- Rust must reuse the prost-generated `MsgId`; never add a handwritten numeric message ID table.
- Keep xproto system control IDs in `1..99` and XKK application IDs at `100+`.
- Use the pinned `protoc-bin-vendored` dependency and the shared `protoc-gen-xmongo-trait` library;
  generation must work on every supported host without `PATH` tools or Windows executables.
- After changing either proto or the xmongo generator, run `bash scripts/gen-proto.sh` and commit
  the refreshed files under `protocol/generated/`.
- Normal Cargo builds must fail when the checked-in generated sources are missing or stale.
