#!/usr/bin/env bash
# Loop the two-test libtest binary (each iteration re-invokes cargo) while two
# fresh-target cargo builds run concurrently as load. Counts MYSTERY fires
# (backwards version / torn-or-foreign snapshot) separately from MUNDANE ones
# (reader starvation) and from anything else.
#
# usage: scripts/hammer.sh [-n ITERS] [-l LOAD_DIR|none] [-f FEATURES] [-o OUT] [-s]
#   -n  iterations (default 300)
#   -l  cargo workspace to build as load (default ./load; "none" = no load)
#   -f  cargo features for the test build (e.g. tracking_alloc)
#   -o  output dir (default ./hammer-out)
#   -s  stop at the first MYSTERY fire
set -u

ITERS=300
LOAD_DIR="$(cd "$(dirname "$0")/.." && pwd)/load"
FEATURES=""
OUT="$(pwd)/hammer-out"
STOP_FIRST=0
while getopts "n:l:f:o:s" opt; do
  case "$opt" in
    n) ITERS="$OPTARG" ;;
    l) LOAD_DIR="$OPTARG" ;;
    f) FEATURES="$OPTARG" ;;
    o) OUT="$OPTARG" ;;
    s) STOP_FIRST=1 ;;
    *) echo "bad option" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"
LOG="$OUT/hammer.log"
FEAT_ARGS=()
[ -n "$FEATURES" ] && FEAT_ARGS=(--features "$FEATURES")
loadavg() { uptime 2>/dev/null | sed 's/.*load averages*: *//' || echo n/a; }

load_loop() { # $1 = name, rest = cargo subcommand
  local name="$1"; shift
  while [ ! -e "$OUT/stop" ]; do
    rm -rf "$OUT/target-$name"
    cargo "$@" --manifest-path "$LOAD_DIR/Cargo.toml" --workspace \
      --target-dir "$OUT/target-$name" >"$OUT/load-$name.log" 2>&1 &
    echo $! >"$OUT/load-$name.pid"
    wait $! 2>/dev/null
  done
}

cleanup() {
  touch "$OUT/stop"
  for f in "$OUT"/load-*.pid; do [ -e "$f" ] && kill "$(cat "$f")" 2>/dev/null; done
  [ -n "${LA:-}" ] && kill "$LA" 2>/dev/null
  [ -n "${LB:-}" ] && kill "$LB" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$OUT"/target-load-* "$OUT"/target-a "$OUT"/target-b "$OUT/stop" "$OUT"/load-*.pid
}
trap cleanup EXIT INT TERM

{
  echo "== arc-swap #210 hammer $(date -u +%FT%TZ)"
  echo "host: $(uname -srm)"
  echo "rustc: $(rustc -V)"
  echo "arc-swap: $(grep -A1 'name = "arc-swap"' "$ROOT/Cargo.lock" 2>/dev/null | sed -n 2p)"
  echo "features: ${FEATURES:-<none>}  iters: $ITERS  load: $LOAD_DIR"
} | tee "$LOG"

echo "warm-up build..." | tee -a "$LOG"
if ! (cd "$ROOT" && cargo test --lib "${FEAT_ARGS[@]+"${FEAT_ARGS[@]}"}" --no-run -q >"$OUT/warmup.log" 2>&1); then
  echo "warm-up build FAILED (see $OUT/warmup.log)" | tee -a "$LOG"; exit 2
fi

rm -f "$OUT/stop"
if [ "$LOAD_DIR" != "none" ]; then
  load_loop a check & LA=$!
  load_loop b test --no-run & LB=$!
  echo "load started (pids $LA $LB)" | tee -a "$LOG"
fi

mystery=0; mundane=0; other=0; done_iters=0
for i in $(seq 1 "$ITERS"); do
  out="$OUT/iter.log"
  (cd "$ROOT" && cargo test --lib "${FEAT_ARGS[@]+"${FEAT_ARGS[@]}"}" -q concurrent_lora -- --test-threads=2 >"$out" 2>&1)
  rc=$?
  done_iters=$i
  if [ $rc -ne 0 ]; then
    la="$(loadavg)"
    if grep -q MYSTERY "$out"; then
      mystery=$((mystery + 1)); kind=MYSTERY
    elif grep -q MUNDANE "$out"; then
      mundane=$((mundane + 1)); kind=MUNDANE
    else
      other=$((other + 1)); kind=OTHER
    fi
    echo "iter $i: $kind (load $la)" | tee -a "$LOG"
    grep -E "panicked|MYSTERY|MUNDANE" "$out" | head -4 | sed 's/^/    /' | tee -a "$LOG"
    cp "$out" "$OUT/fire-$i.log"
    [ "$kind" = MYSTERY ] && [ "$STOP_FIRST" -eq 1 ] && break
  fi
  [ $((i % 25)) -eq 0 ] && echo "iter $i: mystery=$mystery mundane=$mundane other=$other (load $(loadavg))" | tee -a "$LOG"
done

echo "== DONE iters=$done_iters mystery=$mystery mundane=$mundane other=$other" | tee -a "$LOG"
