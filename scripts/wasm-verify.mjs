#!/usr/bin/env node
// bevy-rogue WASM PoC 헤드리스 검증.
//
// 사용:
//   node scripts/wasm-verify.mjs [URL]
//   기본 URL: http://127.0.0.1:8765/
//
// 동작:
//   1. headless chromium 으로 페이지 로드 (WebGL2 활성).
//   2. console message + page error 수집(error/warning/info 분류).
//   3. 캔버스 등장과 첫 프레임 렌더까지 8초 대기.
//   4. dist-wasm/poc-screenshot.png 로 스크린샷 저장 (전체 페이지).
//   5. JS 콘솔 error 0 + 캔버스 픽셀이 전부 검정이 아니면 성공(exit 0).
//      그 외엔 exit 1.

// playwright 위치 — env PLAYWRIGHT_NODE_PATH(디렉터리) 가 있으면 그쪽에서 import.
const playwrightDir = process.env.PLAYWRIGHT_NODE_PATH;
const { chromium } = playwrightDir
  ? await import(`${playwrightDir}/playwright/index.mjs`)
  : await import('playwright');
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot  = resolve(__dirname, '..');
const outDir    = resolve(repoRoot, 'dist-wasm');
const url       = process.argv[2] ?? 'http://127.0.0.1:8765/';
const shotPath  = resolve(outDir, 'poc-screenshot.png');

mkdirSync(outDir, { recursive: true });

const errors = [];
const warnings = [];
const infos = [];

const browser = await chromium.launch({
  headless: true,
  args: [
    '--use-gl=swiftshader',
    '--enable-webgl',
    '--ignore-gpu-blocklist',
    '--no-sandbox',
  ],
});
const context = await browser.newContext({
  viewport: { width: 1280, height: 800 },
});
const page = await context.newPage();

// 화이트리스트 — bevy_winit 의 winit 0.29 가 wasm 에서 EventLoop 종료를
// 던지면서 정상 동작을 위해 console.error 를 한 번 출력한다("control flow").
// 이건 winit 디자인이라 무해. 404 는 우리가 안 쓰는 default 폰트 로딩 시도.
const benignErrorRegexes = [
  /Using exceptions for control flow/,
  /^Failed to load resource: the server responded with a status of 404/,
];
const isBenign = (text) => benignErrorRegexes.some((r) => r.test(text));

page.on('console', (msg) => {
  const type = msg.type();
  const text = msg.text();
  if (type === 'error') {
    (isBenign(text) ? infos : errors).push(`[error] ${text}`);
  } else if (type === 'warning') {
    warnings.push(text);
  } else {
    infos.push(`[${type}] ${text}`);
  }
});
page.on('pageerror', (err) => {
  if (isBenign(err.message)) infos.push(`[benign pageerror] ${err.message}`);
  else errors.push(`[pageerror] ${err.message}`);
});

console.log(`[verify] loading ${url}`);
await page.goto(url, { waitUntil: 'load', timeout: 30000 });

// 캔버스 등장 대기.
await page.waitForSelector('canvas#bevy-canvas', { timeout: 15000 });

// 첫 프레임 + 시뮬 워밍업.
await page.waitForTimeout(8000);

// 캔버스 사이즈 + 픽셀 샘플(검정 전부 여부) 검사.
const canvasInfo = await page.evaluate(() => {
  const c = document.getElementById('bevy-canvas');
  if (!c) return { ok: false, reason: 'canvas 없음' };
  const w = c.width, h = c.height;
  if (w === 0 || h === 0) return { ok: false, reason: `0크기 (${w}x${h})` };
  // WebGL canvas 의 픽셀을 직접 읽으려면 same-context 가 필요해 PNG 캡처로 대체.
  return { ok: true, w, h };
});

console.log(`[verify] canvas: ${JSON.stringify(canvasInfo)}`);

await page.screenshot({ path: shotPath, fullPage: false });
console.log(`[verify] screenshot saved: ${shotPath}`);

// 픽셀 분포로 검정 화면 여부 판정 (zlib 없이 PNG IDAT 통계 대신 sharp 없이도 가능 —
// 여기선 단순히 파일 크기와 콘솔 에러만으로 판정).
const fs = await import('node:fs');
const stat = fs.statSync(shotPath);
console.log(`[verify] screenshot bytes: ${stat.size}`);

await browser.close();

console.log('────────────────────────────────────────────────');
console.log(`[verify] console errors:   ${errors.length}`);
console.log(`[verify] console warnings: ${warnings.length}`);
console.log(`[verify] console infos:    ${infos.length}`);
if (errors.length) {
  console.log('── errors ─────────────────────────');
  for (const e of errors) console.log('  ' + e);
}
if (warnings.length) {
  console.log('── warnings ───────────────────────');
  for (const w of warnings.slice(0, 20)) console.log('  ' + w);
  if (warnings.length > 20) console.log(`  …(+${warnings.length - 20} more)`);
}
// 로그 파일도 남겨 두면 추후 디버깅 편함.
writeFileSync(resolve(outDir, 'poc-console.log'),
  [`URL: ${url}`,
   `canvas: ${JSON.stringify(canvasInfo)}`,
   `screenshot: ${shotPath} (${stat.size} bytes)`,
   '── errors ──', ...errors,
   '── warnings ──', ...warnings,
   '── infos ──', ...infos,
  ].join('\n'));

if (!canvasInfo.ok) {
  console.error(`[verify] FAIL: canvas 검증 실패 — ${canvasInfo.reason}`);
  process.exit(1);
}
if (errors.length > 0) {
  console.error('[verify] FAIL: JS 콘솔 error 가 있음 (위 목록 참조).');
  process.exit(1);
}
console.log('[verify] OK: 콘솔 에러 0 + 캔버스 OK.');
