#!/usr/bin/env bash
# Re-runnable benchmark harness for custom-biome-lint.
#
# Replaces the one-off, hand-typed measurement that used to live in
# docs/ARCHITECTURE.md (a single run against a private, 4393-file dashboard
# tree that isn't available here). This script builds its own synthetic
# corpus instead -- self-contained, portable, and re-runnable by anyone with
# just this repo checked out.
#
# Usage:
#   scripts/benchmark.sh [file_count]
#
# file_count defaults to 2000. Larger values give more realistic per-file
# timings but take proportionally longer to generate and run.
set -euo pipefail

cd "$(dirname "$0")/.."

FILE_COUNT="${1:-2000}"
CORPUS_DIR="$(mktemp -d)"
trap 'rm -rf "$CORPUS_DIR"' EXIT

echo "== Building release binary =="
cargo build --release --quiet
BIN="$PWD/target/release/custom-biome-lint"

echo "== Generating a synthetic corpus of $FILE_COUNT files in $CORPUS_DIR =="
mkdir -p "$CORPUS_DIR/src"
# 90% clean (valid.js), 10% with violations (invalid.js) -- this cache only
# ever marks a file valid when it parsed cleanly AND had zero violations, so
# a corpus of nothing but "invalid" fixtures (an earlier version of this
# script's mistake) never exercises the cache at all: every file would
# always miss, and "warm" would silently measure the same thing as "cold".
# The 90/10 split instead approximates the realistic case this cache targets
# -- most files in a real tree are clean; a few always have violations.
python3 - "$CORPUS_DIR" "$FILE_COUNT" <<'PYEOF'
import pathlib, sys

corpus = pathlib.Path(sys.argv[1])
count = int(sys.argv[2])
clean = pathlib.Path("fixtures/no_native_map/valid.js").read_text()
dirty = pathlib.Path("fixtures/no_native_map/invalid.js").read_text()

for i in range(count):
    content = dirty if i % 10 == 0 else clean
    (corpus / "src" / f"file_{i}.js").write_text(content)
PYEOF

# Both real (wall-clock) and user (CPU) time are reported: wall time is what
# a person waiting on the command feels, but for a corpus of small synthetic
# files it's dominated by the file-read I/O this cache can no longer skip
# (see docs/INCREMENTAL_CACHING_DOCUMENT.md's "Why content hash, not mtime"
# section). User time isolates the CPU work the cache actually exists to
# avoid -- parsing and running rules -- and is where the cache's real,
# substantial saving shows up even when wall time doesn't move much.
# Prints "<real_seconds> <user_seconds>" for running "$@". /usr/bin/time -p
# is the one timing invocation that behaves the same on both BSD (macOS) and
# GNU (Linux) time -- unlike -l/-v, which use different flags and formats.
run_timed() {
  local timing_file real_s user_s
  timing_file="$(mktemp)"
  /usr/bin/time -p "$@" >/dev/null 2>"$timing_file" || true
  real_s=$(awk '/^real/ {print $2}' "$timing_file")
  user_s=$(awk '/^user/ {print $2}' "$timing_file")
  rm -f "$timing_file"
  echo "${real_s:-0} ${user_s:-0}"
}

files_per_sec() {
  local seconds="$1"
  if [ -z "$seconds" ] || [ "$(echo "$seconds <= 0" | bc)" = "1" ]; then
    echo "n/a"
  else
    echo "$FILE_COUNT / $seconds" | bc
  fi
}

echo
echo "== Warming up OS page cache and one-time binary-load cost =="
# A binary's very first exec, and a freshly written file's very first read,
# both pay a real disk-I/O cost that has nothing to do with this tool's own
# cache. Without this throwaway run, the *first* timed measurement below
# would silently absorb that one-time cost and look artificially slow next
# to every measurement after it.
(cd "$CORPUS_DIR" && "$BIN" --no-cache >/dev/null 2>&1) || true
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"

echo
echo "== Cold run (no cache) =="
read -r cold_real cold_user <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN" --no-cache)"
echo "cold: ${cold_real}s real, ${cold_user}s user ($(files_per_sec "$cold_real") files/sec by wall time)"

echo
echo "== Warm run (cache from the cold run above) =="
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
(cd "$CORPUS_DIR" && "$BIN" >/dev/null 2>&1) || true # prime the cache
read -r warm_real warm_user <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN")"
echo "warm: ${warm_real}s real, ${warm_user}s user ($(files_per_sec "$warm_real") files/sec by wall time)"
echo "user-time speedup (the CPU work this cache exists to skip): $(echo "scale=2; $cold_user / $warm_user" | bc 2>/dev/null || echo n/a)x"

echo
echo "== Rayon scaling: --parallel vs --no-parallel (both cold) =="
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
read -r parallel_real _ <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN" --no-cache --parallel)"
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
read -r sequential_real _ <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN" --no-cache --no-parallel)"
echo "parallel:   ${parallel_real}s real"
echo "sequential: ${sequential_real}s real"
echo "speedup: $(echo "scale=2; $sequential_real / $parallel_real" | bc 2>/dev/null || echo n/a)x"

echo
echo "== Parse vs. rule cost: 1 rule enabled vs. all 3 (both cold, user time) =="
# Mirrors the methodology docs/ARCHITECTURE.md used by hand: the fixed cost
# of parsing every file is what's left when you subtract out how much extra
# time each additional rule's tree walk adds.
cat >"$CORPUS_DIR/package.json" <<'EOF'
{ "ignoreBiomeExtensionRules": ["no-arrow-function-create-selector", "reselect-arity-match"] }
EOF
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
read -r _ one_rule_user <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN" --no-cache)"
rm -f "$CORPUS_DIR/package.json"
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
read -r _ three_rule_user <<<"$(cd "$CORPUS_DIR" && run_timed "$BIN" --no-cache)"
echo "1 rule (no-native-map only): ${one_rule_user}s user"
echo "3 rules (all):                ${three_rule_user}s user"
echo "estimated rule-walk cost for the other 2 rules combined: $(echo "$three_rule_user - $one_rule_user" | bc)s"

echo
echo "== Memory (peak RSS, best effort) =="
rm -rf "$CORPUS_DIR/.custom-biome-lint-cache"
if command -v /usr/bin/time >/dev/null 2>&1; then
  case "$(uname -s)" in
    Darwin) (cd "$CORPUS_DIR" && /usr/bin/time -l "$BIN" --no-cache >/dev/null) 2>&1 | grep "maximum resident set size" || true ;;
    Linux) (cd "$CORPUS_DIR" && /usr/bin/time -v "$BIN" --no-cache >/dev/null) 2>&1 | grep "Maximum resident set size" || true ;;
    *) echo "skipped: unsupported platform for peak-RSS measurement" ;;
  esac
else
  echo "skipped: /usr/bin/time not available"
fi

echo
echo "== Done. Corpus size: $FILE_COUNT files (90% clean, 10% with violations) =="
