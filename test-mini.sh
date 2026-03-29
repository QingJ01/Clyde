#!/bin/bash
# 极简模式 SVG 动画测试脚本
# 用法: bash test-mini.sh [每个动画秒数，默认6]
# 注意: 先 npm start 启动应用，再运行此脚本

DELAY=${1:-6}

SVGS=(
  "clyde-mini-idle.svg"
  "clyde-mini-enter.svg"
  "clyde-mini-peek.svg"
  "clyde-mini-alert.svg"
  "clyde-mini-happy.svg"
  "clyde-mini-crabwalk.svg"
)

STATES=(
  "mini-idle"
  "mini-enter"
  "mini-peek"
  "mini-alert"
  "mini-happy"
  "mini-crabwalk"
)

echo "=== Mini Mode Demo: ${#SVGS[@]} animations, ${DELAY}s each ==="
for i in "${!SVGS[@]}"; do
  svg="${SVGS[$i]}"
  state="${STATES[$i]}"
  echo "[$((i+1))/${#SVGS[@]}] $state → $svg"
  curl -s -X POST http://127.0.0.1:23333/state \
    -H "Content-Type: application/json" \
    -d "{\"state\":\"$state\",\"svg\":\"$svg\"}"
  sleep "$DELAY"
done

echo ""
echo "Returning to idle..."
curl -s -X POST http://127.0.0.1:23333/state \
  -H "Content-Type: application/json" \
  -d '{"state":"idle","svg":"clyde-idle-follow.svg"}'
echo ""
echo "=== DONE ==="
