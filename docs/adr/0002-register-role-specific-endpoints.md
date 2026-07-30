# Register role-specific endpoints

Every XKK service package registers with discovery even when it has no internal service listener. Gate publishes its external client endpoint, Query publishes HTTP, and Logic/Public publish internal service endpoints; registration means the instance is discoverable, while only internal service endpoints are valid targets for xservice Hello connections.
