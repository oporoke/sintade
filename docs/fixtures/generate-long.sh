#!/usr/bin/env bash
# The long-duration fixture for the Day 50 performance pass (docs/design.md §18): a 1080p30
# VP9/Opus WebM, never committed. Usage: generate-long.sh [minutes=30] [out=target/fixtures/...]
# `testsrc2` is busier than most screen content, so it's a pessimistic encode input.
set -euo pipefail
minutes="${1:-30}"
out="${2:-target/fixtures/vp9_opus_${minutes}min_1080p.webm}"
mkdir -p "$(dirname "$out")"
secs=$((minutes * 60))
ffmpeg -v error -y \
  -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=${secs}" \
  -f lavfi -i "sine=frequency=440:duration=${secs}" \
  -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -row-mt 1 -b:v 4M -g 150 \
  -c:a libopus "$out"
echo "$out"
