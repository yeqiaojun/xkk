# Prune migration by domain ownership

XKK migrates Product behavior only when the target service owns the behavior and all required domains are present in XKK. A feature is removed rather than partially migrated when it depends on an excluded gameplay domain.

The retained service scope is:

- Auth owns login, role selection, admission limiting, and session issuance.
- Gate owns login, reconnect, logout, session ownership, message routing, and duplicate-login eviction.
- Query owns Player-compatible queries and configuration delivery. Rank and Battle queries are excluded.
- Public owns Mail, including Item-backed attachment collection. WorldBoss is excluded.
- Logic owns only Player and Item, as defined by ADR 0007.

Product-specific operational backends, BI integration, ACE integration, debug proxies, and the legacy configuration hot-update machinery are excluded.
