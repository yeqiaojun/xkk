# XKK Server

XKK is a Rust workspace containing five independently deployable server packages and one login client:

- `xkk-auth`: HTTP account login, xtoken issuance, role admission, and Gate endpoint selection.
- `xkk-logic`: internal service listener and bounded per-player Logic Runtime.
- `xkk-gate`: configurable external TCP/KCP/WebSocket listeners.
- `xkk-public`: internal mail service and shared-state listener.
- `xkk-query`: HTTP gamer queries plus health and readiness probes.
- `xkk-robot`: one-shot Auth and Gate login through a configured TCP, KCP, or WebSocket endpoint.

Shared application crates are deliberately narrow:

- `xkk-config`: typed common/role YAML and version JSON composition, validation, five explicit service configs, and the robot config.
- `xkk-cache`: shared Redis player online-state, service-load, and login-queue protocols.
- `xkk-persist`: the shared Mongo collection catalog plus protobuf model loading and saving.
- `xkk-common`: infrastructure-free time and credential helpers.

Every service reads an explicitly selected role YAML plus `common.yaml` and `version.json` from the
same directory, connects etcd/Mongo/Redis through `xframe`, registers its role-specific endpoint,
emits periodic structured runtime stats, and shuts down through the shared `xframe` signal runner.

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

Protocol generation uses the pinned host-native `protoc-bin-vendored` dependency and calls the
shared `protoc-gen-xmongo-trait` library directly, so ordinary builds verify artifacts on every
supported host without relying on `PATH` or Windows executables.
`proto/xkk.proto` contains wire messages and the shared `MsgId` enum;
`proto/model.proto` contains persistence models only. Run `bash scripts/gen-proto.sh` after changing
either proto or the xmongo generator. The shell and PowerShell scripts refresh the reviewable Rust sources under
`protocol/generated/`, and normal Cargo builds fail when those checked-in files are stale.

The checked-in YAML files contain only deployment-varying identity, listeners, DSNs, secrets, and
typed log settings. Stable limits, timeouts, windows, TTLs, and task periods are commented hard
constants beside the code that enforces them; saturation is rejected or dropped with an error log.
Replace the shared DSNs in `common.yaml`, and select the desired instance YAML explicitly at startup.
Mongo DSNs must include the database. `version.json` supplies
the configuration version; builds may inject the program version with `XKK_PRO_VERSION` and
otherwise use `0`.
