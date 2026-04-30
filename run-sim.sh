#!/bin/bash
# Build the firmware and run the Wokwi cloud simulation against scenario.yaml.
# Outputs (screenshots, serial log) land in target/wokwi/, which is gitignored
# via target/. Requires WOKWI_CLI_TOKEN (https://wokwi.com/dashboard/ci).
set -euo pipefail

cd "$(dirname "$0")"

# Pick up WOKWI_CLI_TOKEN (and any other vars) from a local .env, if present.
# .env is gitignored — keep secrets out of the working tree.
if [ -f .env ]; then
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
fi

if [ -z "${WOKWI_CLI_TOKEN:-}" ]; then
  echo "error: WOKWI_CLI_TOKEN is not set." >&2
  echo "       Generate one at https://wokwi.com/dashboard/ci, then:" >&2
  echo "       export WOKWI_CLI_TOKEN=<your-token>" >&2
  exit 1
fi

./build.sh

mkdir -p target/wokwi

wokwi-cli \
  --scenario scenario.yaml \
  --timeout "${WOKWI_TIMEOUT_MS:-6000}" \
  --serial-log-file target/wokwi/serial.log \
  .

echo
echo "screenshots: target/wokwi/before.png, target/wokwi/after.png"
echo "serial log:  target/wokwi/serial.log (empty unless firmware writes UART)"
