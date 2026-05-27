#!/usr/bin/env bash
# bevy-rogue WASM PoC 헤드리스 검증 래퍼.
# - dist-wasm/ 을 정적 서버로 띄우고 playwright(chromium) 으로 검증.
# - 검증 스크립트가 dist-wasm/poc-screenshot.png + poc-console.log 생성.
#
# 의존:
#   - python3 (정적 서버)
#   - node + playwright (npm i playwright + npx playwright install chromium)
#     이미 /tmp/wasm-verify 에 설치돼 있으면 그 node_modules 사용.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${PORT:-8765}"
URL="http://127.0.0.1:${PORT}/"
DIST="${ROOT}/dist-wasm"

if [ ! -f "${DIST}/index.html" ] || [ ! -f "${DIST}/bevy_rogue.js" ]; then
  echo "[verify] dist-wasm/ 가 비었거나 번들이 없다. 먼저 scripts/wasm-build.sh 를 실행하라." >&2
  exit 2
fi

# playwright 위치 결정 (저장소 node_modules → /tmp/wasm-verify/node_modules 순)
NM=""
if [ -d "${ROOT}/node_modules/playwright" ]; then
  NM="${ROOT}/node_modules"
elif [ -d "/tmp/wasm-verify/node_modules/playwright" ]; then
  NM="/tmp/wasm-verify/node_modules"
else
  # npx 캐시 자동 탐색 (네트워크가 막혀 npm install 이 안 될 때).
  CAND=$(find "$HOME/.npm/_npx" -type d -name playwright 2>/dev/null | head -1)
  if [ -n "${CAND}" ]; then
    NM="$(dirname "${CAND}")"
  else
    echo "[verify] playwright 가 설치돼 있지 않다. 'cd /tmp/wasm-verify && npm i playwright' 또는 'npx -y playwright@1.60 --help' 후 재시도." >&2
    exit 2
  fi
fi
export NODE_PATH="${NM}"
export PLAYWRIGHT_NODE_PATH="${NM}"

# headless chromium 이 시스템 라이브러리(libnspr4/libnss3/libXi 등)를 못 찾는 경우
# /tmp/chrome-deps/libs 에 추출해 둔 libs 를 LD_LIBRARY_PATH 로 끼워준다(있을 때만).
if [ -d /tmp/chrome-deps/libs/usr/lib/x86_64-linux-gnu ]; then
  export LD_LIBRARY_PATH="/tmp/chrome-deps/libs/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
fi

# python3 정적 서버 (background)
python3 -m http.server "${PORT}" --directory "${DIST}" >/tmp/wasm-http.log 2>&1 &
HTTP_PID=$!
trap 'kill ${HTTP_PID} 2>/dev/null || true' EXIT

# 서버 ready 대기
for i in 1 2 3 4 5 6 7 8 9 10; do
  if curl -sf "${URL}" -o /dev/null; then break; fi
  sleep 0.3
done

echo "[verify] static server up: ${URL}  (pid ${HTTP_PID})"
node "${ROOT}/scripts/wasm-verify.mjs" "${URL}"
