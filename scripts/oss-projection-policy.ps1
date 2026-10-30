# 发布器和独立门禁共用此策略；只检查路径，不读取文件内容。
[CmdletBinding()]
param(
    [Alias('Repo')] [string] $CheckRepo,
    [Alias('Ref')] [string] $CheckRef = 'HEAD',
    [switch] $History
)

$script:OssExcludedPaths = @(
    'copy.bara.sky'
    '.github/workflows/sync-oss.yml'
    'scripts/publish-oss.ps1'
    '.agents'
    '.github/copilot-instructions.md'
    '.github/instructions'
    'AGENTS.md'
    '.zcode'
    '.pnpm-store'
    '.research-tmp'
    'scripts/install-skills.mjs'
)
$script:OssDocsWhitelist = @(
    'docs/CHANGELOG.md'
    'docs/development.md'
    'docs/legal/LICENSE'
    'docs/legal/NOTICE.md'
    'docs/legal/SOURCE.md'
    'docs/legal/COMMERCIAL.md'
    'docs/legal/ADDITIONAL-PERMISSION.md'
    'docs/legal/CLA.md'
    'docs/legal/TRADEMARK.md'
)

function Test-OssPublicPath {
    param([Parameter(Mandatory)] [string] $RelativePath)
    # Git 路径按 / 分段；拒绝 Windows 别名与穿越，避免检查名和落盘名不同。
    if ($RelativePath -match '[\\:\x00]' -or $RelativePath.StartsWith('/')) { throw "非法投影路径:$RelativePath" }
    foreach ($part in $RelativePath.Split('/')) {
        if ($part -in @('', '.', '..', '.git') -or $part -match '[. ]$' -or
            $part -match '^(?i:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)') {
            throw "非法投影路径:$RelativePath"
        }
    }
    $root = $RelativePath.Split('/')[0]
    if ($root.StartsWith('.docs-', [StringComparison]::OrdinalIgnoreCase) -or
        $root.StartsWith('.worklog', [StringComparison]::OrdinalIgnoreCase)) { return $false }
    if ($root -ieq 'docs') { return $script:OssDocsWhitelist -ccontains $RelativePath }
    foreach ($excluded in $script:OssExcludedPaths) {
        if ($RelativePath.Equals($excluded, [StringComparison]::OrdinalIgnoreCase) -or
            $RelativePath.StartsWith($excluded + '/', [StringComparison]::OrdinalIgnoreCase)) { return $false }
    }
    return $true
}

function Invoke-OssGitText {
    param([string] $Repo, [string[]] $GitArgs)
    # NUL 分隔的原始 UTF-8 输出保留中文、换行与通配字符文件名。
    $start = [Diagnostics.ProcessStartInfo]::new('git')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false, $true)
    foreach ($argument in (@('-C', $Repo) + $GitArgs)) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "git 失败(exit $($process.ExitCode)):$($stderr.GetAwaiter().GetResult())" }
        return $stdout.GetAwaiter().GetResult()
    }
    finally { $process.Dispose() }
}

function Get-OssGitEntries {
    param([string] $Repo, [string] $Tree)
    $raw = Invoke-OssGitText $Repo @('ls-tree', '-r', '-z', '--full-tree', $Tree)
    foreach ($record in $raw.Split([char]0, [StringSplitOptions]::RemoveEmptyEntries)) {
        if ($record -notmatch '(?s)^([0-7]{6}) (?:blob|commit) [0-9a-f]+\t(.+)$') { throw '无法解析提交树路径' }
        [pscustomobject]@{ Mode = $Matches[1]; Path = $Matches[2] }
    }
}

function Assert-OssPublicPaths {
    param([AllowEmptyCollection()] [string[]] $Paths)
    foreach ($relative in $Paths) {
        if (-not (Test-OssPublicPath $relative)) { throw "公开树含禁发路径:$relative" }
    }
}

