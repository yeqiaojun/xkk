$ErrorActionPreference = "Stop"
$env:XKK_PROTO_UPDATE = "1"
cargo check --quiet --locked -p xkk-protocol
