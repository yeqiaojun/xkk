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

**Robot Client**:
The standalone one-shot client that exercises login behavior without sharing the Service Configuration model.
_Avoid_: robot service, service package

**Published Endpoint**:
The role-specific network endpoint advertised by a Service Package through discovery. A Published Endpoint may speak client, HTTP, or internal service protocol.
_Avoid_: internal listener, xservice connection

**Gate Listener Set**:
The configured external Gate endpoints, with at most one listener and one port for each enabled transport. A Gate Listener Set must enable at least one transport.
_Avoid_: internal service listener, shared transport port

**Service Configuration**:
The typed startup settings composed from Common Configuration and one Role Configuration, then consumed once before a Service Package starts accepting traffic.
_Avoid_: runtime configuration provider, configuration getter trait

**Common Configuration**:
Cluster-wide startup settings shared by every Service Package in one deployment.
_Avoid_: base config, global server config

**Role Configuration**:
Startup settings that vary for exactly one server role and combine with Common Configuration into its Service Configuration.
_Avoid_: server config, service override

**Program Version**:
The non-negative release identifier injected when the server binaries are compiled.
_Avoid_: protocol version, YAML version

**Configuration Version**:
The non-negative identifier read from the deployment's version document when a Service Package starts.
_Avoid_: config key, manifest version

**Persistence Collection Catalog**:
The process-local catalog that initializes every declared Mongo collection from the DSN database and provides mutable collection handles without role-based access restrictions.
_Avoid_: service repository, collection configuration

**Public Player Data**:
The player-scoped aggregate owned by Public, containing Mail data now and future Public player data as those domains are introduced.
_Avoid_: mail cache entry, public player lock

**Public Owner**:
The single Public Service Package instance responsible for one player's Public Player Data at a given time.
_Avoid_: mail server, arbitrary Public instance

**Hard Limit**:
A reviewed code constant owned by the XKK module that retains or admits bounded work. Reaching it rejects or drops work and emits an error log; it is not a deployment setting.
_Avoid_: YAML tuning knob, implicit library default
