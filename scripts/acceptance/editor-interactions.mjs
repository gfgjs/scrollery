import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

// 夹具通过真实 IPC 入库；菜单、输入与导航走 WebView 中的产品事件处理器。
export async function verifyEditorInteractions(ctx, cdp, helpers) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree } = helpers;
  const read = (expression) => evaluate(cdp, expression);
  const wait = async (expression, label) => {
    const deadline = Date.now() + 15000;
    let result;
    do {
      result = await read(expression);
      if (result) return result;
      await delay(80);
    } while (Date.now() < deadline);
    throw new Error(label + ': ' + JSON.stringify(await read(`({url:location.href,text:document.body.innerText.slice(-1800)})`)));
  };
  const key = async (key, code, virtual, modifiers = 0) => {
    const params = { key, code, windowsVirtualKeyCode: virtual, modifiers };
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', ...params });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...params });
  };
  const viewport = async (width) => {
    await cdp.send('Emulation.setDeviceMetricsOverride', { width, height: 900, deviceScaleFactor: 1, mobile: false });
    await read('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
  };
  const menuSnapshot = `Array.from(document.querySelectorAll('select')).filter(s=>s.querySelector('option[value="markdown-editor"]')).map(s=>({value:s.value,visible:!!s.getClientRects().length&&!s.closest('.fold-item--folded'),overflow:s.classList.contains('doc-more__engine-select'),disabled:s.querySelector('option[value="markdown-editor"]').disabled,title:s.querySelector('option[value="markdown-editor"]').title}))`;
  const selectEngine = async (engine) => {
    await wait(`Array.from(document.querySelectorAll('select')).some(s=>s.querySelector('option[value="markdown-editor"]')&&s.getClientRects().length)`, '引擎菜单未显示');
    await read(`(()=>{const s=Array.from(document.querySelectorAll('select')).find(s=>s.querySelector('option[value="markdown-editor"]')&&s.getClientRects().length);s.value=${JSON.stringify(engine)};s.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    await wait(`location.hash.includes('engine=${engine}')`, '引擎路由未完成');
  };
  const navigate = async (hash) => {
    await read(`location.hash=${JSON.stringify(hash)}`);
    await wait(`location.hash===${JSON.stringify(hash)}`, '文档路由未进入');
  };
  const prefs = async (itemId) => JSON.parse(await ipc(cdp, 'get_reader_book_prefs', { itemId }) || '{}');
  // foliate 使用 closed shadow root，读取其现有只读 getContents 接口中的真实 iframe 正文。
  const bookText = `Array.from(document.querySelector('foliate-view')?.renderer?.getContents()??[]).map(c=>c.doc?.body?.innerText??'').join(' ')`;
  const waitForBookText = () => wait(`${bookText}.includes('Acceptance scratch')`, '阅读器正文尚未呈现');
  const bookVisible = `(()=>{const f=document.querySelector('foliate-view')?.renderer?.getContents()?.[0]?.doc?.defaultView?.frameElement;if(!f)return false;const r=f.getBoundingClientRect();return r.width>0&&r.height>0&&r.left<innerWidth&&r.right>0&&r.top<innerHeight&&r.bottom>0})()`;
  const fixture = fs.mkdtempSync(path.join(ctx.root, 'editor-'));
  const sourcePath = path.join(fixture, 'scratch.md');
  const emptyPath = path.join(fixture, 'empty.md');
  const initial = '# Acceptance scratch\n\n- bounded fixture\n\n```text\ninitial\n```\n';
  fs.writeFileSync(sourcePath, initial);
  fs.writeFileSync(emptyPath, '');
  let itemId;
  let emptyId;
  let capabilityScript;
  try {
    await runStep('editor:fixture-and-capability', async () => {
      await wait('!!document.body && !!window.__TAURI_INTERNALS__', '应用未就绪');
      await read(`(()=>{const b=Array.from(document.querySelectorAll('button')).find(b=>/^(跳过|Skip)$/.test(b.textContent.trim()));b?.click()})()`);
      const root = await ipc(cdp, 'add_scan_root', { path: fixture, alias: 'Editor Acceptance' });
      const channel = await openChannel(cdp);
      await ipc(cdp, 'start_scan', { rootId: root.id, runId: 'editor-' + Date.now(), onProgress: channel, groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false });
      const finished = await waitForChannel(cdp, ev => ev?.type === 'completed' || ev?.type === 'error', ctx.timeoutMs, 'editor-scan');
      assert.equal(finished.hit.type, 'completed');
      const dirs = await walkDirectoryTree(cdp, root.id, ctx.timeoutMs);
      const files = await ipc(cdp, 'list_directory_files', { directoryId: dirs[0].id, limit: 20, offset: 0, categories: null });
      itemId = files.find(f => f.fileName === 'scratch.md')?.id;
      emptyId = files.find(f => f.fileName === 'empty.md')?.id;
      assert.ok(itemId && emptyId, JSON.stringify(files));
      await viewport(1600);
      await navigate('#/doc/' + itemId);
      await wait(`!!document.querySelector('.book-reader')`, '原生阅读器未显示');
      await waitForBookText();
      const capability = await read(`typeof globalThis.EditContext === 'function'`);
      assert.equal(capability, true, '当前 WebView 无 EditContext，支持分支未验证');
      const menus = await read(menuSnapshot);
      assert.ok(menus.some(m => m.visible && !m.overflow && !m.disabled));
      return { fixture, itemId, emptyId, editContext: capability, menus, scanItems: finished.hit.totalItems };
    });

    await runStep('editor:save-and-engine-switch', async () => {
      await selectEngine('markdown-editor');
      await wait(`!!document.querySelector('.md-editor')`, '实验编辑器未挂载');
      await read(`document.querySelector('.md-editor').focus()`);
      await key('End', 'End', 35, 2);
      await cdp.send('Input.insertText', { text: '\nSAVED_BY_CTRL_S' });
      await key('s', 'KeyS', 83, 2);
      const deadline = Date.now() + 10000;
      while (!fs.readFileSync(sourcePath, 'utf8').includes('SAVED_BY_CTRL_S') && Date.now() < deadline) await delay(60);
      const firstSave = fs.readFileSync(sourcePath, 'utf8');
      assert.ok(firstSave.includes('SAVED_BY_CTRL_S'), 'Ctrl+S 未落盘');
      await cdp.send('Input.insertText', { text: '\nSAVED_BY_ENGINE_SWITCH' });
      await selectEngine('native');
      await wait(`!!document.querySelector('.book-reader') && !document.querySelector('.markdown-editor-document')`, '切回原生阅读器未完成');
      const final = fs.readFileSync(sourcePath, 'utf8');
      assert.ok(final.includes('SAVED_BY_CTRL_S') && final.includes('SAVED_BY_ENGINE_SWITCH'), '切引擎未保存最新字符');
      assert.ok(final.includes('Acceptance scratch'), '原内容丢失');
      assert.notEqual((await prefs(itemId)).markdownEngine, 'markdown-editor');
      return { ctrlS: true, engineSwitch: true, bytes: Buffer.byteLength(final), sourcePath };
    });

    await runStep('editor:empty-document-leave-save', async () => {
      await navigate('#/doc/' + emptyId);
      await wait(`!!document.querySelector('.book-reader')`, '空文档未加载');
      await selectEngine('markdown-editor');
      await wait(`!!document.querySelector('.md-editor')`, '空文档编辑器未挂载');
      await read(`document.querySelector('.md-editor').focus()`);
      await cdp.send('Input.insertText', { text: 'EMPTY_DOCUMENT_SAVED_ON_LEAVE' });
      await read(`document.querySelector('.doc-viewer__back').click()`);
      await wait(`!document.querySelector('.doc-viewer')`, '返回按钮未离开文档');
      assert.equal(fs.readFileSync(emptyPath, 'utf8'), 'EMPTY_DOCUMENT_SAVED_ON_LEAVE');
      return { sourcePath: emptyPath, leaveSave: true };
    });

    await runStep('editor:unavailable-preference-and-menus', async () => {
      await ipc(cdp, 'set_reader_book_prefs', { itemId, prefs: JSON.stringify({ v: 1, markdownEngine: 'markdown-editor' }) });
      await cdp.send('Page.enable');
      capabilityScript = (await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: `Object.defineProperty(globalThis,'EditContext',{configurable:true,value:undefined});` })).identifier;
      await cdp.send('Page.reload', { ignoreCache: true });
      await wait(`!!window.__TAURI_INTERNALS__ && !!document.querySelector('#app') && !!document.querySelector('main, .app-layout, .sidebar')`, '模拟能力后页面未就绪');
      await viewport(1600);
      await navigate('#/doc/' + itemId);
      await wait(`!!document.querySelector('.book-reader')`, '旧偏好没有回退阅读器');
      await waitForBookText();
      const wide = await read(menuSnapshot);
      assert.ok(wide.some(m => m.visible && !m.overflow && m.disabled && m.value === 'native' && m.title));
      await viewport(480);
      await wait(`!!document.querySelector('.doc-viewer__btn svg.lucide-ellipsis') || Array.from(document.querySelectorAll('.doc-viewer__btn')).some(b=>/更多|More/.test(b.title)&&b.getClientRects().length)`, '溢出按钮未显示');
      await read(`Array.from(document.querySelectorAll('.doc-viewer__btn')).find(b=>/更多|More/.test(b.title)&&b.getClientRects().length).click()`);
      await wait(`!!document.querySelector('select.doc-more__engine-select')`, '溢出引擎菜单未显示');
      const narrow = await read(menuSnapshot);
      assert.ok(narrow.some(m => m.visible && m.overflow && m.disabled && m.value === 'native'));
      assert.equal((await prefs(itemId)).markdownEngine, 'markdown-editor');
      return { controlledCapabilitySimulation: true, wide, narrow, preferencePreserved: true };
    });

    await runStep('editor:unavailable-deeplink', async () => {
      await viewport(1600);
      await navigate('#/doc/' + emptyId);
      await wait(`!!document.querySelector('.book-reader')`, '中间路由未挂载');
      await navigate('#/doc/' + itemId + '?engine=markdown-editor');
      await wait(`!!document.querySelector('.book-reader') && !document.querySelector('.markdown-editor-document')`, '深链没有回退阅读器');
      await waitForBookText();
      assert.equal((await prefs(itemId)).markdownEngine, 'markdown-editor');
      return { controlledCapabilitySimulation: true, hash: await read('location.hash'), preferencePreserved: true, renderedText: await read(bookText) };
    });
    await runStep('editor:visible-fallback', async () => {
      await read(`Promise.all(document.querySelector('foliate-view').renderer.getContents().map(c=>c.doc.fonts.ready)).then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))`);
      const layout = await read(`document.querySelector('foliate-view').renderer.getContents().map(c=>({text:c.doc.body.innerText,body:c.doc.body.getBoundingClientRect().toJSON(),frame:c.doc.defaultView.frameElement?.getBoundingClientRect().toJSON(),color:c.doc.defaultView.getComputedStyle(c.doc.body).color,frameStyle:c.doc.defaultView.frameElement?.getAttribute('style'),sheets:Array.from(c.doc.styleSheets).map(s=>({href:s.href,rules:s.cssRules.length}))}))`);
      fs.writeFileSync(path.join(ctx.root, 'editor-render-layout.json'), JSON.stringify(layout, null, 2));
      await wait(bookVisible, '阅读器正文仍在视口之外');
      return { layout };
    });
    const shot = await cdp.send('Page.captureScreenshot', { format: 'png', fromSurface: true });
    fs.writeFileSync(path.join(ctx.root, 'editor-final.png'), Buffer.from(shot.data, 'base64'));
  } finally {
    if (capabilityScript) await cdp.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: capabilityScript });
    await cdp.send('Emulation.clearDeviceMetricsOverride');
    // 重载还原能力模拟；后续复制测试从产品首页重新建立会话内历史。
    await read(`location.hash='#/'`).catch(() => {});
    await cdp.send('Page.reload', { ignoreCache: true });
    // Page.reload 的回包只说明开始导航，必须等新文档才能请求后续 IPC/退出。
    await wait(`document.readyState==='complete' && typeof globalThis.EditContext==='function' && !!window.__TAURI_INTERNALS__ && !!document.querySelector('.sidebar')`, '能力还原后的首页未就绪');
    await ipc(cdp, 'get_config_status', null);
  }
}
