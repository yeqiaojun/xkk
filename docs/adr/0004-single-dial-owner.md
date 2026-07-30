# Assign one dial owner per service edge

Gate dials Logic and Public, while Logic dials Public; Query has no internal service connections. The accepting side watches caller identities only for Hello admission, and the established TCP connection carries RPC in both directions, avoiding competing duplicate links from mutual connection intents.
