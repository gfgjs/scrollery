import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';

// 仅用于已通过身份校验的验收宿主；不会按进程名附加用户的其它应用。
export async function attachNativeDebuggers(ctx, parentPid, ownedChildren) {
  const query = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command',
    `Get-CimInstance Win32_Process -Filter "ParentProcessId = ${parentPid} AND Name = 'native-thumbnail-worker.exe'" | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress`],
  { encoding: 'utf8', windowsHide: true });
  if (query.status !== 0 || !query.stdout.trim()) throw new Error('Owned native worker discovery failed');
  const found = JSON.parse(query.stdout.replace(/^\uFEFF/, ''));
  const workers = Array.isArray(found) ? found : [found];
  const expected = path.resolve(path.dirname(ctx.exe), 'native-thumbnail-worker.exe').toLowerCase();
  fs.mkdirSync(ctx.logDir, { recursive: true });
  const attached = [];
  for (const worker of workers) {
    if (worker.ParentProcessId !== parentPid || path.resolve(worker.ExecutablePath).toLowerCase() !== expected) {
      throw new Error('Debugger target is not the owned acceptance worker');
    }
    const base = path.join(ctx.logDir, 'native-debug-' + worker.ProcessId).replaceAll('\\', '/');
    // WinDbg 的脚本重定向语法使用无空白路径，本次隔离根目录符合该约束。
    if (/\s/.test(base)) throw new Error('Native debugger requires an acceptance root without spaces');
    const exceptionScript = base + '-exception.dbg';
    fs.writeFileSync(exceptionScript, '.echo THUMB_STACK_OVERFLOW\n.dump /ma "' + base + '.dmp"\n.ecxr\nkv\n!teb\nsxd sov\ngn\n');
    const startupScript = base + '.dbg';
    fs.writeFileSync(startupScript, 'sxe -c "$$><' + exceptionScript + '" sov\n.echo THUMB_DEBUGGER_READY\ng\n');
    const log = base + '.log';
    const debuggerProcess = spawn(ctx.nativeDebugger, ['-p', String(worker.ProcessId), '-hd', '-G', '-logo', log, '-cf', startupScript],
      { windowsHide: true, stdio: 'ignore' });
    ownedChildren.push(debuggerProcess);
    let launchError;
    debuggerProcess.once('error', error => { launchError = error; });
    const deadline = Date.now() + 15000;
    while (!fs.existsSync(log) || !fs.readFileSync(log, 'utf8').includes('THUMB_DEBUGGER_READY')) {
      if (launchError || debuggerProcess.exitCode !== null || Date.now() >= deadline) throw new Error('Native debugger attach failed: ' + (launchError?.message || log));
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    attached.push({ ...worker, debuggerPid: debuggerProcess.pid, log, dump: base + '.dmp' });
  }
  return { scope: 'diagnostic only; debugger changes timing', attached };
}
