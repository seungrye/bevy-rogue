#!/usr/bin/env bash
# bevy-rogue WASM PoC 빌드 스크립트.
#
# - wasm32-unknown-unknown 타깃 + wasm-bindgen-cli 0.2.100 필요(Cargo.lock 과 정확히 일치).
# - .cargo/config.toml 에 RUSTFLAGS=--cfg getrandom_backend="wasm_js" 가 박혀 있어야 한다.
# - 출력: dist-wasm/{bevy_rogue.js, bevy_rogue_bg.wasm, index.html, assets/}.
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

# 5) assets 복사
rm -rf "${OUT}/assets"
cp -r assets "${OUT}/assets"

# 6) index.html 은 git 추적 — 없으면 만들어 둔다(첫 실행 호환).
if [ ! -f "${OUT}/index.html" ]; then
  cat > "${OUT}/index.html" <<'HTML'
<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"/>
<title>bevy-rogue (WASM PoC)</title>
<style>html,body{margin:0;background:#111;color:#ccc;font-family:monospace}canvas#bevy-canvas{display:block;margin:0 auto;background:#000}</style>
</head><body><div id="loader" style="position:fixed;top:8px;left:8px;background:#0008;padding:4px 8px">loading…</div>
<canvas id="bevy-canvas"></canvas>
<script type="module">import init from './bevy_rogue.js';
const l=document.getElementById('loader');
try{await init();l.textContent='running';setTimeout(()=>l.style.display='none',4000);}catch(e){l.textContent='init 실패: '+e;console.error(e);}
</script></body></html>
HTML
fi

# 7) wasm-opt (선택)
if command -v wasm-opt >/dev/null 2>&1; then
  echo "[wasm-build] wasm-opt -O3"
  wasm-opt -O3 -o "${OUT}/bevy_rogue_bg.opt.wasm" "${OUT}/bevy_rogue_bg.wasm"
  mv "${OUT}/bevy_rogue_bg.opt.wasm" "${OUT}/bevy_rogue_bg.wasm"
else
  echo "[wasm-build] wasm-opt 미설치 — 최적화 건너뜀(binaryen apt 또는 cargo install wasm-opt 권장)."
fi

echo "[wasm-build] 완료. 출력:"
ls -lh "${OUT}/" | sed 's/^/    /'
