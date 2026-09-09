# Public Service

- Public binds one internal SS TCP listener.
- Public watches Gate and Logic only as inbound Hello allowlists.
- Gate and Logic own the outbound connection intents; do not add reverse dialing here.
- Public owns Mail list/read/delete/claim/send/push over the persisted Public Player Data aggregate.
- Gate pins each player to the Public instance selected by the gid consistent hash. Public loads
  that aggregate through xlru and serializes access with its process-local read/write lock; do not
  add a Redis lease or a separate player-lock registry. Topology changes must drain old player
  routes before allowing overlapping ownership.
- Public records changed gids in a concurrent dirty set and flushes them to Mongo in batches every
  two minutes using a monotonic-time task and once during graceful shutdown after
  xrpc incoming handlers have drained. Stop the periodic task cooperatively between flushes; never abort an active save. Saving clears dirty state before I/O; failures are
  logged and treated as saved without retry or rollback. This deliberately accepts crash-window and
  active-entry-eviction data loss in exchange for throughput.
- A missing Mongo document loads as a clean empty Public Player Data aggregate. Reads do not create
  a document; the first actual business mutation marks the aggregate dirty for deferred upsert.
- Claim follows the same deferred persistence policy as other mutations. A downstream Logic failure
  is not rolled back or retried implicitly.

## Future Work

- Add an auditable delivery/compensation record before changing claim failure semantics.
- Add other shared-state modules only when they do not belong to the player-authoritative Logic
  state.
