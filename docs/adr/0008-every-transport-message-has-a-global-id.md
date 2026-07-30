# Every transport message has a global ID

Every XKK transport message, including HTTP requests and responses, has a globally unique `MsgId` declared by `xkk-protocol`. HTTP paths route requests but do not replace protocol identity.

The message catalog has exactly three message kinds: `REQ`, `RSP`, and `NTF`. A request and its response use two distinct IDs; a notification has its own ID. Startup registration fails when a handler message lacks a declared ID, uses another kind, or reuses an existing ID.
