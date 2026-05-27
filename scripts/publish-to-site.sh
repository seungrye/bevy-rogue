#!/usr/bin/env bash
# bevy-rogue WASM 산출물을 site repo 의 public 디렉터리로 publish.
#
# 동작:
#   1. scripts/wasm-build.sh 실행 → dist-wasm/ 갱신.
#   2. rsync -a --delete 로 dist-wasm/{bevy_rogue.js,bevy_rogue_bg.wasm[.gz|.br],assets/}
#      를 ${SITE_PATH}/webapp/public/games/bevy-rogue/ 로 복사.
#      (index.html, poc-* 검증 산출물은 제외 — 사이트는 page.tsx 가 직접 렌더.)
#   3. 복사된 파일 목록 + 합계 사이즈 요약.
#
# 환경변수:
#   SITE_PATH       기본 /home/seungrye/site
#   SKIP_BUILD=1    wasm-build.sh 건너뛰기(이미 dist-wasm 가 최신일 때).
#
# 사용:
#   scripts/publish-to-site.sh
#   SITE_PATH=/path/to/site scripts/publish-to-site.sh
#   SKIP_BUILD=1 scripts/publish-to-site.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SITE_PATH="${SITE_PATH:-/home/seungrye/site}"
DEST="${SITE_PATH}/webapp/public/games/bevy-rogue"
SRC="${ROOT}/dist-wasm"

log()  { printf '\033[1;34m[publish]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[publish]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[publish]\033[0m %s\n' "$*" >&2; exit 1; }

# 1) wasm 빌드
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  log "wasm 빌드 실행 (SKIP_BUILD=1 로 건너뛰기 가능)"
  bash "${ROOT}/scripts/wasm-build.sh"
else
  log "SKIP_BUILD=1 — wasm-build.sh 건너뜀."
fi

[[ -d "${SRC}" ]] || die "${SRC} 없음 — 먼저 wasm-build.sh 를 실행하라."
[[ -f "${SRC}/bevy_rogue.js" ]] || die "${SRC}/bevy_rogue.js 없음 — 빌드 실패?"
[[ -f "${SRC}/bevy_rogue_bg.wasm" ]] || die "${SRC}/bevy_rogue_bg.wasm 없음 — 빌드 실패?"

# 2) 대상 디렉터리 보장
log "대상 디렉터리: ${DEST}"
mkdir -p "${DEST}"

# 3) 동기화 — rsync 가 있으면 그쪽(빠르고 --delete 깔끔), 없으면 cp 폴백.
#    어느 쪽이든: 대상 디렉터리를 비우고 글루/wasm/assets/ 만 복사.
#    (index.html / poc-* / *.log / *-screenshot.png 같은 검증용 파일은 제외.)
if command -v rsync >/dev/null 2>&1; then
    log "rsync -a --delete 동기화"
    rsync -a --delete \
      --exclude='index.html' \
      --exclude='poc-*' \
      --exclude='stage*-*' \
      --exclude='*.log' \
      --include='assets/***' \
      --include='bevy_rogue.js' \
      --include='bevy_rogue_bg.wasm' \
      --include='bevy_rogue_bg.wasm.gz' \
      --include='bevy_rogue_bg.wasm.br' \
      --exclude='*' \
      "${SRC}/" "${DEST}/"
else
    warn "rsync 미설치 — cp 폴백 사용(대상 디렉터리 청소 후 복사)"
    # 대상 디렉터리 비우기 (DEST 자체는 유지 — 권한/소유자 보존).
    find "${DEST}" -mindepth 1 -delete
    # 글루 + wasm 본체 + 사전압축본.
    cp -f "${SRC}/bevy_rogue.js" "${DEST}/"
    cp -f "${SRC}/bevy_rogue_bg.wasm" "${DEST}/"
    [[ -f "${SRC}/bevy_rogue_bg.wasm.gz" ]] && cp -f "${SRC}/bevy_rogue_bg.wasm.gz" "${DEST}/"
    [[ -f "${SRC}/bevy_rogue_bg.wasm.br" ]] && cp -f "${SRC}/bevy_rogue_bg.wasm.br" "${DEST}/"
    # assets 디렉터리.
    if [[ -d "${SRC}/assets" ]]; then
        cp -r "${SRC}/assets" "${DEST}/"
    fi
fi

# 4) 요약
log "publish 완료 — 파일 목록:"
( cd "${DEST}" && find . -type f -printf '  %p  %s bytes\n' | sort ) || true

TOTAL=$(du -sb "${DEST}" 2>/dev/null | cut -f1 || echo "?")
fmt() { numfmt --to=iec --suffix=B "${1}" 2>/dev/null || echo "${1}B"; }
log "합계 사이즈: $(fmt "${TOTAL}")"
log "대상 경로: ${DEST}"
