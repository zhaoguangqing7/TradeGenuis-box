#!/usr/bin/env bash
# TradeGenuis · 箱体突破看板 一键启动 (Rust 版)
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "❌ 未找到 cargo，请先安装 Rust 工具链: https://rustup.rs"; exit 1
fi

PORT="${PORT:-8808}"
HOST="${HOST:-127.0.0.1}"

echo "🚀 TradeGenuis 箱体突破看板启动中 (Rust + PostgreSQL)..."
echo "   地址: http://${HOST}:${PORT}"
echo "   停止: Ctrl+C"
exec cargo run --release -- --host "$HOST" --port "$PORT"
