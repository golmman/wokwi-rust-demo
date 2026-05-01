#!/bin/bash
# DCF77 loopback sim: build firmware with --features dcf77-loopback (so
# the TX state machine drives GP13) and run scenario.dcf77.yaml against
# diagram.dcf77.json (which wires GP13 -> GP14).
#
# This is the slow-path counterpart to ./run-sim.sh, used only after
# changes to the loopback path itself: bsp DCF77 wiring, TxState, the
# RTIC dcf77_sample task body, or the diagram. For pure-logic edits
# (decoder, encoder, clock arithmetic) ./check.sh is enough.
set -euo pipefail

cd "$(dirname "$0")"

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

cargo build --release --features dcf77-loopback
elf2uf2-rs target/thumbv6m-none-eabi/release/wokwi-test \
           target/thumbv6m-none-eabi/release/wokwi-test.uf2

mkdir -p target/wokwi

wokwi-cli \
  --diagram-file diagram.dcf77.json \
  --scenario scenario.dcf77.yaml \
  --timeout "${WOKWI_TIMEOUT_MS:-15000}" \
  --serial-log-file target/wokwi/dcf77-serial.log \
  .

echo
echo "screenshots: target/wokwi/dcf77-before.png, target/wokwi/dcf77-after.png"
echo "decoded times:"
DECODED=$(python3 tools/decode_screenshot.py \
  target/wokwi/dcf77-before.png target/wokwi/dcf77-after.png)
echo "$DECODED"

# Assert: both screenshots decoded to valid HH:MM:SS, and the displayed
# minute changed at least once over the run. With the loopback wired,
# the receiver applies decoded frames every ~minute, so over the
# scenario's wall budget the minute *must* advance — if it doesn't,
# something is wrong with TX/RX/clock integration. (We can't be more
# precise than "delta >= 1" because Wokwi's sim-time-vs-wall-time
# ratio varies during the run; see AGENTS.md "Timing caveat".)
BEFORE_MIN=$(echo "$DECODED" | sed -n 's/.*before.png: [0-9][0-9]:\([0-9][0-9]\):[0-9][0-9]/\1/p')
AFTER_MIN=$(echo "$DECODED" | sed -n 's/.*after.png: [0-9][0-9]:\([0-9][0-9]\):[0-9][0-9]/\1/p')
if [ -z "$BEFORE_MIN" ] || [ -z "$AFTER_MIN" ]; then
    echo "error: failed to parse HH:MM:SS from one or both screenshots." >&2
    exit 1
fi
if [ "$BEFORE_MIN" = "$AFTER_MIN" ]; then
    echo "error: displayed minute did not advance between snapshots ($BEFORE_MIN -> $AFTER_MIN)." >&2
    echo "       Either the RX never decoded a frame, the TX never wrapped a minute," >&2
    echo "       or the firmware clock isn't ticking. Check the loopback path." >&2
    exit 1
fi
printf '\nok: minute advanced %s -> %s\n' "$BEFORE_MIN" "$AFTER_MIN"
