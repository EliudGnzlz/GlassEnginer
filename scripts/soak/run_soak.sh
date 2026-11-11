#!/usr/bin/env bash
# Sin set -e: el informe debe generarse también ante fallos.
set -u

HOURS="${1:-1}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$ROOT_DIR" || exit 1

REPORT_DIR="$ROOT_DIR/soak-report"
CSV="$REPORT_DIR/rss.csv"
SUMMARY="$REPORT_DIR/summary.txt"
mkdir -p "$REPORT_DIR"
echo "epoch,rss_kb" > "$CSV"

if [ -z "${VK_ICD_FILENAMES:-}" ]; then
  ICD="$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -n 1)"
  if [ -n "$ICD" ]; then
    export VK_ICD_FILENAMES="$ICD"
  fi
fi

XVFB_PID=""
if [ -z "${DISPLAY:-}" ]; then
  Xvfb :99 -screen 0 1280x720x24 > "$REPORT_DIR/xvfb.log" 2>&1 &
  XVFB_PID=$!
  export DISPLAY=:99
  sleep 1
fi

if [ -d "$HOME/workspace/tools/dotnet" ]; then
  export DOTNET_ROOT="$HOME/workspace/tools/dotnet"
fi

cleanup() {
  if [ -n "${GAME_PID:-}" ] && kill -0 "$GAME_PID" 2>/dev/null; then
    kill "$GAME_PID" 2>/dev/null
    wait "$GAME_PID" 2>/dev/null
  fi
  if [ -n "$XVFB_PID" ]; then
    kill "$XVFB_PID" 2>/dev/null
  fi
}
trap cleanup EXIT

"$ROOT_DIR/target/debug/glass_app" > "$REPORT_DIR/game.log" 2>&1 &
GAME_PID=$!

START_EPOCH="$(date +%s)"
DURATION_SECONDS="$(awk -v h="$HOURS" 'BEGIN { printf "%.0f", h * 3600 }')"
END_EPOCH=$((START_EPOCH + DURATION_SECONDS))
SURVIVED=1

while [ "$(date +%s)" -lt "$END_EPOCH" ]; do
  sleep 60
  if ! kill -0 "$GAME_PID" 2>/dev/null; then
    SURVIVED=0
    break
  fi
  RSS="$(awk '/^VmRSS:/ { print $2 }' "/proc/$GAME_PID/status" 2>/dev/null)"
  echo "$(date +%s),${RSS:-0}" >> "$CSV"
done

# Mediana de una secuencia de valores RSS, en KiB.
median() {
  sort -n | awk '
    { values[NR] = $1 }
    END {
      if (NR == 0) { print 0; exit }
      if (NR % 2) print values[(NR + 1) / 2]
      else printf "%.0f\n", (values[NR / 2] + values[NR / 2 + 1]) / 2
    }'
}

SAMPLES=$(($(wc -l < "$CSV") - 1))
INITIAL_RSS="$(tail -n +2 "$CSV" | head -n 5 | cut -d, -f2 | median)"
FINAL_RSS="$(tail -n +2 "$CSV" | tail -n 5 | cut -d, -f2 | median)"
GROWTH="$(awk -v initial="$INITIAL_RSS" -v final="$FINAL_RSS" \
  'BEGIN { if (initial > 0) printf "%.2f", (final - initial) * 100 / initial; else print "0.00" }')"

VERDICT="FAIL"
if [ "$SURVIVED" -eq 1 ] && \
   awk -v growth="$GROWTH" 'BEGIN { exit !(growth < 10) }'; then
  VERDICT="PASS"
fi

ACTUAL_SECONDS=$(( $(date +%s) - START_EPOCH ))
{
  echo "Duración solicitada: $HOURS h"
  echo "Duración real: $ACTUAL_SECONDS s"
  echo "Muestras: $SAMPLES"
  echo "RSS inicial (mediana, KiB): $INITIAL_RSS"
  echo "RSS final (mediana, KiB): $FINAL_RSS"
  echo "Crecimiento: $GROWTH %"
  echo "Proceso superviviente: $SURVIVED"
  echo "Veredicto: $VERDICT"
} > "$SUMMARY"

[ "$VERDICT" = "PASS" ] && exit 0
exit 1
