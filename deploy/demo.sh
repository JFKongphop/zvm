#!/bin/sh
# Starts a fresh stand-in-chain node on every boot: the demo ledger is throwaway by design.
set -e
DATA=/tmp/zvm-demo
rm -rf "$DATA"
zvm init --dir "$DATA" --network 1 --admin 0x00000000000000000000000000000000000000ad >/dev/null
exec zvm serve --dir "$DATA" --fake --bind 0.0.0.0 --port 8080 --web /app/web
