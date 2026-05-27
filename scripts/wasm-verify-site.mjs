#!/usr/bin/env node
// Stage 3 — site 통합 헤드리스 검증.
//
// 사용:
//   node scripts/wasm-verify-site.mjs [URL]
//   기본 URL: http://127.0.0.1:3099/games/bevy-rogue
//
// 동작:
//   1. headless chromium 으로 페이지 로드 (WebGL2 활성).
//   2. console message + page error 수집(stage 1 화이트리스트와 동일).
//   3. 캔버스 등장 + 첫 프레임 렌더까지 10초 대기(번들 다운로드 여유).
//   4. dist-wasm/stage3-site-screenshot.png 로 스크린샷 저장.
//   5. JS 콘솔 error 0 + 캔버스 OK 이면 exit 0.

const playwrightDir = process.env.PLAYWRIGHT_NODE_PATH;
const { chromium } = playwrightDir
  ? await import(`${playwrightDir}/playwright/index.mjs`)
  : await import('playwright');
import { mkdirSync, writeFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot  = resolve(__dirname, '..');
const outDir    = resolve(repoRoot, 'dist-wasm');
const url       = process.argv[2] ?? 'http://127.0.0.1:3099/games/bevy-rogue';
const shotPath  = resolve(outDir, 'stage3-site-screenshot.png');
const logPath   = resolve(outDir, 'stage3-site-console.log');

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
  viewport: { width: 1400, height: 900 },
});
const page = await context.newPage();

// Stage 1 화이트리스트 + 사이트 측 GA(Firebase Analytics) 비콘은 무해.
// - winit "Using exceptions for control flow": EventLoop 진입 신호.
// - default 폰트 404: Bevy 가 미사용 default 폰트 시도.
// - www.google.com/g/collect: GA4 가 비콘을 보내는 도메인. site 의 CSP 가
//   www.google-analytics.com 만 허용해 차단됨 — 이건 stage 3 와 무관한
//   기존 사이트 동작이고, 게임 실행에 영향 없음. (별도 이슈로 분리.)
// - ERR_CERT_COMMON_NAME_INVALID: 위 GA 비콘이 차단되면서 발생하는 부산물.
const benignErrorRegexes = [
  /Using exceptions for control flow/,
  /^Failed to load resource: the server responded with a status of 404/,
  /https:\/\/www\.google\.com\/g\/collect/,
  /Refused to connect because it violates the document's Content Security Policy/,
  /Fetch API cannot load https:\/\/www\.google\.com\/g\/collect/,
  /ERR_CERT_COMMON_NAME_INVALID/,
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

console.log(`[verify-site] loading ${url}`);
await page.goto(url, { waitUntil: 'load', timeout: 30000 });

// 캔버스 등장 대기 — BevyRogueClient 가 렌더하자마자 보임.
await page.waitForSelector('canvas#bevy-canvas', { timeout: 15000 });

// 첫 프레임 + 시뮬 워밍업 (wasm 다운로드 22MB → 로컬 빠름, 안전하게 10초).
await page.waitForTimeout(10000);

const canvasInfo = await page.evaluate(() => {
  const c = document.getElementById('bevy-canvas');
  if (!c) return { ok: false, reason: 'canvas 없음' };
  const w = c.width, h = c.height;
  if (w === 0 || h === 0) return { ok: false, reason: `0크기 (${w}x${h})` };
  // 로더 오버레이가 사라졌는지(=초기화 완료) 확인.
  const loaderVisible = !!document.querySelector('[role="status"]');
  const errorVisible = !!document.querySelector('[role="alert"]');
  return { ok: true, w, h, loaderVisible, errorVisible };
});

console.log(`[verify-site] canvas: ${JSON.stringify(canvasInfo)}`);

await page.screenshot({ path: shotPath, fullPage: false });
const stat = statSync(shotPath);
console.log(`[verify-site] screenshot saved: ${shotPath} (${stat.size} bytes)`);

await browser.close();

console.log('────────────────────────────────────────────────');
console.log(`[verify-site] console errors:   ${errors.length}`);
console.log(`[verify-site] console warnings: ${warnings.length}`);
console.log(`[verify-site] console infos:    ${infos.length}`);
if (errors.length) {
  console.log('── errors ─────────────────────────');
  for (const e of errors) console.log('  ' + e);
}
if (warnings.length) {
  console.log('── warnings (top 20) ──────────────');
  for (const w of warnings.slice(0, 20)) console.log('  ' + w);
  if (warnings.length > 20) console.log(`  …(+${warnings.length - 20} more)`);
}

writeFileSync(logPath,
  [`URL: ${url}`,
   `canvas: ${JSON.stringify(canvasInfo)}`,
   `screenshot: ${shotPath} (${stat.size} bytes)`,
   '── errors ──', ...errors,
   '── warnings ──', ...warnings,
   '── infos ──', ...infos,
  ].join('\n'));

if (!canvasInfo.ok) {
  console.error(`[verify-site] FAIL: canvas 검증 실패 — ${canvasInfo.reason}`);
  process.exit(1);
}
if (canvasInfo.errorVisible) {
  console.error('[verify-site] FAIL: 에러 오버레이가 화면에 노출됨(role="alert").');
  process.exit(1);
}
if (errors.length > 0) {
  console.error('[verify-site] FAIL: JS 콘솔 error 가 있음 (위 목록 참조).');
  process.exit(1);
}
console.log('[verify-site] OK: 콘솔 에러 0 + 캔버스 OK + 에러 오버레이 없음.');
