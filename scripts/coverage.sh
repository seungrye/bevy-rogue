#!/usr/bin/env bash
# 소스 기반 커버리지 측정 스크립트 (llvm-cov)
#
# 사용법:
#   scripts/coverage.sh            # 측정 + 요약 리포트(텍스트)
#   scripts/coverage.sh html       # 추가로 HTML 리포트 생성 (coverage/html/index.html)
#   scripts/coverage.sh show FILE  # 특정 파일의 라인별 커버리지 출력 (미커버 라인 확인용)
#
# stable 툴체인 기준 region/line 커버리지를 측정한다.
# nightly 가 있으면 RUST_COV_BRANCH=1 환경변수로 진짜 branch 커버리지를 켤 수 있다.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
COV_DIR="$ROOT/coverage"
PROFRAW_DIR="$COV_DIR/profraw"
PROFDATA="$COV_DIR/cov.profdata"

# nightly + branch 커버리지 옵션
RUSTFLAGS_COV="-C instrument-coverage"
CARGO="cargo"
TOOLCHAIN_ARG=""
NIGHTLY_BIN="$HOME/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin"
LLVM_PROFDATA="${LLVM_PROFDATA:-llvm-profdata-19}"
LLVM_COV="${LLVM_COV:-llvm-cov-19}"
if [[ "${RUST_COV_BRANCH:-0}" == "1" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
    TOOLCHAIN_ARG="+nightly"
    RUSTFLAGS_COV="-C instrument-coverage -Z coverage-options=branch"
    # 버전 일치를 위해 nightly 툴체인의 llvm 툴 사용
    LLVM_PROFDATA="$NIGHTLY_BIN/llvm-profdata"
    LLVM_COV="$NIGHTLY_BIN/llvm-cov"
fi

mkdir -p "$PROFRAW_DIR"
rm -f "$PROFRAW_DIR"/*.profraw "$PROFDATA" 2>/dev/null || true

echo "[coverage] 계측 빌드 + 테스트 실행..."
RUSTFLAGS="$RUSTFLAGS_COV" \
LLVM_PROFILE_FILE="$PROFRAW_DIR/cov-%p-%m.profraw" \
    $CARGO $TOOLCHAIN_ARG test --tests --no-fail-fast >"$COV_DIR/test.out" 2>"$COV_DIR/test.log" || {
        echo "[coverage] 테스트/빌드 실패 — 아래는 요약 (전체: coverage/test.out, coverage/test.log)";
        grep -E "FAILED|error\[|error:|panicked|test result:" "$COV_DIR/test.out" "$COV_DIR/test.log" 2>/dev/null | tail -40;
        exit 1; }
grep -E "test result:" "$COV_DIR/test.out" 2>/dev/null | tail -5 || true

echo "[coverage] profdata 병합..."
"$LLVM_PROFDATA" merge -sparse "$PROFRAW_DIR"/*.profraw -o "$PROFDATA"

# 테스트 바이너리 경로 수집 (instrument-coverage 로 빌드된 것)
mapfile -t BINS < <(
    RUSTFLAGS="$RUSTFLAGS_COV" $CARGO $TOOLCHAIN_ARG test --tests --no-run --message-format=json 2>/dev/null \
    | grep -o '"executable":"[^"]*"' | sed 's/"executable":"//;s/"//' | grep -v '^null$'
)
OBJ_ARGS=()
for b in "${BINS[@]}"; do [[ -n "$b" ]] && OBJ_ARGS+=("--object" "$b"); done

IGNORE='--ignore-filename-regex=(/\.cargo/|/rustc/|/\.rustup/|tests?\.rs$)'
BRANCH_FLAG=""
[[ "${RUST_COV_BRANCH:-0}" == "1" ]] && BRANCH_FLAG="--show-branches=count"

CMD="${1:-report}"
case "$CMD" in
    html)
        "$LLVM_COV" show "${OBJ_ARGS[@]}" --instr-profile="$PROFDATA" \
            --sources src $IGNORE $BRANCH_FLAG \
            --format=html --output-dir="$COV_DIR/html" \
            --show-line-counts-or-regions
        echo "[coverage] HTML: $COV_DIR/html/index.html"
        "$LLVM_COV" report "${OBJ_ARGS[@]}" --instr-profile="$PROFDATA" --sources src $IGNORE
        ;;
    show)
        shift
        "$LLVM_COV" show "${OBJ_ARGS[@]}" --instr-profile="$PROFDATA" \
            $IGNORE $BRANCH_FLAG --show-line-counts-or-regions "$@"
        ;;
    *)
        "$LLVM_COV" report "${OBJ_ARGS[@]}" --instr-profile="$PROFDATA" --sources src $IGNORE
        ;;
esac
