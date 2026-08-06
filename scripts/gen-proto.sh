#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export XKK_PROTO_UPDATE="$$-$(date +%s)"

cd "$root"
cargo check --quiet --locked -p xkk-protocol

echo "generated protocol sources: $root/protocol/generated"
