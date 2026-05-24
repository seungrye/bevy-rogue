#!/usr/bin/env bash
# 기존 coverage/cov.profdata 기준으로 특정 소스 파일의 미커버(실행 0회) 라인을 출력.
# 재컴파일 없이 빠르게 조회한다.
#   scripts/uncovered.sh src/modules/elemental/mod.rs
#   scripts/uncovered.sh src/modules/item/mod.rs | head -60
set -euo pipefail
cd "$(dirname "$0")/.."
PROFDATA="coverage/cov.profdata"
LLVM_COV="${LLVM_COV:-llvm-cov-19}"

mapfile -t BINS < <(ls -t target/debug/deps/bevy_rogue-* 2>/dev/null | grep -vE '\.(d|profraw|profdata)$')
OBJ=(); for b in "${BINS[@]}"; do OBJ+=(--object "$b"); done

"$LLVM_COV" show "${OBJ[@]}" --instr-profile="$PROFDATA" --sources "$1" 2>/dev/null \
  | grep -E '^[[:space:]]*[0-9]+\|[[:space:]]*0\|' || echo "(미커버 라인 없음 — 100%)"
