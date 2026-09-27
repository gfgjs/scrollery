import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 只采样本次宿主及其原生 worker；PID 与启动时间共同防止误认复用进程。 */
export async function startOwnedResourceSampler(ctx, ownerPid, ownedChildren) {
  const output = path.join(ctx.logDir, 'owned-resources.jsonl');
  const stderr = path.join(ctx.logDir, 'owned-resources-stderr.txt');
  const stopFile = path.join(ctx.logDir, 'owned-resources.stop');
  fs.rmSync(stopFile, { force: true });
  const script = `
$ErrorActionPreference = 'Stop'
$ownerId = [int]$env:SCROLLERY_SAMPLE_OWNER
$hostPath = $env:SCROLLERY_SAMPLE_EXE
$nativePath = Join-Path (Split-Path -Parent $hostPath) 'native-thumbnail-worker.exe'
$hostProcess = Get-Process -Id $ownerId
if ($hostProcess.Path -ne $hostPath) { throw 'Owned host path mismatch' }
$hostStart = $hostProcess.StartTime.Ticks
$workers = @()
$nextDiscovery = [DateTime]::MinValue
Write-Output '{"kind":"ready"}'
while (!(Test-Path -LiteralPath $env:SCROLLERY_SAMPLE_STOP)) {
  $hostProcess = Get-Process -Id $ownerId -ErrorAction SilentlyContinue
  if (!$hostProcess -or $hostProcess.StartTime.Ticks -ne $hostStart) { break }
  if ([DateTime]::UtcNow -ge $nextDiscovery) {
    $workers = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $ownerId AND Name = 'native-thumbnail-worker.exe'" | Where-Object { $_.ExecutablePath -eq $nativePath } | ForEach-Object {
      $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
      if ($p) { @{ Id = $p.Id; Start = $p.StartTime.Ticks } }
    })
    $nextDiscovery = [DateTime]::UtcNow.AddMilliseconds(500)
  }
  $observed = @(@{ Process = $hostProcess; Role = 'host' })
  foreach ($worker in $workers) {
    $p = Get-Process -Id $worker.Id -ErrorAction SilentlyContinue
    if ($p -and $p.StartTime.Ticks -eq $worker.Start -and $p.Path -eq $nativePath) {
      $observed += @{ Process = $p; Role = 'native' }
    }
  }
  $rows = @($observed | ForEach-Object {
    $p = $_.Process
    @{ pid = $p.Id; role = $_.Role; privateBytes = $p.PrivateMemorySize64; workingSetBytes = $p.WorkingSet64; cpuMs = $p.TotalProcessorTime.TotalMilliseconds; threads = $p.Threads.Count }
  })
  @{ timeMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); processes = $rows } | ConvertTo-Json -Compress -Depth 4
  Start-Sleep -Milliseconds 100
}
`;
  const outFd = fs.openSync(output, 'w');
  const errFd = fs.openSync(stderr, 'w');
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    windowsHide: true,
    env: { ...process.env, SCROLLERY_SAMPLE_OWNER: String(ownerPid), SCROLLERY_SAMPLE_EXE: ctx.exe, SCROLLERY_SAMPLE_STOP: stopFile },
    stdio: ['ignore', outFd, errFd],
  });
  fs.closeSync(outFd);
  fs.closeSync(errFd);
  ownedChildren.push(child);
  let spawnError;
  child.on('error', (error) => { spawnError = error; });
  const start = Date.now();
  while (!fs.readFileSync(output, 'utf8').includes('"kind":"ready"')) {
    if (spawnError) throw spawnError;
    if (child.exitCode !== null || Date.now() - start > 10000) {
      fs.writeFileSync(stopFile, 'stop');
      throw new Error('Resource sampler did not start: ' + fs.readFileSync(stderr, 'utf8'));
    }
    await pause(100);
  }
  return {
    async stop() {
      fs.writeFileSync(stopFile, 'stop');
      const deadline = Date.now() + 10000;
      while (child.exitCode === null && Date.now() < deadline) await pause(100);
      if (child.exitCode !== 0) throw new Error('Resource sampler failed: ' + fs.readFileSync(stderr, 'utf8'));
      return output;
    },
  };
}
