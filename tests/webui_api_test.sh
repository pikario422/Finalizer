#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
TEST_ROOT=$(mktemp -d)
MODULE_DIR="$TEST_ROOT/module"

cleanup() {
    case "$TEST_ROOT" in
        /tmp/*|/var/tmp/*) rm -rf -- "$TEST_ROOT" ;;
    esac
}
trap cleanup EXIT

mkdir -p "$MODULE_DIR" "$MODULE_DIR/log"
cp -R "$ROOT_DIR/mode/." "$MODULE_DIR/"
API="$MODULE_DIR/webroot/api.sh"
printf '%s\n' '[2026-07-26 18:00:00] [INFO] test log' > "$MODULE_DIR/log/log.log"

status=$(bash "$API" status)
grep -q '^mode=powersave$' <<< "$status"
grep -q '^log_level=info$' <<< "$status"
grep -q '^log_bytes=' <<< "$status"

bash "$API" set-mode fast >/dev/null
grep -q '^fast$' "$MODULE_DIR/config/config.txt"

bash "$API" set-log-level debug >/dev/null
status=$(bash "$API" status)
grep -q '^log_level=debug$' <<< "$status"

bash "$API" tail-log | grep -q '\[INFO\] test log'
bash "$API" clear-log >/dev/null
test ! -s "$MODULE_DIR/log/log.log"

cp "$MODULE_DIR/config/config.toml" "$TEST_ROOT/expected.toml"
payload=$(base64 < "$TEST_ROOT/expected.toml" | tr -d '\n')
bash "$API" write-config "$payload" >/dev/null
cmp "$TEST_ROOT/expected.toml" "$MODULE_DIR/config/config.toml"

invalid_payload=$(printf 'invalid config' | base64 | tr -d '\n')
if bash "$API" write-config "$invalid_payload" >/dev/null 2>&1; then
    echo "invalid config was accepted" >&2
    exit 1
fi

echo "WebUI API tests passed"
