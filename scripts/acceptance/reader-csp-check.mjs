// 真实浏览器执行产品 paginator，复现 Tauri 给 style-src 注入 nonce 后的动态样式约束。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import puppeteer from 'puppeteer-core';
import { testCase } from '../lib/test-case.mjs';

const out = path.resolve(process.argv.find(a => a.startsWith('--out='))?.slice(6) ?? 'target/T11/browser');
fs.mkdirSync(out, { recursive: true });
const vendor = path.resolve('src/vendor/foliate-js');
const nonce = 'scrollery-reader-regression';
const csp = `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' 'nonce-${nonce}'; frame-src 'self' blob:`;
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  if (url.pathname === '/') {
    res.writeHead(200, { 'Content-Type': 'text/html', 'Content-Security-Policy': csp });
    res.end(`<html><head><style id="theme-default-css" nonce="${nonce}">body{margin:0}foliate-paginator{display:block;width:800px;height:600px}</style></head><body></body></html>`);
  } else {
    const file = path.resolve(vendor, '.' + decodeURIComponent(url.pathname));
    if (!file.startsWith(vendor + path.sep) || !fs.existsSync(file) || !file.endsWith('.js')) {
      res.writeHead(404); res.end(); return;
    }
    res.writeHead(200, { 'Content-Type': 'text/javascript' });
    res.end(fs.readFileSync(file));
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const executablePath = process.env.CHROME_PATH ?? [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
].find(fs.existsSync);
const report = { csp, browser: null, pageErrors: [], violations: [], cases: [] };
let browser;
try {
  assert(executablePath, '需要已有 Chrome/Edge，不安装浏览器');
  browser = await puppeteer.launch({ executablePath, headless: true, args: ['--disable-gpu', '--no-first-run'], userDataDir: fs.mkdtempSync(path.join(out, 'profile-')), defaultViewport: { width: 900, height: 700 } });
  report.browser = await browser.version();
  const page = await browser.newPage();
  page.on('pageerror', error => report.pageErrors.push(String(error)));
  page.on('console', msg => { if (msg.type() === 'error' && msg.text().includes('Content Security Policy')) report.violations.push(msg.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async () => {
    await import('/paginator.js');
    const reader = document.createElement('foliate-paginator');
    document.body.append(reader);
    const html = '<html><head><style id="chapter-style">body{color:rgb(255,0,0)!important}</style></head><body><h1>CSP reader visible</h1><p>Bounded fixture</p></body></html>';
    const blob = URL.createObjectURL(new Blob([html], { type: 'text/html' }));
    reader.open({ dir: 'ltr', sections: [{ load: () => blob, unload() {} }] });
    reader.setStyles(['body{color:rgb(1,2,3);font-size:20px}', '']);
    await reader.goTo({ index: 0, anchor: 0 });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
  const snapshot = await page.evaluate(() => {
    const doc = document.querySelector('foliate-paginator').getContents()[0].doc;
    const frame = doc.defaultView.frameElement.getBoundingClientRect();
    return { text: doc.body.innerText, frame: frame.toJSON(), viewport: { width: innerWidth, height: innerHeight }, color: doc.defaultView.getComputedStyle(doc.body).color, styles: Array.from(doc.querySelectorAll('style')).map(s => ({ id: s.id, nonce: s.nonce, active: !!s.sheet })) };
  });
  report.snapshot = snapshot;
  await page.screenshot({ path: path.join(out, 'reader.png') });
  await testCase('reader-csp:owned-style-renders', () => {
    assert(snapshot.text.includes('CSP reader visible'));
    assert(snapshot.frame.width > 0 && snapshot.frame.height > 0 && snapshot.frame.left < snapshot.viewport.width && snapshot.frame.right > 0, JSON.stringify(snapshot));
    assert.equal(snapshot.color, 'rgb(1, 2, 3)', '阅读器自有排版被 CSP 拦截');
    assert(snapshot.styles.filter(s => !s.id).every(s => s.active));
    report.cases.push({ name: 'reader-csp:owned-style-renders', ok: true });
  });
  await testCase('reader-csp:chapter-style-remains-untrusted', () => {
    const chapter = snapshot.styles.find(s => s.id === 'chapter-style');
    assert.deepEqual(chapter, { id: 'chapter-style', nonce: '', active: false });
    assert(report.violations.length > 0, '未观察到章节 style 被策略拒绝');
    assert.equal(report.pageErrors.length, 0);
    report.cases.push({ name: 'reader-csp:chapter-style-remains-untrusted', ok: true });
  });
} catch (error) {
  report.error = error.message;
  process.exitCode = 1;
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ out, passed: report.cases.length, error: report.error }));
}
