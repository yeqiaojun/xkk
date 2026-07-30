# Publish the Gate transport set

Gate may listen on TCP, KCP, and WebSocket simultaneously, with one configured port per enabled transport. Discovery keeps the selected primary transport in the stable `host/port` fields and publishes every enabled transport in metadata; when several transports are enabled, configuration must explicitly select the primary transport.
