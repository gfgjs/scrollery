#Requires -Version 5.1
#
# 缩略图全库基准采集脚本：只收集证据，不参与生成。
#   采集运行身份:实际 EXE 路径 + SHA256 + 版本信息、构建模式声明与路径推断、Git HEAD/分支/dirty、
#   缩略图设置(size/quality/AI 开关与短边/strategy/gpu_engine/skip_max_bytes)、冷热缓存与前后台说明。
#   采集素材清单:稳定相对路径 + 长度 + mtime(仅文件系统元数据,不是内容哈希)。
#   产出运行配置 JSON,并按需低频(默认 1 秒)采样指定宿主及其 native-thumbnail-worker 内存。
# 边界:
#   不启动应用、不触发缩略图生成、不清库;只采指定宿主及其同目录的直接图片 worker 子进程;
#   不写入/移动/删除任何素材、缓存或应用数据;素材与 EXE 只读。
#   所有产物只落在 -OutputDir 下的新运行目录;运行目录已存在即失败,不覆盖。
#   原始应用日志不由本脚本收集:由用户复制到运行目录的 raw-logs/ 并在配置里记录路径。
#   计时开销与真实吞吐均由应用侧测量,本脚本不解析、不推算吞吐。
#
# 示例(素材根目录模式,单行):
#   & ./scripts/bench/thumbnail-baseline.ps1 -SourceRoot D:\photos -ExePath C:\repo\target\release\scrollery.exe -BuildMode release -Size 64 -Quality 80 -AiEnabled $false -AiCacheShortEdge 336 -Strategy gpu -GpuEngine wic -SkipMaxBytes 0 -CacheState cold -RunMode foreground -SourceDiskNote "source=D: NVMe; cache=C: NVMe" -OutputDir D:\bench -RunOrdinal 01
# 示例(清单模式,并对已启动的应用采样内存):
#   & ./scripts/bench/thumbnail-baseline.ps1 -SourceList .\samples.txt -SourceListBase D:\photos -ExePath C:\repo\target\release\scrollery.exe -BuildMode release -Size 512 -Quality 80 -AiEnabled $true -AiCacheShortEdge 336 -Strategy gpu -GpuEngine wic -SkipMaxBytes 204800 -CacheState warm -RunMode background -SourceDiskNote "source=D: NVMe; cache=C: NVMe" -OutputDir D:\bench -RunOrdinal 02 -ProcessId 12345 -SampleSeconds 600

[CmdletBinding(DefaultParameterSetName = 'Root')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Root')]
    [string]$SourceRoot,

    [Parameter(Mandatory, ParameterSetName = 'List')]
    [string]$SourceList,

    [Parameter(ParameterSetName = 'List')]
    [string]$SourceListBase = '',

    [Parameter(Mandatory)]
    [string]$ExePath,

    [Parameter(Mandatory)]
    [ValidateSet('debug', 'release')]
    [string]$BuildMode,

    [Parameter(Mandatory)]
    [int]$Size,

    [Parameter(Mandatory)]
    [ValidateRange(1, 100)]
    [int]$Quality,

    [Parameter(Mandatory)]
    [bool]$AiEnabled,

    [Parameter(Mandatory)]
    [ValidateRange(1, 8192)]
    [int]$AiCacheShortEdge,

    [Parameter(Mandatory)]
    [ValidateSet('cpu', 'gpu', 'direct')]
    [string]$Strategy,

    [Parameter(Mandatory)]
    [string]$GpuEngine,

    [Parameter(Mandatory)]
    [ValidateRange(0, 9223372036854775807)]
    [long]$SkipMaxBytes,

    [Parameter(Mandatory)]
    [ValidateSet('cold', 'warm')]
    [string]$CacheState,

    [Parameter(Mandatory)]
    [ValidateSet('foreground', 'background')]
    [string]$RunMode,

    [Parameter(Mandatory)]
    [string]$SourceDiskNote,

    [Parameter(Mandatory)]
    [string]$OutputDir,

    [ValidatePattern('^[0-9]{2}$')]
    [string]$RunOrdinal = '01',

    [string]$RunLabel = '',

    [ValidateRange(0, 2147483647)]
    [int]$ProcessId = 0,

    [ValidateRange(1, 60)]
    [int]$SampleIntervalSeconds = 1,

    [ValidateRange(0, 86400)]
    [int]$SampleSeconds = 0,

    [string]$RawLogPath = '',

    [string]$RepoRoot = ''
)

$ErrorActionPreference = 'Stop'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$scriptWarnings = [System.Collections.Generic.List[string]]::new()

function Stop-WithError([string]$message) {
    throw "采集失败:$message"
}

function Write-JsonFile([string]$path, $value) {
    $json = $value | ConvertTo-Json -Depth 12
    [System.IO.File]::WriteAllText($path, $json + [Environment]::NewLine, $utf8NoBom)
}

