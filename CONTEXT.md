# XKK Server

XKK is the application server context built from the reusable runtime modules in `deps-rust`.

## Language

**Logic Runtime**:
The application-owned execution context that serializes one player's commands and owns that player's persistence lifecycle.
_Avoid_: xlogic crate, gameplay module

**Gameplay Module**:
A domain-specific set of player rules executed by the Logic Runtime. Gameplay Modules are outside the initial server-framework scope.
_Avoid_: runtime, service framework

**Service Package**:
A deployable Rust package for exactly one server role, with its own startup configuration and process composition.
_Avoid_: service mode, shared server binary

**Published Endpoint**:
The role-specific network endpoint advertised by a Service Package through discovery. A Published Endpoint may speak client, HTTP, or internal service protocol.
_Avoid_: internal listener, xservice connection

**Gate Listener Set**:
The configured external Gate endpoints, with at most one listener and one port for each enabled transport. A Gate Listener Set must enable at least one transport.
_Avoid_: internal service listener, shared transport port

**Service Configuration**:
The typed startup settings owned by one Service Package and consumed once before that process starts accepting traffic.
_Avoid_: runtime configuration provider, configuration getter trait

**Capacity Budget**:
An explicit startup limit that bounds retained work or runtime resources for a Service Package.
_Avoid_: tuning hint, implicit library default
