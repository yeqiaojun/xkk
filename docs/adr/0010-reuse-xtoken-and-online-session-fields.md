# Reuse xtoken and online session fields

XKK uses the existing `xtoken` implementation for user-session issuance and verification. Auth issues a user token from the selected player ID and device ID; XKK does not introduce JWT, a separate opaque-token format, or a session-generation field.

Gate login and reconnect perform both checks before binding the connection to a player:

1. Decode the presented token with `xtoken`, validate its expiration and device binding, and require its player ID to match the request.
2. Require the presented token to equal `gamer:{gid}.token` in Redis.

Cryptographic validation proves the credential itself; Redis equality provides immediate revocation and replacement.

The Redis online-session record keeps the Product `gamer:{gid}` hash as its baseline contract:

- `acc`: account
- `gid`: player ID
- `token`: active xtoken
- `sess`: active Gate connection session
- `lgin`: login time
- `lgou`: logout time
- `psid`: Public instance ID
- `gsid`: Gate instance ID
- `lsid`: Logic instance ID

The Product `modf` module-flag field is not migrated. It has no retained Player, Item, Gate-routing, or Mail behavior in XKK.

The record keeps its existing 30-day TTL behavior. Implementation may make small additions when required by an XKK use case, but it must not wholesale rename the fields or reshape their established meanings.
