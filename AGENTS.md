# XKK Architecture

## Workspace

- `service/auth`, `service/logic`, `service/gate`, `service/public`, and `service/query` are independent deployable packages.
- `robot` is a one-shot login client. It must not grow stress, smoke, reconnect, logout, or business-module loops.
- `config` owns all typed YAML models, shared configuration fields, loading, validation, and log option conversion.
- Each service owns only its listeners, discovery watches, role-specific `xframe` composition, and application lifecycle.
- `cache` owns shared Redis protocols and depends directly on xredis; it must not know FrameHandle
  or mutate xservice. `persist` owns shared Mongo model access, and `common` contains
  infrastructure-free helpers.
- Do not introduce a shared bootstrap crate until repeated application behavior exists beyond `xframe`.
- All services connect etcd, Mongo, and Redis before admission and register their real published endpoint.

## Module Organization

- Keep each crate's `lib.rs` as its facade: module declarations, public re-exports, and only small
  crate-wide constants belong there.
- Move implementation into files named for a real responsibility. Keep modules private by default
  and preserve existing public paths with `pub use`.
- Do not split a cohesive implementation only because it is long. A deep module may have a large
  implementation when it keeps the caller-facing interface small and the behavior local.
- Do not widen the public interface for tests. Keep tests with the owning module or use
  `pub(crate)` only when a crate-level test needs internal state.

## Service Topology

- Gate dials Logic and Public.
- Logic accepts Gate and dials Public.
- Public accepts Gate and Logic.
- Query has no internal service connection.
- The acceptor watches dialer identities for Hello admission. Do not add mutual connection intents.

## Dynamic Service Load

- etcd owns instance discovery, endpoints, versions, health, and leases only. Online counts must not
  produce registry updates.
- Logic and Gate publish their own online count to one Redis Hash per cluster and service type,
  keyed by instance ID. The hash TTL covers three refresh intervals; graceful shutdown removes
  the publisher field.
- Gate refreshes discovered Logic counts and Auth refreshes discovered Gate counts, then overlays
  them into each process's local xservice snapshot through `FrameHandle::update_online_counts`.
- Consumers use one `HMGET` for the currently discovered instance IDs. A Redis read failure or one
  missing field keeps the last local value; discovery lease removal remains authoritative for
  removing dead instances and makes stale hash fields irrelevant to selection.

## Logic

- The bounded per-player Logic Runtime belongs in `xkk-logic`, not `deps-rust`.
- Only Player and Item are implemented. Add future gameplay modules under the Logic package without moving mailbox or persistence ownership into `xframe`.
- Keep global/per-player admission, dirty-player admission, same-gid serialization, and shutdown flush explicit and measurable.

## Player Protocol

- Each XKK process initializes one immutable global `xproto::MessageRegistry` containing control and
  application descriptors before `xframe::prepare`; do not install a client formatter or maintain a
  second logging-only protocol map.
- Player packets use `xproto::cs::CsPacket`. Their send/receive logs remain at xnet's single log
  point and include the protobuf type name, numeric message ID, CS header, and single-line JSON.

## Protocol Generation

- `proto/xkk.proto` owns wire messages and the globally unique `MsgId` enum used by front and back ends.
- `proto/model.proto` owns Mongo persistence models only. Do not split wire messages into more files without a concrete maintenance need.
- Never duplicate numeric message IDs in Rust. `xkk-protocol` must use the enum generated from `MsgId`.
- Generate every protocol artifact with the pinned binaries under this repository's `tools/`
  directory.
- Rust protobuf generation must use `tools/protoc.exe`, and Mongo persistence traits must use
  `tools/protoc-gen-xmongo-trait.exe`. Do not fall back to tools found through `PATH` or to a
  generator fetched through a dependency.
- Run `bash scripts/gen-proto.sh` after changing either proto or the xmongo generator, and commit
  the refreshed Rust sources under `protocol/generated/`.
- Keep this binding in `protocol/build.rs` so normal Cargo builds fail fast when either required
  binary is missing or the checked-in generated sources are stale.

## Configuration

- Each service loads its role-specific type from `xkk-config` and converts it once into `xframe::FrameConfig`.
- Keep role-specific storage, capacity, runtime, and listener fields explicit; share only fields with identical semantics.
- Capacities that bound retained work must be explicit in YAML. Do not silently fall back to library defaults.
- Fail fast on missing fields, invalid topology, invalid capacity, or unavailable infrastructure.

## Local Cluster

- `scripts/local-cluster.ps1` is the executable local acceptance path.
- A successful start requires all five processes, five etcd registrations, Auth and Query
  readiness, and the Gate-to-Logic, Gate-to-Public, and Logic-to-Public Hello-validated links.
- The start path must complete Auth login/use-role, Gate login/reconnect/logout, Logic player info,
  Public mail list, and Query gamer/config smoke checks before writing cluster state.
- The stop path must remove all five registrations and prove Gate/Logic retained work is zero.
- Keep generated PIDs and logs under `.run/`; do not write runtime artifacts into source packages.

## Future Work

- Add gameplay modules beyond Player and Item only when their authoritative state and persistence
  boundaries are explicit.
- Add batch player persistence through the existing Logic Runtime saver contract before increasing
  dirty-player throughput targets.
- Add an external metrics exporter and sustained load acceptance for the documented latency, CPU,
  RSS, queue, and shutdown budgets; structured logs remain the current local proof.
- Add multi-instance failover tests before enabling automatic service rerouting. Application RPC
  retries require business idempotency and must not be hidden in xframe or xservice.
