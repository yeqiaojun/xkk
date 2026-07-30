# Login Robot

- Keep the flow linear: Auth login, Auth use-role, Gate connect, `LoginReq/LoginRsp`, shutdown.
- The Auth use-role queue is the only retry loop and remains bounded by the configured overall timeout.
- Do not add pressure, smoke, reconnect, logout, heartbeat, or gameplay behavior to this crate.
- Use `xnet` and `CsPacket`; do not create a second transport or packet implementation.
- A valid `LoginRsp` is success. Always shut down the xnet client before returning.
