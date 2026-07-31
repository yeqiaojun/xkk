#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
protoc="$root/tools/protoc.exe"
xmongo_plugin="$root/tools/protoc-gen-xmongo-trait.exe"

[[ -f "$protoc" ]] || { echo "missing protocol compiler: $protoc" >&2; exit 1; }
[[ -f "$xmongo_plugin" ]] || { echo "missing xmongo protocol plugin: $xmongo_plugin" >&2; exit 1; }

export PROTOC="$protoc"
export XKK_PROTO_UPDATE="$$-$(date +%s)"

cd "$root"
cargo check --quiet -p xkk-protocol

echo "generated protocol sources: $root/protocol/generated"
