import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const MiB = 1024 * 1024;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const row = (id) => `.tree-item[data-dir-id="${id}"]`;
const comparablePath = (value) => path.toNamespacedPath(path.resolve(value)).toLowerCase();

async function manifest(directory) {
  const entries = [];
  async function visit(relative) {
    for (const entry of await fs.readdir(path.join(directory, relative), { withFileTypes: true })) {
      const name = path.posix.join(relative, entry.name);
      assert(!entry.isSymbolicLink(), `夹具中出现链接：${name}`);
      if (entry.isDirectory()) {
        entries.push({ path: name, directory: true });
        await visit(name);
      } else {
        assert(entry.isFile(), `夹具中出现特殊文件：${name}`);
        const bytes = await fs.readFile(path.join(directory, name));
        entries.push({ path: name, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
      }
    }
  }
  await visit('');
  return entries.sort((a, b) => a.path.localeCompare(b.path));
}

// 只读生产 Pinia；不依赖 DEV 的组件 setupState，也不改写历史记录。
function readHistory() {
  const pinia = document.querySelector('#app')?.__vue_app__?.config.globalProperties.$pinia;
  const history = pinia?._s.get('history');
  if (!history) throw new Error('生产 Pinia history 尚未就绪');
  return JSON.parse(JSON.stringify({
    undo: history.undoStack, redo: history.redoStack,
    canUndo: history.canUndo, canRedo: history.canRedo,
    busy: history.busy, copyProgress: history.copyProgress,
    scanRunning: pinia._s.get('scan')?.isAnyScanRunning ?? false,
  }));
}

// Tauri invoke 是只读属性；旁路观察真实 fetch 与调试 callbacks Map，不改变 IPC 参数或回执。
function installProbe() {
  if (window.__copyInteractionProbe) throw new Error('copy probe 已存在');
  const bridge = window.__TAURI_INTERNALS__;
  const original = window.fetch;
  const probe = { calls: [], ui: [], arm: null, channelRestores: [] };
  const clone = (value) => value === undefined ? null : JSON.parse(JSON.stringify(value));
  const errorData = (error) => ({ code: error?.code ?? null, message: error?.message ?? String(error) });
  function wrapped(input, init, ...rest) {
    const url = typeof input === 'string' ? input : input.url;
    const command = url?.startsWith('http://ipc.localhost/') ? decodeURIComponent(new URL(url).pathname.slice(1)) : '';
    if (!['copy_directory', 'cancel_directory_copy', 'delete_directory_to_trash'].includes(command)) {
      return original.call(this, input, init, ...rest);
    }
    const args = JSON.parse(typeof init.body === 'string' ? init.body : new TextDecoder().decode(init.body));
    const call = { command, args: clone(args), started: Date.now(), events: [], outcome: 'pending' };
    probe.calls.push(call);
    const channelId = Number(args?.onProgress?.replace('__CHANNEL__:', ''));
    if (command === 'copy_directory' && bridge.callbacks.has(channelId)) {
      const handler = bridge.callbacks.get(channelId);
      const observed = function (raw) {
        if (!raw?.end) call.events.push({ at: Date.now(), message: clone(raw?.message ?? raw) });
        return handler.call(this, raw);
      };
      bridge.callbacks.set(channelId, observed);
      probe.channelRestores.push(() => { if (bridge.callbacks.get(channelId) === observed) bridge.callbacks.set(channelId, handler); });
    }
    return original.call(this, input, init, ...rest).then((response) => {
      const copy = response.clone();
      const content = (copy.headers.get('content-type') || '').includes('application/json') ? copy.json() : copy.text();
      content.then((result) => {
        call.outcome = response.headers.get('Tauri-Response') === 'ok' ? 'fulfilled' : 'rejected';
        if (call.outcome === 'fulfilled') call.result = clone(result);
        else call.error = errorData(result);
        call.ended = Date.now();
      }).catch((error) => { call.outcome = 'observation-error'; call.error = errorData(error); });
      return response;
    }, error => {
      call.outcome = 'transport-error'; call.error = errorData(error); call.ended = Date.now();
      throw error;
    });
  }
  const observer = new MutationObserver(() => {
    const arm = probe.arm;
    if (!arm || arm.clicked) return;
    const indicator = [...document.querySelectorAll('.statusbar__file-job')]
      .find((element) => element.title === arm.name);
    if (!indicator) return;
    const button = indicator.querySelector('button.statusbar__file-job-cancel');
    const state = { at: Date.now(), name: arm.name, text: indicator.textContent.trim(), disabled: button?.disabled ?? true };
    if (!arm.states.length || arm.states.at(-1).disabled !== state.disabled || arm.states.at(-1).text !== state.text) {
      arm.states.push(state);
    }
    if (!button || button.disabled || !button.getClientRects().length) return;
    arm.clicked = true;
    // 同一个真实按钮触发 Vue 的 @click；保留 isTrusted=false，明确区别于 CDP 鼠标事件。
    button.addEventListener('click', (event) => {
      probe.ui.push({ at: Date.now(), kind: 'MutationObserver:HTMLElement.click', name: arm.name,
        isTrusted: event.isTrusted, selector: '.statusbar__file-job[title] button.statusbar__file-job-cancel' });
    }, { once: true });
    button.click();
  });
  observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  probe.restore = () => {
    observer.disconnect();
    probe.channelRestores.forEach((restore) => restore());
    if (window.fetch === wrapped) window.fetch = original;
    delete window.__copyInteractionProbe;
  };
  window.fetch = wrapped;
  window.__copyInteractionProbe = probe;
  return true;
}

/**
 * 在已通过隔离身份校验的 Acceptance 页面验证目录复制交互。
 * @param {object} ctx - 主驱动上下文；root 为本轮新建隔离目录，timeoutMs 为等待上限。
 * @param {object} cdp - 主驱动 CDP 连接，提供 send(method, params)。
 * @param {object} helpers - runStep、ipc、evaluate、openChannel、waitForChannel、walkDirectoryTree。
 * @returns {Promise<object>} 步骤证据、夹具位置与磁盘证据文件位置；未验证或失败时抛错。
 */
export async function verifyCopyInteractions(ctx, cdp, helpers) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree } = helpers;
  const timeout = Math.min(ctx.timeoutMs || 30000, 15000);
  const root = await fs.realpath(ctx.root);
  const own = await fs.mkdtemp(path.join(root, 'copy-'));
  const sourceRoot = path.join(own, 'fixture');
  const small = path.join(sourceRoot, 'copy-small');
  const large = path.join(sourceRoot, 'copy-cancel');
  const destination = path.join(sourceRoot, 'copy-target');
  const targetSmall = path.join(destination, 'copy-small');
  const targetLarge = path.join(destination, 'copy-cancel');
  const report = { status: 'running', fixtureDirectory: sourceRoot, evidenceDirectory: own,
    identity: ctx.identity ?? null, steps: {}, cancellationAttempts: 0, unverified: [] };
  let rootId, ids, smallExpected, largeExpected, probeInstalled = false;
  const page = (fn, ...args) => evaluate(cdp, `(${fn.toString()})(...${JSON.stringify(args)})`, timeout);
  const history = () => page(readHistory);
  async function until(label, fn) {
    const deadline = Date.now() + timeout;
    do {
      const value = await fn();
      if (value) return value;
      await sleep(80);
    } while (Date.now() < deadline);
    throw new Error(`等待超时：${label}`);
  }
  const idle = () => until('历史操作收尾', async () => {
    const value = await history();
    return !value.busy && !value.copyProgress && !value.scanRunning ? value : null;
  });
  async function point(selector) {
    return page((selector) => {
      const element = document.querySelector(selector);
      if (!element || element.disabled) return null;
      element.scrollIntoView({ block: 'nearest' });
      const rect = element.getBoundingClientRect();
      const x = rect.x + rect.width / 2, y = rect.y + rect.height / 2;
      const hit = document.elementFromPoint(x, y);
      if (!rect.width || !rect.height || !hit || !element.contains(hit)) return null;
      return { x, y, selector };
    }, selector);
  }
  async function click(selector) {
    const p = await until(`可点击 ${selector}`, () => point(selector));
    await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: p.x, y: p.y, button: 'left', buttons: 1, clickCount: 1 });
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: p.x, y: p.y, button: 'left', buttons: 0, clickCount: 1 });
    return p;
  }
  async function expand(id) {
    await until(`目录行 ${id}`, () => point(row(id)));
    const collapsed = await page((selector) => !!document.querySelector(`${selector} .tree-chevron:not(.expanded)`), row(id));
    if (collapsed) await click(`${row(id)} .tree-arrow`);
  }
  async function home() {
    await until('首页与引导就绪', async () => {
      await page(() => { if (location.hash !== '#/') location.hash = '#/'; });
      const skip = await page(() => {
        const dialog = [...document.querySelectorAll('.dialog-content')]
          .find((el) => el.getClientRects().length && el.querySelector('.onboarding-body, .guide-body'));
        const button = dialog && [...dialog.querySelectorAll('button')].find((el) => /^(跳过|跳過|Skip)(\s|$)/i.test(el.textContent.trim()));
        if (!button) return null;
        button.setAttribute('data-copy-helper-skip', 'true');
        return '[data-copy-helper-skip="true"]';
      });
      if (skip) { await click(skip); return false; }
      return page(() => !!document.querySelector('#app')?.__vue_app__?.config.globalProperties.$pinia?._s.get('history')
        && !!document.querySelector('.toolbar__sidebar-toggle'));
    });
    if (!await point('.show-all-btn')) {
      const hidden = await page(() => !document.querySelector('.sidebar__scroll-area')?.getClientRects().length);
      if (hidden) await click('.toolbar__sidebar-toggle');
    }
    await click('.show-all-btn');
    const collapsed = await page(() => !document.querySelector('#acc-body-folders')?.getClientRects().length);
    if (collapsed) {
      const selector = await page(() => {
        const button = document.querySelector('.show-all-btn')?.closest('.acc-header')?.querySelector('.acc-header__toggle');
        if (!button) throw new Error('缺少目录区折叠按钮');
        button.setAttribute('data-copy-helper-expand', 'true');
        return '[data-copy-helper-expand="true"]';
      });
      await click(selector);
    }
    await idle();
  }
  async function scan() {
    const channel = await openChannel(cdp);
    const [_, progress] = await Promise.all([
      ipc(cdp, 'start_scan', { rootId, runId: `copy-helper-${Date.now()}`, onProgress: channel,
        groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false }, timeout),
      waitForChannel(cdp, (event) => ['completed', 'error'].includes(event?.type), timeout, 'copy fixture scan'),
    ]);
    assert.equal(progress.hit.type, 'completed', JSON.stringify(progress.hit));
    return progress;
  }
  const probe = () => page(() => {
    const p = window.__copyInteractionProbe;
    return { calls: p.calls, ui: p.ui, arm: p.arm };
  });
  async function mark(cancelName = null) {
    return page((name) => {
      const p = window.__copyInteractionProbe;
      p.arm = name ? { name, clicked: false, states: [] } : null;
      return p.calls.length;
    }, cancelName);
  }
  async function finishCopy(index) {
    const call = await until('真实 copy_directory 终态', async () => {
      const p = await probe();
      return p.calls.slice(index).find((call) => call.command === 'copy_directory' && call.outcome !== 'pending');
    });
    await idle();
    await until('状态栏复制指示器消失', () => page((names) =>
      ![...document.querySelectorAll('.statusbar__file-job')].some((el) => names.includes(el.title)), ['copy-small', 'copy-cancel']));
    return call;
  }
  async function drag(sourceId) {
    await expand(ids.root);
    const source = await until('拖动源', () => point(`${row(sourceId)} .tree-label`));
    const target = await until('拖动目标', () => point(`${row(ids.target)} .tree-label`));
    // 坐标获取可能滚动虚拟树；起手前重新确认两行仍同时可命中。
    const actualSource = await point(`${row(sourceId)} .tree-label`);
    const actualTarget = await point(`${row(ids.target)} .tree-label`);
    assert(actualSource && actualTarget && actualSource.y === source.y && actualTarget.y === target.y,
      '源与目标未同时进入目录树视口');
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Control', code: 'ControlLeft', modifiers: 2, windowsVirtualKeyCode: 17 });
    let released = false;
    try {
      await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: source.x, y: source.y, button: 'left', buttons: 1, clickCount: 1, modifiers: 2 });
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: source.x + 12, y: source.y + 2, buttons: 1, modifiers: 2 });
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: target.x, y: target.y, buttons: 1, modifiers: 2 });
      await until('Ctrl 拖动目标高亮', () => page((selector) => document.querySelector(selector)?.classList.contains('drag-over'), row(ids.target)));
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: target.x, y: target.y, button: 'left', buttons: 0, clickCount: 1, modifiers: 2 });
      released = true;
    } finally {
      // 失败时在原行释放，避免留下按键或意外落到其它目录。
      if (!released) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: source.x, y: source.y, buttons: 1, modifiers: 2 });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: source.x, y: source.y, button: 'left', buttons: 0, modifiers: 2 });
      }
      await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Control', code: 'ControlLeft', modifiers: 0, windowsVirtualKeyCode: 17 });
    }
    return { kind: 'CDP:Ctrl+pointer-drag', source, target, sourceId, targetId: ids.target };
  }
  async function shortcut(kind) {
    const before = await idle();
    assert(before[kind === 'undo' ? 'canUndo' : 'canRedo'], `${kind} 当前不可用`);
    // 模糊掉输入焦点，仍由产品的全局 grid 命令处理快捷键。
    await page(() => document.activeElement?.blur());
    const key = kind === 'undo' ? 'z' : 'y', code = `Key${key.toUpperCase()}`, keyCode = key.toUpperCase().charCodeAt(0);
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Control', code: 'ControlLeft', windowsVirtualKeyCode: 17, modifiers: 2 });
    try {
      await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: keyCode, modifiers: 2 });
      await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: keyCode, modifiers: 2 });
    } finally {
      await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Control', code: 'ControlLeft', windowsVirtualKeyCode: 17, modifiers: 0 });
    }
    return { kind: `CDP:Ctrl+${key.toUpperCase()}`, before };
  }
  async function undoCopy(expectedPath) {
    const before = await idle();
    assert.equal(before.undo.at(-1)?.createdAbsPath && comparablePath(before.undo.at(-1).createdAbsPath), comparablePath(expectedPath), '撤销栈顶不是本次副本');
    const index = await mark();
    const ui = await shortcut('undo');
    await until('真实回收 IPC 完成', async () => (await probe()).calls.slice(index)
      .some((call) => call.command === 'delete_directory_to_trash' && call.outcome !== 'pending'));
    const after = await idle();
    assert.deepEqual(after.undo, before.undo.slice(0, -1));
    assert.deepEqual(after.redo, [...before.redo, before.undo.at(-1)]);
    await assert.rejects(fs.stat(expectedPath), { code: 'ENOENT' });
    return { ui, before, after, calls: (await probe()).calls.slice(index) };
  }
  async function published(call, source, target, expected) {
    assert.equal(call.outcome, 'fulfilled', JSON.stringify(call));
    const finalProgress = call.events.at(-1)?.message;
    const files = expected.filter(entry => !entry.directory);
    assert(finalProgress?.generation > 0 && finalProgress.publishing, '缺少真实 Channel 发布终态');
    assert.equal(finalProgress.copiedFiles, files.length);
    assert.equal(finalProgress.copiedBytes, files.reduce((sum, entry) => sum + entry.bytes, 0));
    assert.equal(call.args.sourceDirId, source === small ? ids.small : ids.large);
    assert.equal(call.args.targetDirId, ids.target);
    assert.equal(comparablePath(call.result.createdAbsPath), comparablePath(target));
    assert.deepEqual(await manifest(source), expected, '源目录内容变化');
    assert.deepEqual(await manifest(target), expected, '目标目录哈希或目录清单不符');
    const dirs = await walkDirectoryTree(cdp, rootId, timeout);
    const relative = path.relative(sourceRoot, target).replaceAll('\\', '/');
    const node = dirs.find((entry) => entry.relPath.replaceAll('\\', '/') === relative);
    assert(node && typeof node.id === 'number', '复制副本未入库');
    await expand(ids.target);
    await until('目录树出现副本', () => point(row(node.id)));
    return { source, target, manifest: expected, databaseDirectory: node.id, history: await idle() };
  }
  async function cancelAttempt(kind) {
    const before = await idle();
    const beforeTarget = await manifest(destination);
    const isRedo = kind === 'redo';
    const source = isRedo ? small : large;
    const expected = isRedo ? smallExpected : largeExpected;
    const target = isRedo ? targetSmall : targetLarge;
    const index = await mark(path.basename(source));
    report.cancellationAttempts++;
    const ui = isRedo ? await shortcut('redo') : await drag(ids.large);
    const call = await finishCopy(index);
    const p = await probe();
    const evidence = { ui, call, calls: p.calls.slice(index), cancelUI: p.ui.filter((event) => event.at >= call.started), arm: p.arm, before, after: await idle() };
    await mark();
    assert.deepEqual(await manifest(source), expected, '取消后源目录变化');
    if (call.outcome === 'fulfilled') {
      evidence.status = 'unverified-window-missed';
      evidence.published = await published(call, source, target, expected);
      return evidence;
    }
    assert.equal(call.error?.code, 'Cancelled', JSON.stringify(evidence));
    assert.equal(call.args.sourceDirId, isRedo ? ids.small : ids.large);
    assert.equal(call.args.targetDirId, ids.target);
    const receipt = evidence.calls.find((item) => item.command === 'cancel_directory_copy');
    assert(evidence.arm?.clicked && evidence.cancelUI.length, '没有实际取消按钮事件');
    assert(receipt && receipt.outcome === 'fulfilled', '取消 IPC 未成功回执');
    assert(call.events.some((event) => event.message.generation === receipt.args.generation), '缺少同代次真实 Channel 进度');
    assert.deepEqual(await manifest(destination), beforeTarget, '取消后目标或暂存残留未清理');
    assert.deepEqual(evidence.after.undo, before.undo, '取消改变了撤销记录');
    assert.deepEqual(evidence.after.redo, before.redo, '取消改变了重做记录');
    const dirs = await walkDirectoryTree(cdp, rootId, timeout);
    const targetRelative = path.relative(sourceRoot, target).replaceAll('\\', '/');
    assert(!dirs.some((entry) => entry.relPath.replaceAll('\\', '/') === targetRelative), '取消后副本数据库目录仍残留');
    evidence.status = 'verified';
    evidence.sourceManifest = expected;
    evidence.targetManifest = beforeTarget;
    return evidence;
  }
  async function saveStep(name, evidence) {
    evidence.verified = true;
    report.steps[name] = evidence;
    await fs.writeFile(path.join(own, 'copy-evidence.json'), JSON.stringify(report, null, 2));
    return evidence;
  }
  const required = (value, name) => {
    if (!value?.verified) throw new Error(`${name} 未完成，停止后续依赖场景`);
    return value;
  };
  function missed(label, evidence) {
    report.unverified.push({ scenario: label, reason: '在固定夹具规模内错过取消窗口', evidence });
    const error = new Error(`${label} 未验证：复制已完成，不能作为取消通过`);
    error.code = 'COPY_CANCEL_UNVERIFIED';
    throw error;
  }
  try {
    required(await runStep('copy:prepare-fixture', async () => {
      await home();
      await fs.mkdir(path.join(small, 'nested'), { recursive: true });
      await fs.mkdir(large);
      await fs.mkdir(destination);
      await fs.writeFile(path.join(small, 'index.md'), '# Copy interaction\n');
      await fs.writeFile(path.join(small, 'nested', 'note.txt'), 'Nested copy hash fixture.\n');
      await fs.writeFile(path.join(large, 'index.txt'), 'Cancellation fixture.\n');
      await fs.writeFile(path.join(destination, 'index.txt'), 'Destination anchor.\n');
      await fs.writeFile(path.join(large, 'payload.bin'), Buffer.alloc(12 * MiB, 0x5a));
      // 少量文件带来可观察的进度边界，不用媒体或循环增大负载。
      for (let i = 0; i < 96; i++) await fs.writeFile(path.join(large, `part-${String(i).padStart(3, '0')}.bin`), Buffer.alloc(4096, i));
      const initialManifest = await manifest(sourceRoot);
      const fixtureBytes = initialManifest.reduce((sum, file) => sum + (file.bytes || 0), 0);
      // 最坏同时计入源、已回收副本、重做副本与取消暂存的四份负载，预留 editor 空间。
      assert(fixtureBytes * 4 < 63 * MiB, '复制夹具总占用预算超限');
      report.fixtureBytes = fixtureBytes;
      report.conservativePeakBytes = fixtureBytes * 4;
      smallExpected = await manifest(small);
      largeExpected = await manifest(large);
      const added = await ipc(cdp, 'add_scan_root', { path: sourceRoot, alias: 'Acceptance Copy' });
      assert.equal(typeof added.id, 'number');
      rootId = added.id;
      const scanned = await scan();
      const dirs = await walkDirectoryTree(cdp, rootId, timeout);
      const find = (relative) => {
        const node = dirs.find((entry) => entry.relPath.replaceAll('\\', '/') === relative);
        assert(node && typeof node.id === 'number', `扫描未生成目录：${relative}`);
        return node.id;
      };
      ids = { root: find(''), small: find('copy-small'), large: find('copy-cancel'), target: find('copy-target') };
      await page(async () => {
        const scan = document.querySelector('#app').__vue_app__.config.globalProperties.$pinia._s.get('scan');
        if (!await scan.loadScanRoots()) throw new Error('扫描根刷新未应用');
      });
      await expand(ids.root);
      await page(installProbe);
      probeInstalled = true;
      return saveStep('prepare', { rootId, ids, fixtureBytes, conservativePeakBytes: fixtureBytes * 4,
        scan: scanned.hit, initialManifest, history: await idle() });
    }), 'prepare');

    required(await runStep('copy:ctrl-drag-directory', async () => {
      const before = await idle(), index = await mark();
      const ui = await drag(ids.small);
      const call = await finishCopy(index);
      const disk = await published(call, small, targetSmall, smallExpected);
      assert.equal(disk.history.undo.length, before.undo.length + 1);
      assert.equal(disk.history.undo.at(-1).sourceDirId, ids.small);
      assert.equal(disk.history.undo.at(-1).targetDirId, ids.target);
      assert.deepEqual(disk.history.redo, []);
      return saveStep('copy', { ui, before, call, ...disk });
    }), 'copy');

    required(await runStep('copy:keyboard-undo', async () => {
      const evidence = await undoCopy(targetSmall);
      assert.deepEqual(await manifest(small), smallExpected);
      return saveStep('undo', evidence);
    }), 'undo');

    required(await runStep('copy:keyboard-redo', async () => {
      const index = await mark(), ui = await shortcut('redo');
      const call = await finishCopy(index);
      const disk = await published(call, small, targetSmall, smallExpected);
      assert.deepEqual(disk.history.redo, ui.before.redo.slice(0, -1));
      assert.equal(disk.history.undo.at(-1).sourceDirId, ids.small);
      return saveStep('redo', { ui, call, ...disk });
    }), 'redo');

    required(await runStep('copy:statusbar-cancel-initial', async () => {
      const first = await cancelAttempt('initial');
      const evidence = { attempts: [first] };
      report.steps.cancelInitial = evidence;
      if (first.status !== 'verified') {
        // 唯一一次额外取消机会：真实撤销已完成的本次副本，复用原夹具。
        evidence.retryUndo = await undoCopy(targetLarge);
        const retry = await cancelAttempt('initial');
        evidence.attempts.push(retry);
        if (retry.status !== 'verified') missed('initial-copy-cancel', evidence);
      }
      return saveStep('cancelInitial', evidence);
    }), 'cancel initial');

    required(await runStep('copy:prepare-cancel-redo', async () => {
      const undo = await undoCopy(targetSmall);
      // 复用非媒体负载，避免两个取消源各持有一份大文件。
      for (const name of await fs.readdir(large)) {
        if (name.endsWith('.bin')) await fs.rename(path.join(large, name), path.join(small, name));
      }
      smallExpected = await manifest(small);
      const scanned = await scan();
      return saveStep('prepareCancelRedo', { undo, sourceManifest: smallExpected, scan: scanned.hit, history: await idle() });
    }), 'prepare cancel redo');

    required(await runStep('copy:statusbar-cancel-redo-retains-record', async () => {
      const evidence = await cancelAttempt('redo');
      report.steps.cancelRedo = evidence;
      if (evidence.status !== 'verified') missed('redo-cancel', evidence);
      assert(evidence.after.canRedo, '取消重做后按钮能力未恢复');
      return saveStep('cancelRedo', evidence);
    }), 'cancel redo');

    required(await runStep('copy:redo-after-cancel', async () => {
      const index = await mark(), ui = await shortcut('redo');
      assert.deepEqual(ui.before.redo, report.steps.cancelRedo.before.redo, '重做记录不是取消前的同一记录');
      const call = await finishCopy(index);
      const disk = await published(call, small, targetSmall, smallExpected);
      assert.deepEqual(disk.history.redo, ui.before.redo.slice(0, -1));
      assert.equal(disk.history.undo.at(-1).sourceDirId, ids.small);
      return saveStep('redoAfterCancel', { ui, call, ...disk });
    }), 'redo after cancel');
    report.status = 'verified';
    return report;
  } catch (error) {
    report.status = error.code === 'COPY_CANCEL_UNVERIFIED' ? 'unverified' : 'failed';
    report.error = { code: error.code ?? null, message: error.message };
    throw error;
  } finally {
    // 失败也保留原始回执；库/进程恢复与夹具清理由主驱动承担。
    if (probeInstalled) {
      try { report.probe = await probe(); }
      catch (error) { report.probeReadError = error.message; }
      try { await page(() => window.__copyInteractionProbe?.restore()); }
      catch (error) { report.probeRestoreError = error.message; }
    }
    if (report.status === 'verified' && (report.probeReadError || report.probeRestoreError)) {
      report.status = 'failed';
      await fs.writeFile(path.join(own, 'copy-evidence.json'), JSON.stringify(report, null, 2));
      throw new Error('复制场景完成，但证据读取或 probe 恢复失败；见 copy-evidence.json');
    }
    await fs.writeFile(path.join(own, 'copy-evidence.json'), JSON.stringify(report, null, 2));
  }
}