function Get-FileSha256([string]$path) {
    return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}

function Get-ThumbnailHostStatus($record, $hostIdentity) {
    if ($null -eq $record) { return 'process-exited' }
    if ($null -eq $record.CreationDate -or [string]::IsNullOrWhiteSpace($record.ExecutablePath)) {
        throw '宿主创建时间或映像路径不可读,无法确认采样身份。'
    }
    if ($record.ProcessId -ne $hostIdentity.ProcessId -or
        $record.CreationDate.ToUniversalTime() -ne $hostIdentity.CreationDate.ToUniversalTime()) {
        return 'pid-reused'
    }
    if ($record.ExecutablePath -ne $hostIdentity.ExecutablePath) { return 'identity-changed' }
    return 'running'
}

function New-ThumbnailMemorySample([object[]]$records, $hostIdentity, [string]$workerPath) {
    $hostRecord = $records | Where-Object { $_.ProcessId -eq $hostIdentity.ProcessId } | Select-Object -First 1
    $status = Get-ThumbnailHostStatus $hostRecord $hostIdentity
    if ($status -ne 'running') { return @{ status = $status; processes = @(); totals = $null } }
    $selected = [System.Collections.Generic.List[object]]::new()
    $selected.Add($hostRecord)
    foreach ($record in $records) {
        if ($record.ProcessId -eq $hostIdentity.ProcessId -or $record.ParentProcessId -ne $hostIdentity.ProcessId) { continue }
        # 查询已限于该宿主的图片 worker；身份读不到时不能把漏采伪装成零占用。
        if ($null -eq $record.CreationDate -or [string]::IsNullOrWhiteSpace($record.ExecutablePath)) {
            throw "worker PID $($record.ProcessId) 的创建时间或映像路径不可读。"
        }
        # ParentProcessId 本身可能来自已退出的同号父进程，创建时间必须在本次宿主之后。
        if ($record.ExecutablePath -eq $workerPath -and
            $record.CreationDate.ToUniversalTime() -ge $hostIdentity.CreationDate.ToUniversalTime()) {
            $selected.Add($record)
        }
    }
    $rows = @(
        foreach ($record in $selected) {
            [pscustomobject][ordered]@{
                role = $(if ($record.ProcessId -eq $hostIdentity.ProcessId) { 'host' } else { 'native-thumbnail-worker' })
                pid = [int]$record.ProcessId
                parentPid = [int]$record.ParentProcessId
                startTimeUtc = $record.CreationDate.ToUniversalTime().ToString('o')
                imagePath = $record.ExecutablePath
                workingSetBytes = [long]$record.WorkingSetSize
                privateBytes = [long]$record.PrivatePageCount
                # Win32_Process 的两个峰值字段以 KiB 计，其余内存字段以字节计。
                peakWorkingSetBytes = [long]$record.PeakWorkingSetSize * 1024
                peakPrivateBytes = [long]$record.PeakPageFileUsage * 1024
                handleCount = [long]$record.HandleCount
                threadCount = [long]$record.ThreadCount
                cpuSeconds = ([double]$record.KernelModeTime + [double]$record.UserModeTime) / 10000000
            }
        }
    )
    $totals = [ordered]@{ workerCount = $rows.Count - 1 }
    # 生命周期峰值和累计 CPU 不可跨 worker 重启简单相加；只合计本次快照的存量。
    foreach ($field in @('workingSetBytes', 'privateBytes', 'handleCount', 'threadCount')) {
        $sum = [long]0
        foreach ($row in $rows) { $sum += $row.$field }
        $totals[$field] = $sum
    }
    return @{ status = 'running'; processes = $rows; totals = $totals }
}

