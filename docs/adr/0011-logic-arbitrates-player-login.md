# Logic arbitrates player login

Gate validates the client session but does not arbitrate an existing player connection and does not send a cross-Gate kick based on Redis `gsid/sess`.

For login:

1. Gate reads `gamer:{gid}.lsid`.
2. When `lsid` is non-zero, Gate sends the login RPC to that Logic instance.
3. When `lsid` is zero, Gate selects the healthy connected Logic instance with the lowest player count and sends the login RPC there.
4. Logic serializes the login in the player's `gid` execution domain. If another session currently owns the player, Logic tells the corresponding Gate to kick that exact old session, replaces the active Gate/session identity, and completes the login RPC.

When Redis contains a non-zero `lsid`:

- If that Logic instance is still present in discovery but its service link is not Ready, Gate rejects the login as temporarily unavailable. It must not select another Logic during a transient disconnect.
- If that Logic instance has been removed from discovery, Gate treats `lsid` as a stale route and selects the healthy connected Logic instance with the lowest player count.

Gate updates Redis routing state only after the Logic login RPC succeeds. It then binds the local user route and writes `gsid`, `sess`, `lsid`, and `lgin`. A failed Logic login leaves the previous routing state unchanged.

On client disconnect, Gate records `lgou` and clears only `gsid/sess`, using the disconnecting `sess` as a compare condition. Gate does not clear `lsid`. Logic clears `lsid` only after it finishes offline retention, completes the required save, and releases the player's execution domain.

Logic is the authority for a player's active execution session. Redis `gsid/sess/lsid` records cluster routing state, while Gate-local session tables remain delivery indexes. Gate must not implement an independent cross-Gate ownership protocol.
