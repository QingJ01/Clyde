#!/bin/bash
# Clyde 动画全播放测试脚本
# 用法: bash test-demo.sh [每个动画秒数，默认8]

DELAY=${1:-8}

SVGS=(
  "clyde-idle-living.svg"
  "clyde-sleeping.svg"
  "clyde-working-thinking.svg"
  "clyde-working-typing.svg"
  "clyde-working-juggling.svg"
  "clyde-working-sweeping.svg"
  "clyde-working-building.svg"
  "clyde-working-debugger.svg"
  "clyde-working-wizard.svg"
  "clyde-working-carrying.svg"
  "clyde-working-conducting.svg"
  "clyde-working-confused.svg"
  "clyde-working-overheated.svg"
  "clyde-error.svg"
  "clyde-working-ultrathink.svg"
  "clyde-happy.svg"
  "clyde-notification.svg"
  "clyde-disconnected.svg"
)

echo "=== Clyde Demo: ${#SVGS[@]} animations, ${DELAY}s each ==="
for i in "${!SVGS[@]}"; do
  svg="${SVGS[$i]}"
  echo "[$((i+1))/${#SVGS[@]}] $svg"
  curl -s -X POST http://127.0.0.1:23333/state \
    -H "Content-Type: application/json" \
    -d "{\"state\":\"working\",\"svg\":\"$svg\"}"
  sleep "$DELAY"
done
echo "=== DONE ==="
