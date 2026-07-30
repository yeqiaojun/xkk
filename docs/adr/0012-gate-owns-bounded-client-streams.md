# Gate owns bounded client streams

Gate owns one retained `ClientStream` for each logged-in player. The module hides client sequencing, acknowledgement, replay, reconnect binding, and local request limiting behind a small interface. These mechanics do not belong in xnet, xservice, Logic business modules, or Redis.

The client stream follows Product Gate's acknowledgement and reconnect behavior, using cumulative `u32` sequence and acknowledgement numbers instead of the Product `u16` fields:

- Every accepted C2S message advances the client sequence. Duplicate or stale C2S messages are not forwarded again.
- Every reliable S2C message receives a server sequence and is retained in a bounded outbox until cumulatively acknowledged.
- C2S traffic carries the latest S2C acknowledgement. When the client is otherwise idle, it sends a lightweight `ACK_NTF` so the server can release the outbox promptly.
- Reconnect presents the last acknowledged server sequence. Gate replays only later outbox entries and then continues the same stream.
- Sequence zero is reserved for transient control traffic. Reliable C2S and S2C sequences start at one and use wrapping serial-number comparison with a retained window smaller than `2^31`.

The outbox is an in-memory ring bounded by message count, total encoded bytes, and retention time. A payload is encoded once and shared between the live send and possible replay. Acknowledgement removes entries from the front in O(1) amortized time.

When an outbox bound is exceeded, Gate silently evicts the oldest entries until the stream is within budget. It records the highest evicted server sequence. Eviction does not interrupt a currently connected client because those frames have already entered its live send path.

On reconnect, resume succeeds only when the client's last acknowledged server sequence covers every evicted entry. If the acknowledgement is older than the recorded eviction point, Gate cannot produce a contiguous replay, rejects reconnect with `RESUME_EXPIRED`, releases the retained stream, and requires a normal login.

Only messages sent to an authenticated, bound player are eligible for the outbox. Every such message enters the outbox by default.

`xkk-protocol` centrally declares the exceptions as exact message IDs or contiguous message-ID ranges. Heartbeat, acknowledgement, login/reconnect handshake, kick, and connection-control traffic bypass the outbox through those exclusions. Gate handlers and business modules must not maintain their own exception lists.

All Gate-to-player sends, including Gate-generated error responses, pass through the same `ClientStream` egress point before xnet delivery. Messages sent to an unauthenticated connection or to another server never enter a player outbox.

XKK does not add a separate Gate-wide outbox byte budget. Memory is controlled by each stream's count, byte, and age limits together with the configured connection and retained-stream capacities.

The local C2S limiter preserves Product Gate's two sliding limits:

- at most 15 accepted client requests in any 5-second window;
- at most 8 accepted client requests in any 1-second window.

The Rust implementation uses fixed-capacity state with no heap allocation. Session and sequence validation, acknowledgement pruning, and rate admission occur in one serialized player-stream operation. A rate-limited request still consumes its valid client sequence and receives the corresponding response with `TCP_C2S_TOO_FAST`; it is not forwarded to Logic or Public. ACK and connection-control traffic bypass the request limiter.

Fast resume is guaranteed only while the original Gate process and retained `ClientStream` are alive. The client reconnects to its original Gate endpoint during that window. If the Gate process is unavailable or the stream has expired, the client performs a normal login and necessary state recovery. XKK does not persist the outbox in Redis and does not transfer it between Gate instances.

Gate retains a disconnected stream for 60 seconds, matching Product Gate's reconnect window. Reconnect replaces the session only when the recorded session is either the disconnecting session or already offline. A token may be used for at most 10 reconnects, with at most 5 accepted reconnects in a rolling 60-second window. Successful reconnect replays retained messages whose server sequence is newer than the client's acknowledgement.

These are Product Gate behavior contracts, not an instruction to translate its implementation. XKK uses an in-memory ring instead of a linked list, normal Rust synchronization instead of `go-deadlock`, fixed-capacity limiter state instead of temporary slices, and corrected inclusive acknowledgement and window-limit comparisons.
