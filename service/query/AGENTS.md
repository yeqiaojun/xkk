# Query Service

- Query binds HTTP only and creates no internal service connections.
- `/healthz` reports process liveness; `/readyz` succeeds only while xframe is Running.
- `/v1/query/gamers` performs one bounded Mongo `$in` query.
- Query still registers its HTTP endpoint and connects etcd, Mongo, and Redis before admission.

## Future Work

- Add indexes and pagination with measured query plans before expanding gamer query volume.
- Keep Query read-only; mutations belong to their authoritative service.
