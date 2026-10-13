// 按需浏览器集成验收：小样本触发映射，用真实滚轮/键盘验证边界。
// 仅访问 Vite DEV gallery harness，后端为夹具；不启动 Tauri 或原生 worker。
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import puppeteer from 'puppeteer-core';
import { testCase } from '../lib/test-case.mjs';

const arg = (name, fallback) => process.argv.find(a => a.startsWith(`--${name}=`))?.slice(name.length + 3) ?? fallback;
const origin = new URL(arg('origin', 'http://127.0.0.1:1436'));
assert(origin.protocol === 'http:' && ['127.0.0.1', 'localhost'].includes(origin.hostname), '仅使用本机开发夹具');
const executablePath = process.env.CHROME_PATH ?? [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
].find(existsSync);
assert(executablePath, '需要已有 Chrome/Edge，可用 CHROME_PATH 指定；脚本不下载浏览器');
const out = resolve(arg('out', 'target/T10/browser'));
mkdirSync(out, { recursive: true });
const report = { environment: 'Vite DEV gallery harness / headless browser / GPU disabled', items: 80, spacerCap: 1800, steps: [], pageErrors: [], failedResources: [] };
const browser = await puppeteer.launch({ executablePath, headless: true, userDataDir: mkdtempSync(join(out, 'profile-')), args: ['--disable-gpu', '--no-first-run'], defaultViewport: { width: 1100, height: 800 } });
let page;
let snapshot;

try {
  report.browser = await browser.version();
  page = await browser.newPage();
  page.on('pageerror', e => report.pageErrors.push(String(e)));
  page.on('response', r => { if (r.status() >= 400) report.failedResources.push({ url: r.url(), status: r.status() }); });
  await page.evaluateOnNewDocument(() => {
    localStorage.setItem('scrollery.debug.bucketSpacer', '1800');
  });
  await page.goto(new URL('/?ui-harness=gallery&items=80&rowHeight=160&render=dom', origin).href, { waitUntil: 'networkidle0', timeout: 60000 });
  await page.waitForSelector('.media-grid__content--bucket', { timeout: 15000 });
  // 网络静默不等于布局恢复完成；沿用产品已有的稳定屏障后才发送输入。
  await page.evaluate(async () => {
    await document.fonts.ready;
    await document.querySelector('.media-grid').__vueParentComponent.setupState.bucketScroll.whenSettled();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });

  // DEV 组件公开给调试器的读数用于观测；操作仍由真实鼠标/键盘产生，不写内部状态。
  snapshot = () => page.evaluate(() => {
    const el = document.querySelector('.media-grid');
    const state = el.__vueParentComponent.setupState;
    return { logical: state.currentLogicalY, total: state.media.totalHeight, physical: el.scrollTop, physicalHeight: el.scrollHeight, viewport: el.clientHeight, canvas: state.canvasActive };
  });
  const initial = await snapshot();
  assert.equal(initial.physicalHeight, report.spacerCap, '开发封顶入口未生效');
  assert(initial.total > report.spacerCap, '样本没有进入映射态');
  assert.equal(initial.canvas, false, '本场景固定使用 DOM 渲染');
  report.initial = initial;
  const box = await page.$eval('.media-grid', el => { const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
  await page.mouse.move(box.x, box.y);
  await page.focus('.media-grid');

  await testCase('bucket:end-after-wheel', async () => {
    const step = { name: 'bucket:end-after-wheel', passed: false, action: 'wheel' };
    report.steps.push(step);
    await page.mouse.wheel({ deltaY: 320 });
    await page.waitForFunction(() => document.querySelector('.media-grid').__vueParentComponent.setupState.currentLogicalY > 0, { timeout: 3000 });
    const before = await snapshot();
    step.before = before;
    step.action = 'End';
    await page.keyboard.press('End');
    await page.waitForFunction(() => {
      const el = document.querySelector('.media-grid'), s = el.__vueParentComponent.setupState;
      return Math.abs(s.currentLogicalY - (s.media.totalHeight - el.clientHeight)) <= 1;
    }, { timeout: 3000 });
    const after = await snapshot();
    assert(Math.abs(after.logical - (after.total - after.viewport)) <= 1);
    Object.assign(step, { passed: true, after });
    await page.screenshot({ path: join(out, 'end.png') });
  });

  await testCase('bucket:home-after-wheel', async () => {
    const step = { name: 'bucket:home-after-wheel', passed: false, action: 'wheel' };
    report.steps.push(step);
    await page.mouse.wheel({ deltaY: -320 });
    await page.waitForFunction(() => {
      const el = document.querySelector('.media-grid'), s = el.__vueParentComponent.setupState;
      return s.currentLogicalY < s.media.totalHeight - el.clientHeight - 1;
    }, { timeout: 3000 });
    const before = await snapshot();
    step.before = before;
    step.action = 'Home';
    await page.keyboard.press('Home');
    await page.waitForFunction(() => document.querySelector('.media-grid').__vueParentComponent.setupState.currentLogicalY <= 1, { timeout: 3000 });
    const after = await snapshot();
    assert(after.logical <= 1);
    Object.assign(step, { passed: true, after });
    await page.screenshot({ path: join(out, 'home.png') });
  });
  assert.deepEqual(report.pageErrors, [], '页面出现未捕获异常');
} catch (error) {
  report.failure = String(error);
  if (snapshot) report.finalState = await snapshot();
  if (page) await page.screenshot({ path: join(out, 'failure.png') });
  process.exitCode = 1;
} finally {
  await browser.close();
  writeFileSync(join(out, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
