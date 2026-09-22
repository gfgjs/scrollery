// 无发行模型时的安装版负向验收；由隔离驱动调用，不能代替成功推理验收。
// 用法：isolated-app-smoke.mjs --stage=enhance --install --verify-user-library-untouched
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';

// 独立验证安装版 worker 在加载真实模型之前的缺失文件路径，并确认随包 ORT 能初始化。
async function rejectMissingModel(worker, root, api) {
  const workDir = path.join(root, 'enhance-worker-negative');
  fs.mkdirSync(workDir, { recursive: true });
  const absent = path.join(workDir, 'scunet-fp32.onnx');
  if (fs.existsSync(absent)) throw new Error('缺失模型夹具路径已有文件:' + absent);
  const env = { ...process.env };
  delete env.ORT_DYLIB_PATH;
  return new Promise((resolve, reject) => {
    const child = spawn(worker, [], { windowsHide: true, env, stdio: ['pipe', 'pipe', 'pipe'] });
    let bytes = Buffer.alloc(0);
    let outcome;
    let failure;
    let stderr = '';
    const stages = [];
    const timer = setTimeout(() => finish(new Error('安装版 worker 模型缺失验证超时')), 30000);
    function finish(error, result) {
      if (failure || outcome) return;
      failure = error;
      outcome = result;
      clearTimeout(timer);
      child.kill();
    }
    child.once('error', (error) => { clearTimeout(timer); reject(error); });
    child.once('close', () => {
      clearTimeout(timer);
      if (failure) reject(failure);
      else if (outcome) resolve(outcome);
      else reject(new Error('worker 提前退出:' + stderr.slice(-500)));
    });
    child.stderr.on('data', (data) => { stderr += data.toString(); });
    child.stdin.on('error', (error) => finish(error));
    child.stdout.on('data', (data) => {
      bytes = Buffer.concat([bytes, data]);
      try {
        let packet;
        while ((packet = api.exotTakeFrame(bytes))) {
          bytes = packet.rest;
          const frame = packet.frame;
          if (frame.type === 2) {
            if (frame.json.worker_id !== 'enhance-worker') throw new Error('worker 身份不符');
            // len/hash 有意留空：文件应在 canonicalize 阶段即被拒，未提供真实模型元数据。
            child.stdin.write(api.exotFrame(3, 1, {
              op: 'enhance_session_init', session_id: 1, models_root: workDir, work_dir: workDir,
              models: [{ role: 'enhance', model_id: 'scunet', handle: { kind: 'path', value: absent }, len: 0, sha256: '' }],
            }));
          } else if (frame.type === 7) {
            stages.push(frame.json.stage);
          } else if (frame.type === 5) {
            if (frame.json.code !== 'model_load_failed' || !stages.includes('ort_runtime_init:ok')) {
              throw new Error('预期随包 ORT 初始化成功后拒绝缺失模型，实际:' + JSON.stringify({ stages, error: frame.json }));
            }
            finish(null, { code: frame.json.code, stages, modelPathAbsent: !fs.existsSync(absent),
              scope: '安装目录 worker 协议层；不代表就绪清单下的 UI 模型缺失路径' });
          } else if (frame.type === 4) {
            throw new Error('缺失模型却成功初始化');
          }
        }
      } catch (error) { finish(error); }
    });
    child.stdin.write(api.exotFrame(1, 0, { host_version: 'acceptance', protocol_version: 3, max_blob_len: 64 << 20 }));
  });
}

