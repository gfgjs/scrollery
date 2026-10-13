import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { testCase } from '../lib/test-case.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
// 只加载函数定义，不执行素材枚举、基准采集或任何产品进程。
const setup = String.raw`
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'scripts/bench/thumbnail-baseline.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
foreach ($statement in $ast.EndBlock.Statements) {
    if ($statement -is [System.Management.Automation.Language.FunctionDefinitionAst]) {
        . ([scriptblock]::Create($statement.Extent.Text))
    }
}
function Assert($condition, [string]$message) { if (-not $condition) { throw $message } }
function Record([int]$id, [int]$parent, [string]$path, [int]$second, [long]$bytes) {
    [pscustomobject]@{
        ProcessId = $id; ParentProcessId = $parent; ExecutablePath = $path
        CreationDate = ([datetime]'2026-10-11T00:00:00Z').ToUniversalTime().AddSeconds($second)
        WorkingSetSize = $bytes; PrivatePageCount = $bytes * 2
        PeakWorkingSetSize = 8; PeakPageFileUsage = 16
        HandleCount = 3; ThreadCount = 2; KernelModeTime = 10000000; UserModeTime = 20000000
    }
}
$hostRecord = Record 100 1 'C:\fixture\scrollery.exe' 0 1000
$workerPath = 'C:\fixture\native-thumbnail-worker.exe'
$worker = Record 101 100 $workerPath 1 2000
`;

function check(body) {
  const result = spawnSync('powershell.exe', [
    '-NoLogo', '-NoProfile', '-NonInteractive', '-OutputFormat', 'Text', '-EncodedCommand',
    Buffer.from(setup + body + "\nWrite-Output 'PASS'", 'utf16le').toString('base64'),
  ], { cwd: root, windowsHide: true, encoding: 'utf8', timeout: 30000 });
  assert.equal(result.status, 0, result.error?.message || result.stderr);
  assert.match(result.stdout, /PASS/);
}

testCase('thumbnail:scoped-worker-aggregation', () => check(String.raw`
$records = @($hostRecord, $worker,
    (Record 102 999 $workerPath 1 9000),
    (Record 103 100 'C:\other\native-thumbnail-worker.exe' 1 9000),
    (Record 104 101 $workerPath 1 9000),
    (Record 105 100 $workerPath -1 9000))
$sample = New-ThumbnailMemorySample $records $hostRecord $workerPath
Assert ($sample.status -eq 'running') 'sample status'
Assert ($sample.processes.Count -eq 2) 'scope must include only host and its current sibling worker'
Assert ($sample.totals.workingSetBytes -eq 3000 -and $sample.totals.privateBytes -eq 6000) 'memory totals'
Assert ($sample.totals.workerCount -eq 1 -and $sample.totals.handleCount -eq 6 -and $sample.totals.threadCount -eq 4) 'counts'
Assert ($sample.processes[1].cpuSeconds -eq 3 -and $sample.processes[1].peakWorkingSetBytes -eq 8192 -and $sample.processes[1].peakPrivateBytes -eq 16384) 'CIM metric units'
Assert ($sample.processes[1].startTimeUtc -eq $worker.CreationDate.ToString('o')) 'worker identity in row'
Assert (-not $sample.totals.Contains('peakWorkingSetBytes')) 'independent lifetime peaks must not be summed'
`));

testCase('thumbnail:host-exit-and-pid-reuse', () => check(String.raw`
Assert ((New-ThumbnailMemorySample @($worker) $hostRecord $workerPath).status -eq 'process-exited') 'host exit'
$replacement = Record 100 1 'C:\fixture\scrollery.exe' 2 9000
$sample = New-ThumbnailMemorySample @($replacement, $worker) $hostRecord $workerPath
Assert ($sample.status -eq 'pid-reused' -and $sample.processes.Count -eq 0) 'same executable with reused host PID'
$changedImage = Record 100 1 'C:\other\scrollery.exe' 0 9000
Assert ((New-ThumbnailMemorySample @($changedImage) $hostRecord $workerPath).status -eq 'identity-changed') 'host image mismatch'
`));

testCase('thumbnail:worker-restart-and-unreadable-identity', () => check(String.raw`
$first = New-ThumbnailMemorySample @($hostRecord, $worker) $hostRecord $workerPath
$reused = Record 101 100 'C:\fixture\other.exe' 3 9000
$gap = New-ThumbnailMemorySample @($hostRecord, $reused) $hostRecord $workerPath
Assert ($gap.totals.workerCount -eq 0 -and $gap.totals.workingSetBytes -eq 1000) 'unrelated PID reuse must not retain old worker'
$restarted = Record 101 100 $workerPath 4 4000
$next = New-ThumbnailMemorySample @($hostRecord, $restarted) $hostRecord $workerPath
Assert ($next.totals.workingSetBytes -eq 5000 -and $next.processes[1].startTimeUtc -ne $first.processes[1].startTimeUtc) 'restarted worker has a new identity'
$worker.ExecutablePath = $null
$failed = $false
try { New-ThumbnailMemorySample @($hostRecord, $worker) $hostRecord $workerPath | Out-Null } catch { $failed = $true }
Assert $failed 'unreadable candidate identity must fail rather than emit a host-only total'
`));