function Get-ThumbnailMemorySample($hostIdentity, [string]$workerPath) {
    $targetId = [int]$hostIdentity.ProcessId
    $records = @(Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $targetId OR (ParentProcessId = $targetId AND Name = 'native-thumbnail-worker.exe')" -ErrorAction Stop)
    $sample = New-ThumbnailMemorySample $records $hostIdentity $workerPath
    if ($sample.status -ne 'running') { return $sample }
    # CIM 枚举不是原子操作；发现子进程后复核宿主，丢弃退出或 PID 换代期间的快照。
    $currentHost = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $targetId" -ErrorAction Stop
    $status = Get-ThumbnailHostStatus $currentHost $hostIdentity
    if ($status -ne 'running') { return @{ status = $status; processes = @(); totals = $null } }
    return $sample
}

# 只用于本机同条件多次运行之间的稳定相对路径;不是内容身份,内容不同但元数据相同会得到同样的行。
function Get-RelativeUnder([string]$basePath, [string]$fullPath) {
    $b = $basePath.TrimEnd('\', '/')
    if ($fullPath.Length -le $b.Length) { return $null }
    if (-not $fullPath.StartsWith($b, [System.StringComparison]::OrdinalIgnoreCase)) { return $null }
    $sep = $fullPath.Substring($b.Length, 1)
    if ($sep -ne '\' -and $sep -ne '/') { return $null }
    return $fullPath.Substring($b.Length + 1).Replace('\', '/')
}

function Get-GitLines([string]$root, [string[]]$gitArgs) {
    $out = & git -C $root @gitArgs 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    if ($null -eq $out) { return @() }
    return @($out | ForEach-Object { [string]$_ })
}

# ── 1. 输入校验:任何缺失/空值/矛盾都在创建运行目录之前失败 ────────────────────

$tierSizes = @(64, 128, 256, 512, 1024)
if ($tierSizes -notcontains $Size) {
    Stop-WithError "-Size=$Size 不是有效档位 $($tierSizes -join '/')。应用会把非档位值吸附到最近档位,记录将失真。"
}
if ($Size -ne 64 -and $Size -ne 512) {
    $scriptWarnings.Add("-Size=$Size 不是首要对比场景(64/512),请确认这是有意选择。")
}

if ([string]::IsNullOrWhiteSpace($SourceDiskNote)) {
    Stop-WithError '-SourceDiskNote 不能为空:需要写明素材盘与缓存盘的介质/接口(例如 source=D: NVMe SSD; cache=C: NVMe SSD)。'
}
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    Stop-WithError '-OutputDir 不能为空。'
}

$exeResolved = $null
if ([string]::IsNullOrWhiteSpace($ExePath)) {
    Stop-WithError '-ExePath 不能为空:必须是本次实际运行的应用二进制,不是仓库里的中间产物猜测。'
}
if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    Stop-WithError "-ExePath 不存在或不是文件:$ExePath"
}
$exeResolved = (Resolve-Path -LiteralPath $ExePath).Path

# 构建模式与路径矛盾说明用户量错了二进制,直接失败而不是留下误导记录。
$exeNorm = $exeResolved.Replace('/', '\').ToLowerInvariant()
$buildModeFromPath = 'unknown'
if ($exeNorm -match '\\target\\debug\\') { $buildModeFromPath = 'debug' }
elseif ($exeNorm -match '\\target\\release\\') { $buildModeFromPath = 'release' }
if ($buildModeFromPath -ne 'unknown' -and $buildModeFromPath -ne $BuildMode) {
    Stop-WithError "-BuildMode=$BuildMode 与 EXE 路径推断的 $buildModeFromPath 矛盾($exeResolved)。请确认实际运行的二进制。"
}

$sampleTargetPath = ''
$sampleTargetStartUtc = ''
$sampleTargetMatchesExe = $null
$sampleHostIdentity = $null
$sampleWorkerPath = Join-Path (Split-Path -Parent $exeResolved) 'native-thumbnail-worker.exe'
if ($ProcessId -gt 0) {
    $probe = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $ProcessId" -ErrorAction Stop
    if ($null -eq $probe) {
        Stop-WithError "-ProcessId=$ProcessId 当前不存在。请在应用启动后取得其 PID 再采集;只要元数据可省略 -ProcessId 重跑。"
    }
    # 无法确认身份时拒绝采样，避免把其它进程的内存登记在本次二进制名下。
    try { $sampleTargetStartUtc = $probe.CreationDate.ToUniversalTime().ToString('o') }
    catch { Stop-WithError "-ProcessId=$ProcessId 的 CreationDate 不可读,无法确认采样目标。" }
    # 目标进程的可执行文件路径必须与 -ExePath 指向同一文件,否则采集到的是别的二进制。
    $probePath = $null
    $probePath = $probe.ExecutablePath
    if ([string]::IsNullOrWhiteSpace($probePath)) {
        Stop-WithError "-ProcessId=$ProcessId 的映像路径不可读,无法核对是否就是 -ExePath。"
    }
    else {
        $sampleTargetPath = $probePath
        $resolvedProbe = $null
        try { $resolvedProbe = (Resolve-Path -LiteralPath $probePath -ErrorAction Stop).Path } catch { $resolvedProbe = $probePath }
        $sampleTargetMatchesExe = ($resolvedProbe -eq $exeResolved)
        if (-not $sampleTargetMatchesExe) {
            Stop-WithError "-ProcessId=$ProcessId 的映像是 $resolvedProbe,与 -ExePath=$exeResolved 不是同一文件。请确认采样目标。"
        }
    }
    $sampleHostIdentity = $probe
}

$sourceMode = $PSCmdlet.ParameterSetName
$manifestRows = [System.Collections.Generic.List[object]]::new()
$manifestSourceRoot = ''
$manifestListPath = ''
$manifestRelativeBase = ''
$duplicateEntries = 0

if ($sourceMode -eq 'Root') {
    if ([string]::IsNullOrWhiteSpace($SourceRoot)) { Stop-WithError '-SourceRoot 不能为空。' }
    if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
        Stop-WithError "-SourceRoot 不是已存在的目录:$SourceRoot"
    }
    $manifestSourceRoot = (Resolve-Path -LiteralPath $SourceRoot).Path.TrimEnd('\', '/')
    $manifestRelativeBase = $manifestSourceRoot
    $files = Get-ChildItem -LiteralPath $manifestSourceRoot -Recurse -File -Force
    foreach ($f in $files) {
        $rel = Get-RelativeUnder $manifestSourceRoot $f.FullName
        if ($null -eq $rel) { Stop-WithError "无法把 $($f.FullName) 归入根目录 $manifestSourceRoot。" }
        $manifestRows.Add([ordered]@{
            rel = $rel.Replace([char]9, ' ').Replace([char]10, ' ').Replace([char]13, ' ')
            len = [long]$f.Length
            mtime = $f.LastWriteTimeUtc.ToString('o')
        })
    }
}
else {
    if ([string]::IsNullOrWhiteSpace($SourceList)) { Stop-WithError '-SourceList 不能为空。' }
    if (-not (Test-Path -LiteralPath $SourceList -PathType Leaf)) {
        Stop-WithError "-SourceList 不存在或不是文件:$SourceList"
    }
    $manifestListPath = (Resolve-Path -LiteralPath $SourceList).Path
    if (-not [string]::IsNullOrWhiteSpace($SourceListBase)) {
        if (-not (Test-Path -LiteralPath $SourceListBase -PathType Container)) {
            Stop-WithError "-SourceListBase 不是已存在的目录:$SourceListBase"
        }
        $manifestRelativeBase = (Resolve-Path -LiteralPath $SourceListBase).Path.TrimEnd('\', '/')
    }
    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $lineNo = 0
    foreach ($line in [System.IO.File]::ReadAllLines($manifestListPath)) {
        $lineNo++
        $entry = $line.Trim()
        if ($entry.Length -eq 0) { continue }
        if ($entry.StartsWith('#')) { continue }
        $resolvedEntry = $entry
        if (-not [System.IO.Path]::IsPathRooted($resolvedEntry)) {
            # 相对条目统一相对 -SourceListBase;未给基准时退回清单所在目录。
            $relativeAnchor = $manifestRelativeBase
            if ([string]::IsNullOrWhiteSpace($relativeAnchor)) {
                $relativeAnchor = Split-Path -Parent $manifestListPath
            }
            $resolvedEntry = Join-Path $relativeAnchor $resolvedEntry
        }
        if (-not (Test-Path -LiteralPath $resolvedEntry -PathType Leaf)) {
            Stop-WithError "清单第 $lineNo 行指向的文件不存在:$resolvedEntry"
        }
        $full = (Resolve-Path -LiteralPath $resolvedEntry).Path
        if (-not $seen.Add($full)) {
            $duplicateEntries++
            continue
        }
        $fi = Get-Item -LiteralPath $full -Force
        if ([string]::IsNullOrWhiteSpace($manifestRelativeBase)) {
            $rel = $full.Replace('\', '/')
        }
        else {
            $rel = Get-RelativeUnder $manifestRelativeBase $full
            if ($null -eq $rel) {
                Stop-WithError "$full 不在 -SourceListBase=$manifestRelativeBase 之下,无法给出稳定相对路径。"
            }
        }
        $manifestRows.Add([ordered]@{
            rel = $rel.Replace([char]9, ' ').Replace([char]10, ' ').Replace([char]13, ' ')
            len = [long]$fi.Length
            mtime = $fi.LastWriteTimeUtc.ToString('o')
        })
    }
}

$orderedRows = @($manifestRows | Sort-Object -Property rel)
if ($orderedRows.Count -eq 0) {
    Stop-WithError '素材清单为空:没有可记录的文件。空输入不创建运行记录,请检查素材路径或清单内容。'
}
if ($duplicateEntries -gt 0) {
    $scriptWarnings.Add("清单含 $duplicateEntries 个重复条目,已按完整路径去重。")
}

$scriptPath = $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($scriptPath) -or -not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) {
    $scriptPath = Join-Path $PSScriptRoot 'thumbnail-baseline.ps1'
}

# 仓库根:优先参数,其次从脚本位置向上找 .git。
$repoResolved = ''
if (-not [string]::IsNullOrWhiteSpace($RepoRoot)) {
    if (-not (Test-Path -LiteralPath $RepoRoot -PathType Container)) {
        Stop-WithError "-RepoRoot 不是已存在的目录:$RepoRoot"
    }
    $repoResolved = (Resolve-Path -LiteralPath $RepoRoot).Path
}
else {
    $cursor = Split-Path -Parent $PSScriptRoot
    while (-not [string]::IsNullOrWhiteSpace($cursor)) {
        if (Test-Path -LiteralPath (Join-Path $cursor '.git')) { $repoResolved = $cursor; break }
        $parent = Split-Path -Parent $cursor
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}

$gitAvailable = $false
$gitHead = $null
$gitBranch = $null
$gitDirtyCount = 0
$gitDirty = $null
if ($repoResolved -eq '') {
    $scriptWarnings.Add('未找到 Git 仓库根(.git),Git 证据记为不可用。可用 -RepoRoot 显式指定。')
}
elseif ($null -eq (Get-Command git -ErrorAction SilentlyContinue)) {
    $scriptWarnings.Add('PATH 中没有 git 可执行文件,Git 证据记为不可用。')
}
else {
    $headOut = Get-GitLines $repoResolved @('rev-parse', 'HEAD')
    if ($null -eq $headOut) {
        $scriptWarnings.Add("在 $repoResolved 上读取 Git HEAD 失败,Git 证据记为不可用。")
    }
    else {
        # 单元素结果经管道会被拆成标量,包一层数组再取下标。
        $gitAvailable = $true
        $gitHead = @($headOut)[0]
        $branchOut = Get-GitLines $repoResolved @('rev-parse', '--abbrev-ref', 'HEAD')
        if ($null -ne $branchOut) { $gitBranch = @($branchOut)[0] }
        $statusOut = Get-GitLines $repoResolved @('status', '--porcelain')
        if ($null -ne $statusOut) {
            $gitDirtyCount = @($statusOut).Count
            $gitDirty = ($gitDirtyCount -gt 0)
        }
    }
}

$benchmarkEnvName = 'SCROLLERY_THUMB_BENCHMARK'
$benchmarkObserved = [string]$env:SCROLLERY_THUMB_BENCHMARK
if ($benchmarkObserved -ne '1') {
    $scriptWarnings.Add("采集 shell 里 $benchmarkEnvName='$benchmarkObserved'(不是 1)。本脚本不启动应用:必须在启动应用的同一环境里设为 1,否则应用不会输出基准摘要。")
}

# ── 2. 创建运行目录并落盘证据(目录已存在即失败,不覆盖) ──────────────────────

if (-not (Test-Path -LiteralPath $OutputDir -PathType Container)) {
    if (Test-Path -LiteralPath $OutputDir) {
        Stop-WithError "-OutputDir 已存在但不是目录:$OutputDir"
    }
    $null = New-Item -ItemType Directory -Path $OutputDir -Force
}
$outRoot = (Resolve-Path -LiteralPath $OutputDir).Path
if ($repoResolved -ne '') {
    $relOut = Get-RelativeUnder $repoResolved.Replace('/', '\') $outRoot.Replace('/', '\')
    if ($null -ne $relOut) {
        $scriptWarnings.Add("输出目录位于 Git 仓库内($relOut),注意不要误提交采集产物。")
    }
}

$nowUtc = (Get-Date).ToUniversalTime()
$labelSuffix = ''
if (-not [string]::IsNullOrWhiteSpace($RunLabel)) {
    $cleanLabel = ($RunLabel -replace '[^A-Za-z0-9._-]', '-').Trim('-')
    if ($cleanLabel.Length -gt 0) { $labelSuffix = '-' + $cleanLabel }
}
$runId = $nowUtc.ToString("yyyyMMdd'T'HHmmss'Z'") + '-run' + $RunOrdinal + $labelSuffix
$runDir = Join-Path $outRoot $runId
if (Test-Path -LiteralPath $runDir) {
    Stop-WithError "运行目录已存在,不覆盖:$runDir 。请换 -RunOrdinal 或 -RunLabel。"
}
$null = New-Item -ItemType Directory -Path $runDir
$rawLogDir = Join-Path $runDir 'raw-logs'
$null = New-Item -ItemType Directory -Path $rawLogDir

$binaryInfo = [ordered]@{
    path = $exeResolved
    sha256 = Get-FileSha256 $exeResolved
    lengthBytes = [long](Get-Item -LiteralPath $exeResolved).Length
    lastWriteTimeUtc = (Get-Item -LiteralPath $exeResolved).LastWriteTimeUtc.ToString('o')
    fileVersion = $null
    productVersion = $null
    buildModeDeclared = $BuildMode
    buildModeFromPath = $buildModeFromPath
}
$exeVersion = (Get-Item -LiteralPath $exeResolved).VersionInfo
if ($null -ne $exeVersion) {
    $binaryInfo.fileVersion = $exeVersion.FileVersion
    $binaryInfo.productVersion = $exeVersion.ProductVersion
}
Write-JsonFile (Join-Path $runDir 'binary.json') $binaryInfo

Write-JsonFile (Join-Path $runDir 'git.json') ([ordered]@{
    repoRoot = $repoResolved
    available = $gitAvailable
    head = $gitHead
    branch = $gitBranch
    dirty = $gitDirty
    dirtyEntryCount = $gitDirtyCount
    note = 'dirty 依据 git status --porcelain 是否为空;未跟踪文件也计入。'
})

$manifestPath = Join-Path $runDir 'source-manifest.tsv'
$manifestLines = [System.Collections.Generic.List[string]]::new()
$manifestLines.Add((@('relative_path', 'length_bytes', 'mtime_utc') -join [char]9))
$manifestTotalBytes = [long]0
foreach ($row in $orderedRows) {
    $manifestTotalBytes += [long]$row.len
    $manifestLines.Add((@($row.rel, [string]$row.len, $row.mtime) -join [char]9))
}
$lf = [string][char]10
[System.IO.File]::WriteAllText($manifestPath, (($manifestLines -join $lf) + $lf), $utf8NoBom)

[System.IO.File]::WriteAllText(
    (Join-Path $rawLogDir 'README.txt'),
    @(
        '把本次运行的应用原始日志复制到本目录,并在 run-config.json 的 rawLog.recordedPaths 里登记文件名。',
        '本脚本不收集、不解析、不推算日志中的吞吐数字。',
        '应用侧基准摘要以 [FullThumbGen] BENCHMARK 开头的单行 JSON 结束;请把该行原样保留在日志里。'
    ) -join [Environment]::NewLine,
    $utf8NoBom
)

# ── 3. 运行配置(采样结束后重写,保证中断时也有完整记录) ──────────────────────

$hostInfo = [ordered]@{
    computerName = $env:COMPUTERNAME
    userName = $env:USERNAME
    osVersion = [System.Environment]::OSVersion.VersionString
    architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    psVersion = $PSVersionTable.PSVersion.ToString()
    logicalProcessors = [System.Environment]::ProcessorCount
    totalPhysicalMemoryBytes = $null
}
try {
    $cs = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop
    if ($null -ne $cs -and $null -ne $cs.TotalPhysicalMemory) {
        $hostInfo.totalPhysicalMemoryBytes = [long]$cs.TotalPhysicalMemory
    }
}
catch {
    $scriptWarnings.Add('读取物理内存总量失败(Get-CimInstance 不可用),该字段记为 null。')
}

$memorySampling = [ordered]@{
    enabled = ($ProcessId -gt 0)
    pid = $(if ($ProcessId -gt 0) { $ProcessId } else { $null })
    intervalSeconds = $SampleIntervalSeconds
    maxSeconds = $SampleSeconds
    sampleCount = 0
    errorCount = 0
    startedUtc = $null
    endedUtc = $null
    endedBecause = $(if ($ProcessId -gt 0) { 'running' } else { 'not-requested' })
    lastError = $null
    csvPath = $null
    totalsCsvPath = $null
    scope = 'host-and-direct-native-thumbnail-workers'
    workerImagePath = $sampleWorkerPath
    processRowCount = 0
    identitySource = 'Win32_Process: ProcessId + CreationDate + ExecutablePath; worker additionally requires ParentProcessId and CreationDate >= host'
    targetImagePath = $(if ($ProcessId -gt 0) { $sampleTargetPath } else { $null })
    targetStartTimeUtc = $(if ($ProcessId -gt 0) { $sampleTargetStartUtc } else { $null })
    targetMatchesExe = $(if ($ProcessId -gt 0) { $sampleTargetMatchesExe } else { $null })
    fields = [ordered]@{
        timestampUtc = '采样时刻(UTC)。'
        sampleIndex = '同一轮逐进程记录与合计记录的关联编号;sampleCount 计成功轮数。'
        startTimeUtc = 'Win32_Process.CreationDate;与 PID、映像路径共同标识一次进程生命周期。'
        workingSetBytes = 'OS 工作集(物理驻留)。这不是 private bytes,两者不可互换。'
        privateBytes = '进程私有提交(Private Bytes / commit)。'
        peakWorkingSetBytes = '进程生命周期峰值工作集。'
        peakPrivateBytes = '进程生命周期峰值提交量(PROCESS_MEMORY_COUNTERS.PeakPagefileUsage)。'
        handleCount = '句柄数。'
        threadCount = '线程数。'
        cpuSeconds = '进程累计 CPU 时间(秒)。'
    }
    note = '逐轮发现指定宿主的同目录 native-thumbnail-worker.exe 直接子进程;核对宿主创建时间和路径,并在发现后再次复核宿主身份。worker 按 PID、创建时间和路径区分重启,不沿用旧 PID。CIM 查询失败或身份不可读的轮次不输出合计,记入 errorCount。低频快照可能漏掉采样间隔内结束的 worker,不含逐图阶段耗时。'
    totalsNote = '合计仅含本轮观察到的宿主与 worker 工作集、私有提交、句柄、线程。工作集求和可能重复计入共享页;逐进程生命周期峰值不相加,合计序列最大值只是采样峰值下界。workerCount=0 表示该轮未观察到 worker。不含 WebView、其他 sidecar 或 GPU 显存。'
}

$runConfig = [ordered]@{
    schemaVersion = 2
    runId = $runId
    runOrdinal = $RunOrdinal
    createdAtUtc = $nowUtc.ToString('o')
    outputDir = $runDir
    collector = [ordered]@{
        scriptPath = $scriptPath
        scriptSha256 = Get-FileSha256 $scriptPath
        note = '本脚本只采集证据:不启动应用、不触发生成、不清库、不修改用户数据。'
    }
    host = $hostInfo
    binary = $binaryInfo
    git = [ordered]@{
        repoRoot = $repoResolved
        available = $gitAvailable
        head = $gitHead
        branch = $gitBranch
        dirty = $gitDirty
        dirtyEntryCount = $gitDirtyCount
    }
    thumbnailSettings = [ordered]@{
        size = $Size
        quality = $Quality
        aiEnabled = $AiEnabled
        aiCacheShortEdge = $AiCacheShortEdge
        aiCacheShortEdgeNote = 'AI 分析缓存短边(设置键 ai_cache_short_edge);-AiEnabled $false 时记录值但不生效。'
        strategy = $Strategy
        gpuEngine = $GpuEngine
        skipMaxBytes = $SkipMaxBytes
        skipMaxKbEquivalent = [long]($SkipMaxBytes / 1024)
        skipMaxBytesNote = '应用设置键 thumb_skip_max_kb 以 KB 计,运行时字段是字节;两值同时记录以免单位混淆。'
    }
    runConditions = [ordered]@{
        cacheState = $CacheState
        runMode = $RunMode
        sourceDiskNote = $SourceDiskNote
    }
    source = [ordered]@{
        mode = $(if ($sourceMode -eq 'Root') { 'root' } else { 'list' })
        sourceRoot = $manifestSourceRoot
        listPath = $manifestListPath
        relativeBase = $manifestRelativeBase
        relativePathSeparator = '/'
        fileCount = $orderedRows.Count
        totalBytes = $manifestTotalBytes
        duplicateEntriesDropped = $duplicateEntries
        manifestPath = $manifestPath
        manifestSort = 'relative_path 升序(同一台机器同集合时稳定)'
        manifestNote = '清单只含相对路径、长度和 mtime,不是内容哈希:内容变了但长度与 mtime 相同不会体现。'
    }
    benchmark = [ordered]@{
        envVarName = $benchmarkEnvName
        envVarIntendedValue = '1'
        envVarObservedInCollectorShell = $benchmarkObserved
        appliedByThisScript = $false
        summaryLinePrefix = '[FullThumbGen] BENCHMARK'
        expectedSummaryLines = 1
        scope = '仅全库/增量生成输出单条收尾摘要;原进度 generated 仍是旧口径(含直显与失败结果)。'
        fieldNotes = [ordered]@{
            queuePeak = '队列长度为采样峰值下界,不是精确峰值。'
            backend = 'backend 是实际最终 ImageEngine::name,不是"确实用了 GPU"的事实。'
            publicationSpanRate = '稳态辅助字段,取自首批到末批已发布新图的区间且分子排除首批;只有一批时为 null,不能当作完整稳态测量。'
            memory = '内存低频采集指定宿主及其同目录 native-thumbnail-worker 直接子进程,逐进程身份与当次合计分别落盘。'
        }
        note = '脚本不代用户启动应用,也不解析摘要结构;请把摘要行连同原始日志一起保留。'
    }
    memorySampling = $memorySampling
    rawLog = [ordered]@{
        dir = $rawLogDir
        recordedPaths = $(if ([string]::IsNullOrWhiteSpace($RawLogPath)) { @() } else { @($RawLogPath) })
        note = '原始日志由用户复制并登记路径;脚本不读取日志内容。'
    }
    warnings = @($scriptWarnings)
}

$runConfigPath = Join-Path $runDir 'run-config.json'

# ── 4. 可选低频内存采样(指定宿主及对应图片 worker;宿主退出即停止) ──────────

if ($ProcessId -gt 0) {
    $memoryCsvPath = Join-Path $runDir 'memory-samples.csv'
    $totalsCsvPath = Join-Path $runDir 'memory-totals.csv'
    $memorySampling.csvPath = $memoryCsvPath
    $memorySampling.totalsCsvPath = $totalsCsvPath
    $memorySampling.startedUtc = (Get-Date).ToUniversalTime().ToString('o')
    # 先落一次配置:采样进行中硬中止也会留下完整运行记录,不至于只剩散件。
    Write-JsonFile $runConfigPath $runConfig
    $writer = [System.IO.StreamWriter]::new($memoryCsvPath, $false, $utf8NoBom)
    $writer.AutoFlush = $true
    $totalsWriter = $null
    $endedBecause = 'running'
    try {
        $totalsWriter = [System.IO.StreamWriter]::new($totalsCsvPath, $false, $utf8NoBom)
        $totalsWriter.AutoFlush = $true
        $writer.WriteLine('sampleIndex,timestampUtc,role,pid,parentPid,startTimeUtc,imagePath,workingSetBytes,privateBytes,peakWorkingSetBytes,peakPrivateBytes,handleCount,threadCount,cpuSeconds')
        $totalsWriter.WriteLine('sampleIndex,timestampUtc,workerCount,workingSetBytes,privateBytes,handleCount,threadCount')
        $deadline = $null
        if ($SampleSeconds -gt 0) { $deadline = (Get-Date).AddSeconds($SampleSeconds) }
        Write-Host "开始采样宿主 PID $ProcessId 及其图片 worker(间隔 $SampleIntervalSeconds s,上限 $(if ($SampleSeconds -gt 0) { "$SampleSeconds s" } else { '直到宿主退出或中断' }))。"
        while ($true) {
            try {
                $timestampUtc = (Get-Date).ToUniversalTime().ToString('o')
                $sample = Get-ThumbnailMemorySample $sampleHostIdentity $sampleWorkerPath
                if ($sample.status -ne 'running') { $endedBecause = $sample.status; break }
                $sampleIndex = [int]$memorySampling.sampleCount + 1
                foreach ($processRow in $sample.processes) {
                    $row = [ordered]@{ sampleIndex = $sampleIndex; timestampUtc = $timestampUtc }
                    foreach ($property in $processRow.PSObject.Properties) { $row[$property.Name] = $property.Value }
                    # CSV 会调用值的本地化 ToString；CPU 秒先固定小数点，路径再交给 CSV 转义。
                    $row.cpuSeconds = $processRow.cpuSeconds.ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)
                    $writer.WriteLine((@([pscustomobject]$row | ConvertTo-Csv -NoTypeInformation)[1]))
                }
                $totalRow = [ordered]@{ sampleIndex = $sampleIndex; timestampUtc = $timestampUtc }
                foreach ($key in $sample.totals.Keys) { $totalRow[$key] = $sample.totals[$key] }
                $totalsWriter.WriteLine((@([pscustomobject]$totalRow | ConvertTo-Csv -NoTypeInformation)[1]))
                $memorySampling.sampleCount = $sampleIndex
                $memorySampling.processRowCount += $sample.processes.Count
            }
            catch {
                # 读取单个样本失败不等于进程退出:记错误并继续,避免把权限/瞬时错误伪报为退出。
                $memorySampling.errorCount = [int]$memorySampling.errorCount + 1
                $memorySampling.lastError = $_.Exception.Message
            }
            if ($null -ne $deadline -and (Get-Date) -ge $deadline) { $endedBecause = 'duration-reached'; break }
            Start-Sleep -Seconds $SampleIntervalSeconds
        }
    }
    finally {
        if ($endedBecause -eq 'running') { $endedBecause = 'interrupted' }
        $memorySampling.endedBecause = $endedBecause
        $memorySampling.endedUtc = (Get-Date).ToUniversalTime().ToString('o')
        try { $writer.Flush() } catch { }
        try { $writer.Dispose() } catch { }
        if ($null -ne $totalsWriter) {
            try { $totalsWriter.Dispose() } catch { }
        }
        Write-JsonFile $runConfigPath $runConfig
    }
}
else {
    Write-JsonFile $runConfigPath $runConfig
}

# ── 5. 回执 ──────────────────────────────────────────────────────────────────

Write-Host "运行目录:$runDir"
Write-Host "  素材清单 :$manifestPath($($orderedRows.Count) 个文件,$manifestTotalBytes 字节;仅元数据,不是内容哈希)"
Write-Host "  二进制   :$(Join-Path $runDir 'binary.json')(sha256=$($binaryInfo.sha256))"
Write-Host "  Git      :$(Join-Path $runDir 'git.json')(head=$gitHead dirty=$gitDirty)"
if ($ProcessId -gt 0) {
    Write-Host "  内存采样 :$($memorySampling.csvPath)($($memorySampling.sampleCount) 轮,$($memorySampling.processRowCount) 条进程记录,结束原因=$($memorySampling.endedBecause))"
    Write-Host "  内存合计 :$($memorySampling.totalsCsvPath)"
}
Write-Host "  运行配置 :$runConfigPath"
Write-Host "  原始日志 :请复制到 $rawLogDir 并登记路径(脚本不收集日志)"
if ($scriptWarnings.Count -gt 0) {
    Write-Host "提醒($($scriptWarnings.Count) 条):"
    foreach ($w in $scriptWarnings) { Write-Host "  - $w" }
}
