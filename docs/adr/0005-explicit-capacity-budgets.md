# Require explicit capacity budgets

Every XKK service configuration explicitly declares memory-bounding capacities such as RPC pending calls and write queues, while Gate also declares handshake/session admission and Logic declares mailbox, dirty-player, and resident-player limits. Library defaults remain available for tuning details, but they are not accepted as the process capacity contract.

For services that dial internal dependencies, the configured write queue applies to both accepted
internal links and xservice-managed outgoing links.
