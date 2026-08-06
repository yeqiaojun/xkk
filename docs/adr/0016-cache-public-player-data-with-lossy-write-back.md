---
status: accepted
---

# Cache Public Player Data with lossy write-back

Each gid has one Public Owner, which loads one `PublicPlayerData` aggregate from the fixed `public_players` collection into a large sliding-TTL xlru and serializes access through the aggregate's in-memory read/write lock. Public records changed gids in a concurrent dirty set and, every two minutes and once during graceful shutdown, passes that set to xlru for unordered batched Mongo upserts.

If Mongo has no document for a gid, the loader returns a clean empty aggregate. A read does not create a document; the first business mutation marks it dirty and the normal deferred upsert creates it.

This design deliberately prioritizes throughput over durability: a saver copies and clears dirty state before Mongo I/O, logs a failed save but reports success to xlru, and neither retries nor rolls back; crashes may lose the current two-minute window. The cache does not pin active values or maintain generations, so capacity and TTL must be sized to avoid active-player eviction, and Public topology changes must drain old owners before reassignment.
