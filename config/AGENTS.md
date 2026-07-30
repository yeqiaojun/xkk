# Configuration

- This crate owns all service YAML types, loading, fail-fast validation, and shared log conversion.
- Keep `AuthConfig`, `GateConfig`, `LogicConfig`, `PublicConfig`, `QueryConfig`, and the standalone `RobotConfig` explicit. Service assembly differs and must not be hidden behind a dynamic trait or generic map.
- Share a field type only when its YAML shape and semantics are identical across services.
- Do not move listeners, discovery watches, `xframe::Application`, or service startup into this crate.
- Every retained-work capacity remains mandatory in YAML; do not add silent defaults.
