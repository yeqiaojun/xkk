# XKK Server

XKK is a Rust workspace containing five independently deployable server packages and one login client:

- `xkk-auth`: HTTP account login, xtoken issuance, role admission, and Gate endpoint selection.
- `xkk-logic`: internal service listener and bounded per-player Logic Runtime.
- `xkk-gate`: configurable external TCP/KCP/WebSocket listeners.
- `xkk-public`: internal mail service and shared-state listener.
- `xkk-query`: HTTP gamer/config queries plus health and readiness probes.
- `xkk-robot`: one-shot Auth and Gate login through a configured TCP, KCP, or WebSocket endpoint.

Shared application crates are deliberately narrow:

- `xkk-config`: typed YAML loading, validation, common node/infrastructure/log fields, five explicit service configs, and the robot config.
- `xkk-cache`: shared Redis player online-state, service-load, and login-queue protocols.
- `xkk-persist`: shared Mongo protobuf model loading and saving.
- `xkk-common`: infrastructure-free time and credential helpers.

Every service reads its own YAML file, connects etcd/Mongo/Redis through `xframe`, registers its
role-specific endpoint, emits periodic structured runtime stats, and shuts down through the shared
`xframe` signal runner.

etcd carries stable discovery and lease data. Logic and Gate publish online counts to TTL-backed
Redis hashes keyed by instance ID; Gate and Auth periodically load discovered instance fields into
their own local xservice snapshots for minimum-online routing without generating etcd watch
updates.

Run a service with:

```powershell
cargo run -p xkk-logic -- --config config/logic.yaml
```

Run the one-shot login robot with:

```powershell
cargo run -p xkk-robot -- --config config/robot.yaml
```

It performs Auth login, bounded use-role queue waiting, Gate login, and connection shutdown. It does
not run smoke, pressure, reconnect, logout, or gameplay requests.

Start and verify the complete local cluster against the local etcd, MongoDB, and Redis instances:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\local-cluster.ps1
```

The command builds all binaries, starts the five services in dependency order, verifies all etcd
registrations and the three expected Hello-validated service-link edges, then runs HTTP and real
`xnet` business smoke checks across Auth, Gate, Logic, Public, and Query. A successful Start leaves
the cluster running. Use `-Action Status` to inspect it and `-Action Stop` to require graceful drain
and immediate etcd deregistration. Runtime logs, smoke identity, and PIDs are kept under `.run/`.

Protocol generation is deliberately pinned to the checked-in `tools/protoc.exe` and
`tools/protoc-gen-xmongo-trait.exe`; normal Cargo builds fail when either tool is missing.
`tools/protoc-gen-go-grpc.exe` is also retained locally for future Go gRPC generation.
`proto/xkk.proto` contains wire messages and the shared `MsgId` enum;
`proto/model.proto` contains persistence models only. Run `bash scripts/gen-proto.sh` after changing
either proto or the xmongo generator. The script refreshes the reviewable Rust sources under
`protocol/generated/`, and normal Cargo builds fail when those checked-in files are stale.

The checked-in YAML files are explicit capacity contracts and local examples. Replace their DSNs,
advertised hosts, and ports for each environment.
