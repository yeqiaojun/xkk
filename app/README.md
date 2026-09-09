# xkk-app

The shared process entry helper provides only repeated setup and ownership:

1. Open the configured log worker and initialize service names and the protocol registry.
2. Validate and connect Redis/Mongo; close Redis if Mongo connection fails.
3. Run the role-specific service future with explicit cloned clients.
4. Close Mongo, Redis and the log worker after the service future returns, including errors.

Each service retains configuration conversion, xframe preparation, listeners, discovery watches,
business handlers and its Application implementation. Its future must finish frame shutdown
before returning. `Resources` contains only the two persistence clients; there is no singleton,
configuration trait, dependency container or extra lifecycle callback layer.