function Assert-OssGitTree {
    param([string] $Repo, [string] $Tree, [switch] $History)
    if ($History) {
        if ((Invoke-OssGitText $Repo @('rev-parse', '--is-shallow-repository')).Trim() -eq 'true') {
            throw '历史门禁需要完整历史，拒绝浅克隆'
        }
        # 遍历全部父链，包含合并的侧支；逐树复用 NUL 路径解析，文件名中的换行不拆记录。
        $commit = (Invoke-OssGitText $Repo @('rev-parse', '--verify', "${Tree}^{commit}")).Trim()
        $commits = Invoke-OssGitText $Repo @('rev-list', $commit)
        foreach ($ancestor in $commits.Split("`n", [StringSplitOptions]::RemoveEmptyEntries)) {
            Assert-OssGitTree -Repo $Repo -Tree $ancestor.Trim()
        }
        return
    }
    Assert-OssPublicPaths -Paths @(Get-OssGitEntries $Repo $Tree | ForEach-Object { $_.Path })
}

function Assert-OssSafePath {
    param([string] $Root, [string] $Path, [switch] $AllowRoot, [switch] $Recurse)
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $pathFull = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $comparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    if ($rootFull -eq '' -or $rootFull -eq [IO.Path]::GetPathRoot($rootFull).TrimEnd('\', '/')) { throw '操作根不能是文件系统根' }
    if (-not (($AllowRoot -and $pathFull.Equals($rootFull, $comparison)) -or
        $pathFull.StartsWith($rootFull + [IO.Path]::DirectorySeparatorChar, $comparison))) { throw "目标越界:$pathFull" }
    # 逐级检查现存父目录；Resolve-Path 本身不会消除 junction/symlink。
    $current = $pathFull
    while ($current) {
        $item = Get-Item -LiteralPath $current -Force -ErrorAction Ignore
        if ($null -ne $item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "目标经过链接:$current" }
        $current = [IO.Path]::GetDirectoryName($current)
    }
    if ($Recurse -and (Test-Path -LiteralPath $pathFull -PathType Container)) {
        foreach ($item in Get-ChildItem -LiteralPath $pathFull -Force -Recurse) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "操作树含链接:$($item.FullName)" }
        }
    }
}

function Export-OssProjection {
    param([string] $Repo, [string] $Tree, [string] $WorkRoot)
    # 先按实际提交路径定出排除面，再解包；内部文件无需暂存、删除或回填。
    $entries = @(Get-OssGitEntries $Repo $Tree)
    $allowed = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($entry in $entries) {
        if (-not (Test-OssPublicPath $entry.Path)) { continue }
        if ($entry.Mode -notin @('100644', '100755')) { throw "公开路径必须是普通文件:$($entry.Path)" }
        [void] $allowed.Add($entry.Path)
    }
    $parent = Split-Path -Parent $WorkRoot
    Assert-OssSafePath -Root $parent -Path $WorkRoot -Recurse
    if (Get-ChildItem -LiteralPath $WorkRoot -Force) { throw '投影临时目录必须为空' }
    $archive = $WorkRoot + '.zip'
    Assert-OssSafePath -Root $parent -Path $archive
    if (Test-Path -LiteralPath $archive) { throw "归档路径已存在:$archive" }
    Invoke-OssGitText $Repo @('-c', 'core.autocrlf=false', 'archive', '--format=zip', '-o', $archive, $Tree) | Out-Null
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($entry in $zip.Entries) {
            if (-not $allowed.Contains($entry.FullName)) { continue }
            $destination = Join-Path $WorkRoot $entry.FullName
            Assert-OssSafePath -Root $WorkRoot -Path $destination
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $destination, $false)
        }
    }
    finally { $zip.Dispose() }
    Assert-OssPublicPaths -Paths @(Get-ChildItem -LiteralPath $WorkRoot -File -Force -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($WorkRoot, $_.FullName).Replace('\', '/') })
}

if ($MyInvocation.InvocationName -ne '.') {
    $ErrorActionPreference = 'Stop'
    if (-not $CheckRepo) { throw '独立门禁需要 -Repo，可用 -Ref 指定待检查提交或树' }
    Assert-OssGitTree -Repo $CheckRepo -Tree $CheckRef -History:$History
    Write-Output "public-paths: PASS ($CheckRef)"
}
