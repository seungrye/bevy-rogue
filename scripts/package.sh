#!/usr/bin/env bash
# bevy-rogue 배포 패키지 생성
#   - 릴리즈 바이너리 + assets/ 를 dist/ 에 묶어 tar.gz 생성
#   - 게임은 런타임에 assets/ 를 cwd 기준으로 읽으므로 바이너리와 assets 를 함께 배포한다.
#   - 주의: dynamic_linking 은 dev 전용. 배포는 정적(기본) 릴리즈로 빌드한다(이 스크립트는 --release 기본 빌드).
#
# 사용법:
#   scripts/package.sh
#   NEXT_FEATURES= scripts/package.sh   # (참고용; 이 스크립트는 추가 피처 없이 빌드)
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
BIN_NAME="bevy-rogue"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
PKG="${BIN_NAME}-${VERSION}-${ARCH}-linux"
OUT="$ROOT/dist"
STAGE="$OUT/$PKG"

echo "[package] 릴리즈 빌드 (정적, dynamic_linking 미사용)..."
# 배포용은 명시적으로 기본 피처만 — dev 가속 피처(dynamic_linking 등)가 섞이지 않게.
cargo build --release --bin "$BIN_NAME"

echo "[package] 스테이징: $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE"

cp "target/release/$BIN_NAME" "$STAGE/$BIN_NAME"
# 디버그 심볼 제거(용량 축소) — strip 있으면.
command -v strip >/dev/null 2>&1 && strip --strip-all "$STAGE/$BIN_NAME" || true

# 런타임 필수 에셋 동봉.
cp -r assets "$STAGE/assets"

# 실행 안내.
cat > "$STAGE/README.txt" <<EOF
bevy-rogue ${VERSION} (${ARCH} linux)

실행:
  이 디렉터리에서  ./${BIN_NAME}
  (assets/ 가 바이너리와 같은 폴더에 있어야 합니다.)

옵션:
  ./${BIN_NAME} --help
  ./${BIN_NAME} --algorithm <생성기>   --glyph-style <ascii|unicode|icon>

저장 파일은 실행 디렉터리의 save/ 에 생성됩니다.
EOF

echo "[package] 압축..."
tar -czf "$OUT/${PKG}.tar.gz" -C "$OUT" "$PKG"

echo "[package] 완료:"
echo "  폴더 : $STAGE"
echo "  아카이브: $OUT/${PKG}.tar.gz"
du -sh "$OUT/${PKG}.tar.gz" 2>/dev/null || true
