# Configuration

- This crate owns all service YAML types, loading, fail-fast validation, and shared log conversion.
- Keep `AuthConfig`, `GateConfig`, `LogicConfig`, `PublicConfig`, `QueryConfig`, and the standalone `RobotConfig` explicit. Service assembly differs and must not be hidden behind a dynamic trait or generic map.
- Share a field type only when its YAML shape and semantics are identical across services.
- YAML contains only deployment-varying identity, listeners, DSNs, secrets, and typed log fields.
  Stable process limits, windows, TTLs, timeouts, and task periods belong as commented constants in
  the module that enforces them; do not add generic `runtime` or `capacity` sections.
- Common Configuration may own a value only when semantics, validation, change reason, and
  lifecycle are identical for every consumer. Sharing a Rust type alone does not justify moving
  role values into `common.yaml`.
- Do not move listeners, discovery watches, `xframe::Application`, or service startup into this crate.
- Reject unknown retired operational sections so stale files fail fast instead of being ignored.
