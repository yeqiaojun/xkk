# Logic Service

- This package owns the bounded per-player Logic Runtime and its persistence lifecycle.
- Player and Item are the only migrated gameplay modules. Do not migrate other Product modules
  without a separate scope decision.
- Every player RPC must submit through `LogicRuntime`; never spawn independent same-gid work.
- YAML capacities are mandatory and must map directly to `LogicConfig` and xnet/xrpc budgets.
- Shutdown must stop Logic admission and flush dirty players before xrpc and storage close.
- Login arbitration, old-session kick routing, player persistence, online ownership, and final
  dirty/inflight metrics remain owned here.
- Logic publishes its local online count to the shared TTL-backed Redis service-load protocol;
  metrics logging and etcd registration remain independent of that publisher.

## Future Work

- Supply a batch persistence implementation through the existing saver contract when measurements
  justify it. Keep Mongo I/O outside the player lock.
- Add gameplay modules as cohesive state transitions on `PlayerState`, not as parallel runtimes.
