#!/usr/bin/env bash
# bevy-rogue WASM 빌드 스크립트 (stage 2).
#
# - wasm32-unknown-unknown 타깃 + wasm-bindgen-cli 0.2.100 필요(Cargo.lock 과 정확히 일치).
# - .cargo/config.toml 에 RUSTFLAGS=--cfg getrandom_backend="wasm_js" 가 박혀 있어야 한다.
# - 출력: dist-wasm/{bevy_rogue.js, bevy_rogue_bg.wasm[.br], index.html, assets/}.
#
# stage 2 추가:
#   - wasm-opt -Oz 자동 시도 (없으면 cargo install / apt 자동 설치 시도; 실패시 경고만).
#   - brotli -q 11 압축 (없으면 경고만 — nginx 가 .br 을 정적 서빙).
#   - 단계별 사이즈 보고 (원본 / wasm-opt / gzip / brotli).
#
# 사용: scripts/wasm-build.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# 0) PATH 보강 (~/.cargo/bin)
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

# 1) wasm32 타깃
if command -v rustup >/dev/null 2>&1; then
  rustup target add wasm32-unknown-unknown >/dev/null
fi

# 2) wasm-bindgen-cli (Cargo.lock 과 같은 0.2.100 정확 일치)
WB_VER="0.2.100"
if ! command -v wasm-bindgen >/dev/null 2>&1 || ! wasm-bindgen --version | grep -q " ${WB_VER}\$"; then
  echo "[wasm-build] wasm-bindgen-cli ${WB_VER} 설치/갱신…"
  cargo install --locked wasm-bindgen-cli --version "${WB_VER}"
fi

# 3) wasm 빌드
echo "[wasm-build] cargo build --release --target wasm32-unknown-unknown --lib"
cargo build --release --target wasm32-unknown-unknown --lib

# 4) bindgen → web glue
OUT="dist-wasm"
mkdir -p "${OUT}"
echo "[wasm-build] wasm-bindgen → ${OUT}/"
wasm-bindgen --target web --out-dir "${OUT}" --no-typescript \
  target/wasm32-unknown-unknown/release/bevy_rogue.wasm

WASM="${OUT}/bevy_rogue_bg.wasm"
RAW_SIZE=$(stat -c%s "${WASM}" 2>/dev/null || stat -f%z "${WASM}")

# 5) assets 복사
rm -rf "${OUT}/assets"
cp -r assets "${OUT}/assets"

# 6) index.html 은 git 추적 — 없으면 만들어 둔다(첫 실행 호환).
if [ ! -f "${OUT}/index.html" ]; then
  cat > "${OUT}/index.html" <<'HTML'
<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"/>
<title>bevy-rogue (WASM)</title>
<style>html,body{margin:0;background:#111;color:#ccc;font-family:monospace}canvas#bevy-canvas{display:block;margin:0 auto;background:#000}</style>
</head><body><div id="loader" style="position:fixed;top:8px;left:8px;background:#0008;padding:4px 8px">loading…</div>
<canvas id="bevy-canvas"></canvas>
<script type="module">import init from './bevy_rogue.js';
const l=document.getElementById('loader');
try{await init();l.textContent='running';setTimeout(()=>l.style.display='none',4000);}catch(e){l.textContent='init 실패: '+e;console.error(e);}
</script></body></html>
HTML
fi

# 7) wasm-opt: 있으면 -Oz 로 자리 교체. 없으면 자동 설치 시도 후 다시.
ensure_wasm_opt() {
  if command -v wasm-opt >/dev/null 2>&1; then return 0; fi
  echo "[wasm-build] wasm-opt 미설치 — 자동 설치 시도…"
  # 우선 cargo install wasm-opt (rust 포팅, 가장 호환성 좋음).
  if cargo install wasm-opt --locked 2>/tmp/wasm-opt-install.log; then
    echo "[wasm-build] cargo install wasm-opt 완료."
    return 0
  fi
  echo "[wasm-build] cargo install wasm-opt 실패. apt-get install binaryen 시도…" >&2
  if command -v apt-get >/dev/null 2>&1; then
    if sudo -n apt-get install -y binaryen 2>/dev/null; then
      echo "[wasm-build] apt-get install binaryen 완료."
      return 0
    fi
  fi
  return 1
}

OPT_SIZE=""
if ensure_wasm_opt; then
  echo "[wasm-build] wasm-opt -Oz"
  if wasm-opt -Oz -o "${WASM}.opt" "${WASM}"; then
    mv "${WASM}.opt" "${WASM}"
    OPT_SIZE=$(stat -c%s "${WASM}" 2>/dev/null || stat -f%z "${WASM}")
  else
    echo "[wasm-build] 경고: wasm-opt 실행 실패 — 원본 wasm 유지."
  fi
else
  echo "[wasm-build] 경고: wasm-opt 사용 불가 — 최적화 건너뜀(원본 유지)."
fi

# 8) gzip / brotli 사전 압축 — nginx 가 Content-Encoding 으로 직접 서빙용.
GZ_SIZE=""
if command -v gzip >/dev/null 2>&1; then
  gzip -kf -9 "${WASM}"  # bevy_rogue_bg.wasm.gz
  GZ_SIZE=$(stat -c%s "${WASM}.gz" 2>/dev/null || stat -f%z "${WASM}.gz")
fi

BR_SIZE=""
if command -v brotli >/dev/null 2>&1; then
  brotli -fq 11 -o "${WASM}.br" "${WASM}"
  BR_SIZE=$(stat -c%s "${WASM}.br" 2>/dev/null || stat -f%z "${WASM}.br")
else
  echo "[wasm-build] 경고: brotli 미설치 — .br 사전 압축 건너뜀(apt install brotli 권장)."
fi

# 9) 사이즈 요약.
fmt() { numfmt --to=iec --suffix=B "${1}" 2>/dev/null || echo "${1}B"; }
echo "────────────────────────────────────────────────"
echo "[wasm-build] 사이즈 요약 (bevy_rogue_bg.wasm):"
printf "  원본     : %s\n" "$(fmt "${RAW_SIZE}")"
[ -n "${OPT_SIZE}" ] && printf "  wasm-opt : %s\n" "$(fmt "${OPT_SIZE}")"
[ -n "${GZ_SIZE}" ]  && printf "  gzip -9  : %s\n" "$(fmt "${GZ_SIZE}")"
[ -n "${BR_SIZE}" ]  && printf "  brotli 11: %s\n" "$(fmt "${BR_SIZE}")"
echo "────────────────────────────────────────────────"
echo "[wasm-build] 완료. 출력:"
ls -lh "${OUT}/" | sed 's/^/    /'
