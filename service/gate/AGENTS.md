# Gate Service

- Gate binds only external client listeners; it has no internal service listener.
- TCP, KCP, and WebSocket may be enabled together with one configured port per transport.
- Gate is the dial owner for Gate-to-Logic and Gate-to-Public links.
- Gate publishes its own online count to Redis and refreshes discovered Logic counts into its
  local xservice snapshot. A new-player Logic selection increments the selected local snapshot
  count before dispatch, and the next Redis refresh reconciles it. Dynamic load must not update
  etcd.
- Gate authenticates Login/Reconnect, binds the xnet user connection, owns the player's Logic and
  Public routes in its session state, and forwards Player/Item RPC to Logic and Mail RPC to Public.
- Login and Reconnect responses publish the Gate-side session ID required by the next reconnect.
- Player-bound messages enter the bounded outbox by default. xproto control IDs `1..99` and the
  explicitly enumerated Ping/Login/Reconnect/Logout/Kick session responses bypass it; adding an
  application Message ID range must not silently create more bypasses.
- Reconnect requires monotonic client sequence, matching ACK, retained outbox continuity, and the
  previous Gate session. A gap fails resume and requires a fresh login.
- External handshake/session admission and write queues must remain explicitly bounded.
- Shutdown stops client admission, awaits workers, drains resumable state, notifies Logic, and
  conditionally clears Redis session ownership. Cleanup notifications use the same FrameHandle
  as normal sends; Stopping does not disable the outbound drain path.

## Future Work

- Add multi-Gate resume transfer only with an explicit owner-transfer protocol; current resume is
  intentionally local to one Gate instance.
- Add sustained weak-network and outbox-overflow load tests before changing the silent oldest-drop
  policy.
