# Public Service

- Public binds one internal SS TCP listener.
- Public watches Gate and Logic only as inbound Hello allowlists.
- Gate and Logic own the outbound connection intents; do not add reverse dialing here.
- Public owns Mail list/read/delete/claim/send/push and the Mongo mail document.
- A per-gid Redis lease serializes mail mutation. Claim persists the claimed state before calling
  Logic AddItems; a downstream failure is not rolled back or retried implicitly.

## Future Work

- Add an auditable delivery/compensation record before changing claim failure semantics.
- Add other shared-state modules only when they do not belong to the player-authoritative Logic
  state.
