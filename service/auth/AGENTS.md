# Auth Service

- Auth owns account login, role selection, admission limiting, and xtoken issuance.
- Auth is HTTP-only. It watches Gate discovery state but creates no service connection.
- Login and role admission use the shared Redis and Mongo dependencies from xframe.
- `/v1/auth/login` issues the repository's xtoken. `/v1/auth/use-role` validates both xtoken
  contents and the Redis `gamer:{gid}` token before returning every configured Gate transport.
- Auth periodically reads discovered Gate online counts from Redis, overlays its local xservice
  snapshot, and chooses the healthy Gate with the lowest refreshed count. Each admission also
  increments the selected Gate in the local snapshot so a burst does not reuse one stale minimum;
  the next Redis refresh reconciles the estimate. Auth never opens a Gate link or writes dynamic
  load to etcd.
- Keep queue and rate budgets explicit in YAML and reject overload before expensive account work.
- Keep `lib.rs` as the public package surface, `service.rs` as process composition/lifecycle, and
  `api.rs` as the cohesive login/use-role workflow. Do not split individual workflow steps into
  tiny modules.

## Future Work

- Move queue admission to a durable multi-instance policy only when one Auth instance is no longer
  sufficient; preserve the current Redis field names and double-token validation.
- Add account-provider adapters behind the existing login workflow instead of widening xframe.