testCase('thumbnail:host-change-during-discovery', () => check(String.raw`
$script:queries = [System.Collections.Generic.List[string]]::new()
function Get-CimInstance {
    param($ClassName, $Filter, $ErrorAction)
    $script:queries.Add($Filter)
    if ($script:queries.Count -eq 1) { return @($hostRecord, $worker) }
    return (Record 100 1 'C:\fixture\scrollery.exe' 5 9000)
}
$sample = Get-ThumbnailMemorySample $hostRecord $workerPath
Assert ($sample.status -eq 'pid-reused' -and $sample.processes.Count -eq 0) 'discard snapshot if host changed during discovery'
Assert ($script:queries[0] -eq "ProcessId = 100 OR (ParentProcessId = 100 AND Name = 'native-thumbnail-worker.exe')") 'discovery must be scoped to this host'
Assert ($script:queries[1] -eq 'ProcessId = 100') 'recheck host identity after discovery'
`));

testCase('thumbnail:owned-idle-process-cim', () => check(String.raw`
$child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoLogo -NoProfile -NonInteractive -Command Start-Sleep -Seconds 20' -WindowStyle Hidden -PassThru
try {
    $identity = Get-CimInstance Win32_Process -Filter "ProcessId = $($child.Id)"
    Assert ($identity.ParentProcessId -eq $PID) 'owned test child parent PID'
    $sample = Get-ThumbnailMemorySample $identity $workerPath
    Assert ($sample.status -eq 'running' -and $sample.totals.workerCount -eq 0) 'idle child host-only snapshot'
    Assert ($sample.processes[0].pid -eq $child.Id -and $sample.processes[0].workingSetBytes -gt 0) 'real CIM metrics'
    Assert ($sample.processes[0].imagePath -eq $identity.ExecutablePath) 'real image identity'
} finally {
    if (-not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
    $child.Dispose()
}
`));

testCase('thumbnail:csv-sampling-loop', () => check(String.raw`
$runDir = Join-Path (Get-Location) ('target/T5/selftest-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $runDir -Force
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$ProcessId = 100; $SampleSeconds = 0; $SampleIntervalSeconds = 1
$sampleHostIdentity = $hostRecord
$workerPath = 'C:\fixture,images\native-thumbnail-worker.exe'
$worker.ExecutablePath = $workerPath; $worker.UserModeTime = 21250000
$sampleWorkerPath = $workerPath
$memorySampling = [ordered]@{ csvPath = $null; totalsCsvPath = $null; startedUtc = $null; endedUtc = $null; endedBecause = 'running'; sampleCount = 0; processRowCount = 0; errorCount = 0; lastError = $null }
$runConfig = @{ memorySampling = $memorySampling }
$runConfigPath = Join-Path $runDir 'run-config.json'
$script:sampleCalls = 0
function Get-ThumbnailMemorySample {
    param($hostIdentity, $path)
    $script:sampleCalls++
    if ($script:sampleCalls -eq 1) { throw 'synthetic CIM failure' }
    if ($script:sampleCalls -eq 2) { return (New-ThumbnailMemorySample @($hostRecord, $worker) $hostIdentity $path) }
    return @{ status = 'process-exited'; processes = @(); totals = $null }
}
function Start-Sleep { param($Seconds) }
$samplingBlock = $ast.EndBlock.Statements | Where-Object {
    $_ -is [System.Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains('$memoryCsvPath =')
}
Assert ($null -ne $samplingBlock) 'production sampling block exists'
$previousCulture = [System.Threading.Thread]::CurrentThread.CurrentCulture
try {
    [System.Threading.Thread]::CurrentThread.CurrentCulture = [cultureinfo]'de-DE'
    . ([scriptblock]::Create($samplingBlock.Extent.Text))
} finally { [System.Threading.Thread]::CurrentThread.CurrentCulture = $previousCulture }
$rows = @(Import-Csv -LiteralPath $memorySampling.csvPath)
$totals = @(Import-Csv -LiteralPath $memorySampling.totalsCsvPath)
$saved = Get-Content -LiteralPath $runConfigPath -Raw | ConvertFrom-Json
Assert ($rows.Count -eq 2 -and $totals.Count -eq 1) 'one sample writes two process rows and one total'
Assert ($rows[1].imagePath -eq $workerPath -and $rows[1].cpuSeconds -eq '3.125') 'CSV path quoting and invariant decimals'
Assert ($totals[0].workingSetBytes -eq '3000' -and $totals[0].sampleIndex -eq $rows[1].sampleIndex) 'CSV aggregate and sample join'
Assert ($saved.memorySampling.sampleCount -eq 1 -and $saved.memorySampling.processRowCount -eq 2 -and $saved.memorySampling.errorCount -eq 1 -and $saved.memorySampling.lastError -eq 'synthetic CIM failure' -and $saved.memorySampling.endedBecause -eq 'process-exited') 'final config, failed query accounting and termination'
`));

console.log('thumbnail baseline: 6 self-tests passed');