export async function verifyEnhanceGates(ctx, cdp, api) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree, handshakeWorker } = api;
  if (!ctx.installedEvidence?.ok) throw new Error('enhance 阶段必须从已验证的安装目录执行');
  const worker = path.resolve(path.dirname(ctx.exe), 'enhance-worker.exe');
  const root = fs.realpathSync(ctx.root);
  if (!fs.realpathSync(worker).toLowerCase().startsWith(root.toLowerCase() + path.sep)) {
    throw new Error('worker 不在隔离验收根内');
  }
  if (process.env.PICASA_ENHANCE_WORKER_PATH) throw new Error('验收要求使用随包 worker，不能设置覆盖路径');

  await runStep('enhance:installed-worker-handshake', async () => {
    const result = await handshakeWorker(worker, 'enhance-worker', 30000);
    if (!result.capabilities.includes('enhance')) throw new Error('worker 未声明 enhance 能力');
    return { path: worker, ...result };
  });
  await runStep('enhance:installed-worker-model-missing', () => rejectMissingModel(worker, root, api));

  const scanRoot = await ipc(cdp, 'add_scan_root', { path: ctx.mediaDir, alias: 'Enhance Fixture' });
  await runStep('enhance:scan-fixtures', async () => {
    const channel = await openChannel(cdp);
    const done = waitForChannel(cdp, (ev) => ev?.type === 'completed' || ev?.type === 'error', ctx.timeoutMs, 'enhance scan');
    await ipc(cdp, 'start_scan', { rootId: scanRoot.id, runId: 'enhance-acceptance', onProgress: channel,
      groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false }, ctx.timeoutMs);
    const result = await done;
    if (result.hit.type === 'error') throw new Error(JSON.stringify(result.hit));
    return result.hit;
  });
  const dirs = await walkDirectoryTree(cdp, scanRoot.id, ctx.timeoutMs);
  const items = await ipc(cdp, 'list_directory_files', { directoryId: dirs[0].id, limit: 100, offset: 0, categories: null });
  const item = items.find((entry) => ctx.fixtures.items.some((fixture) => fixture.name === entry.fileName));
  if (!item) throw new Error('缺少入库图片夹具');
  const params = { steps: [{ task: 'denoise', model_id: 'scunet' }], outputFormat: 'png' };
  const channel = await openChannel(cdp);
  const attempts = [
    ['enhance_start', { itemIds: [item.id], params }],
    ['enhance_preview', { itemId: item.id, point: null, params }],
    ['download_enhance_model', { modelId: 'scunet', onProgress: channel }],
  ];
  async function assertRejected(code) {
    const evidence = [];
    for (const [command, args] of attempts) {
      const result = await evaluate(cdp, `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)}).then(() => ({accepted:true}), error => ({error}))`);
      if (result.accepted || result.error?.code !== code) {
        throw new Error(command + ' 预期 ' + code + '，实际 ' + JSON.stringify(result));
      }
      evidence.push({ command, code: result.error.code });
    }
    const queue = await ipc(cdp, 'get_enhance_queue', null);
    if (queue.length) throw new Error('拒绝操作仍产生了增强任务');
    return { evidence, queueLength: queue.length };
  }
  async function assertSettings(label) {
    await evaluate(cdp, `new Promise((resolve, reject) => {
      const deadline = Date.now() + 10000;
      const poll = () => {
        if (!document.querySelector('.onboarding-title, .guide-body')) return resolve(true);
        if (Date.now() > deadline) return reject(new Error('首次使用引导未关闭'));
        const skip = [...document.querySelectorAll('button')].find(button => ['跳过', 'Skip'].includes(button.textContent.trim()));
        if (!skip) return reject(new Error('找不到首次使用引导的跳过按钮'));
        skip.click();
        setTimeout(poll, 100);
      };
      poll();
    })`);
    await evaluate(cdp, `location.hash = '#/'`);
    await evaluate(cdp, `new Promise((resolve, reject) => {
      const deadline = Date.now() + 10000;
      const poll = () => !document.querySelector('.enh-model__dl-btn') ? resolve(true)
        : Date.now() > deadline ? reject(new Error('设置页未退出')) : setTimeout(poll, 100);
      poll();
    })`);
    await evaluate(cdp, `location.hash = '#/settings/ai'`);
    const evidence = await evaluate(cdp, `new Promise((resolve, reject) => {
      const deadline = Date.now() + 10000;
      const poll = () => {
        const buttons = [...document.querySelectorAll('.enh-model__dl-btn')];
        const messages = [...document.querySelectorAll('.enh-models__unready')].map(node => node.textContent.trim());
        const workerMessage = messages.some(text => /增强组件缺失|enhancement component is missing/i.test(text));
        if (buttons.length === 5 && (${JSON.stringify(label)} !== 'worker-missing' || workerMessage)) return resolve({ buttons: buttons.length,
          disabled: buttons.every(button => button.disabled),
          storeLink: Boolean(document.querySelector('.enh-models__auth-link')),
          messages });
        if (Date.now() > deadline) return reject(new Error('增强设置未加载五档模型'));
        setTimeout(poll, 100);
      }; poll();
    })`);
    if (!evidence.disabled || evidence.storeLink || evidence.messages.length === 0) {
      throw new Error('设置页仍引导不可完成流程:' + JSON.stringify(evidence));
    }
    await evaluate(cdp, `document.querySelector('.enh-models__unready')?.scrollIntoView({block:'center'})`);
    const shot = await cdp.send('Page.captureScreenshot', { format: 'png' });
    const screenshot = path.join(ctx.root, 'enhance-' + label + '.png');
    fs.writeFileSync(screenshot, Buffer.from(shot.data, 'base64'));
    return { ...evidence, screenshot };
  }
  await runStep('enhance:manifest-unready', async () => {
    const status = await ipc(cdp, 'enhance_status', null);
    if (!status.workerReady || status.models.length !== 5 || status.models.some((model) =>
      model.readiness !== 'manifestUnready' || model.canDownload || model.installed)) {
      throw new Error('发行资产缺失时状态不正确:' + JSON.stringify(status));
    }
    return { status, ...await assertRejected('enhance_manifest_unready') };
  });
  await runStep('enhance:manifest-unready-settings', () => assertSettings('manifest-unready'));
  const hidden = worker + '.acceptance-missing';
  if (fs.existsSync(hidden)) throw new Error('缺失路径演练存在未恢复文件:' + hidden);
  // 仅暂移已验证安装目录中的单个 worker，finally 恢复；不触碰用户的实际安装。
  fs.renameSync(worker, hidden);
  try {
    await runStep('enhance:worker-missing', async () => {
      const status = await ipc(cdp, 'enhance_status', null);
      if (status.workerReady || status.models.some((model) => model.readiness !== 'workerMissing' || model.canDownload)) {
        throw new Error('worker 缺失状态不正确:' + JSON.stringify(status));
      }
      return { status, ...await assertRejected('enhance_worker_missing') };
    });
    await runStep('enhance:worker-missing-settings', () => assertSettings('worker-missing'));
  } finally {
    fs.renameSync(hidden, worker);
  }
  await runStep('enhance:worker-restored', async () => {
    const status = await ipc(cdp, 'enhance_status', null);
    if (!status.workerReady) throw new Error('worker 恢复后状态未更新');
    return { workerReady: true };
  });
  await runStep('enhance:originals-unchanged', () => {
    const evidence = ctx.fixtures.allMediaItems.map((fixture) => {
      const hash = createHash('sha256').update(fs.readFileSync(fixture.file)).digest('hex');
      if (hash !== fixture.sha256) throw new Error('原图发生变化:' + fixture.file);
      return { path: fixture.file, before: fixture.sha256, after: hash };
    });
    return { scope: '仅本次拒绝路径；不代表成功增强已验证', files: evidence };
  });
  await runStep('enhance:done', () => ({
    successfulInferenceVerified: false,
    previewVerified: false,
    modelMissingWithReadyManifestVerified: false,
    blocker: '获准分发的真实 ONNX、许可材料与固定托管版本尚未准备好',
  }));
}
