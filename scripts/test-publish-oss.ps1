# 离线回归：仅使用临时合成仓库与扫描器替身，不连接远端。
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'oss-projection-policy.ps1')

function Assert-True {
    param([bool] $Condition, [string] $Message)
    if (-not $Condition) { throw $Message }
}

function Assert-Rejected {
    param([scriptblock] $Action, [string] $Expected)
    try { & $Action }
    catch {
        if ($_.Exception.Message -notlike "*$Expected*") { throw }
        return
    }
    throw "应拒绝操作:$Expected"
}

function Write-Fixture {
    param([string] $Root, [string] $Relative, [byte[]] $Bytes)
    $path = Join-Path $Root $Relative
    Assert-OssSafePath -Root $Root -Path $path
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    [IO.File]::WriteAllBytes($path, $Bytes)
}

function New-FixtureCommit {
    param([string] $Repo)
    Invoke-OssGitText $Repo @('-c', 'core.autocrlf=false', 'add', '-f', '-A') | Out-Null
    $tree = (Invoke-OssGitText $Repo @('write-tree')).Trim()
    $commit = (Invoke-OssGitText $Repo @('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
        'commit-tree', $tree, '-m', 'synthetic fixture')).Trim()
    Invoke-OssGitText $Repo @('update-ref', 'HEAD', $commit) | Out-Null
    return $commit
}

function New-FixtureNewlineTree {
    param([string] $Repo, [string] $Blob)
    # 直接构造 Git 树，Windows 无需创建含换行的实体文件。
    $start = [Diagnostics.ProcessStartInfo]::new('git')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($argument in @('-C', $Repo, 'mktree', '-z')) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $process.StandardInput.Write("100644 blob $Blob`tsource`nAGENTS.md`0")
        $process.StandardInput.Close()
        $output = $process.StandardOutput.ReadToEnd()
        $errorOutput = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        Assert-True ($process.ExitCode -eq 0) "换行路径树构造失败:$errorOutput"
        return $output.Trim()
    }
    finally { $process.Dispose() }
}

$temp = Join-Path ([IO.Path]::GetTempPath()) ('oss-policy-test-' + [guid]::NewGuid().ToString('N'))
$source = Join-Path $temp 'source'
$target = Join-Path $temp 'target'
$work = Join-Path $temp 'work'
$outside = Join-Path $temp 'work-other'
$link = Join-Path $temp 'junction'
$pwsh = (Get-Command pwsh -ErrorAction Stop).Source
$scanLogBefore = $env:OSS_TEST_SCAN_LOG
$injectBefore = $env:OSS_TEST_INJECT_PATH
New-Item -ItemType Directory -Path $source, $target, $work, $outside | Out-Null
try {
    Invoke-OssGitText $source @('init', '--quiet') | Out-Null
    Invoke-OssGitText $target @('init', '--quiet') | Out-Null
    Invoke-OssGitText $target @('remote', 'add', 'origin', 'https://github.com/gfgjs/scrollery.git') | Out-Null
    $docs = @(
        'docs/CHANGELOG.md', 'docs/development.md', 'docs/legal/LICENSE', 'docs/legal/NOTICE.md',
        'docs/legal/SOURCE.md', 'docs/legal/COMMERCIAL.md', 'docs/legal/ADDITIONAL-PERMISSION.md',
        'docs/legal/CLA.md', 'docs/legal/TRADEMARK.md'
    )
    $sources = @('src/main.ts', 'src/with [brackets].ts', 'src/docs/parser.ts', 'README.md',
        '.github/workflows/ci.yml', '.github/.hidden-source', '.gitignore')
    $blocked = @(
        '.docs-废弃-归档-禁止读取/nested/private.md', '.docs-[archive]/.hidden', '.docs-/private.md',
        'docs/tasks/nested/internal.md', 'docs/.hidden', 'docs/legal/internal.md',
        'copy.bara.sky', '.github/workflows/sync-oss.yml', 'scripts/publish-oss.ps1',
        '.agents/skills/.hidden', '.github/copilot-instructions.md', '.github/instructions/nested/a.md',
        'AGENTS.md', '.zcode/.hidden', '.worklog/tasks/one.md', '.worklogrc.jsonc',
        '.pnpm-store/.hidden', '.research-tmp/nested/a', 'scripts/install-skills.mjs'
    )
    $expected = @{}
    foreach ($relative in ($docs + $sources)) {
        # 同时覆盖 BOM、CRLF、中文与二进制字节，检查过程按字节比较。
        $bytes = [byte[]] (@(0xef, 0xbb, 0xbf) + [Text.Encoding]::UTF8.GetBytes("# $relative`r`n中文`n") + @(0, 0xff, 0x80))
        Write-Fixture $source $relative $bytes
        $expected[$relative] = [Convert]::ToBase64String($bytes)
    }
    foreach ($relative in $blocked) { Write-Fixture $source $relative ([byte[]] @(112, 114, 105, 118, 97, 116, 101)) }
    # Windows 隐藏属性和点文件均须处理，不能依赖普通目录枚举。
    if ([IO.Path]::DirectorySeparatorChar -eq '\') {
        foreach ($relative in @('docs/.hidden', '.github/.hidden-source', '.docs-[archive]/.hidden')) {
            $item = Get-Item -LiteralPath (Join-Path $source $relative) -Force
            $item.Attributes = $item.Attributes -bor [IO.FileAttributes]::Hidden
        }
    }
    $sourceCommit = New-FixtureCommit $source
    # 未提交内容必须留在源仓；发布只取固定提交。
    Write-Fixture $source 'docs/development.md' ([byte[]] @(100, 105, 114, 116, 121))
    $sourceStatus = Invoke-OssGitText $source @('status', '--porcelain=v1', '-z')
    Export-OssProjection -Repo $source -Tree $sourceCommit -WorkRoot $work
    $actual = @(Get-ChildItem -LiteralPath $work -File -Force -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($work, $_.FullName).Replace('\', '/') })
    $difference = @(Compare-Object -ReferenceObject ($docs + $sources) -DifferenceObject $actual -CaseSensitive)
    Assert-True ($difference.Count -eq 0) "投影文件集合错误:$($difference | Out-String)"
    foreach ($relative in ($docs + $sources)) {
        Assert-True ([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $work $relative))) -ceq $expected[$relative]) "文件字节改变:$relative"
    }
    Write-Output 'PASS: 提交路径投影、全部排除项、隐藏文件、9 项文档与允许源码逐字节保留'

    foreach ($relative in ($blocked + @('.DoCs-other/a', 'Docs/development.md', 'docs/legal/LICENSE/child'))) {
        Assert-Rejected { Assert-OssPublicPaths -Paths @('src/main.ts', $relative) } '禁发路径'
    }
    foreach ($relative in @('../outside/a', '/absolute', 'src/../../a', 'src\alias', 'docs./private', 'src/.git/config')) {
        Assert-Rejected { Test-OssPublicPath $relative | Out-Null } '非法投影路径'
    }
    Assert-Rejected { Assert-OssSafePath -Root $work -Path (Join-Path $outside 'sentinel') } '越界'
    Assert-Rejected { Assert-OssSafePath -Root $work -Path (Join-Path $work '../work-other/sentinel') } '越界'
    Write-Fixture $outside 'sentinel' ([byte[]] @(1, 2, 3))
    $linkType = if ([IO.Path]::DirectorySeparatorChar -eq '\') { 'Junction' } else { 'SymbolicLink' }
    New-Item -ItemType $linkType -Path $link -Target $outside | Out-Null
    try {
        Assert-Rejected { Assert-OssSafePath -Root $temp -Path (Join-Path $link 'sentinel') } '链接'
        Assert-Rejected { Assert-OssSafePath -Root $link -Path $link -AllowRoot } '链接'
        Assert-Rejected { Assert-OssSafePath -Root $temp -Path $temp -AllowRoot -Recurse } '链接'
    }
    finally { [IO.Directory]::Delete($link) }
    Assert-True ([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $outside 'sentinel'))) -eq 'AQID') '越界文件改变'
    # 合成 symlink 模式，无需在源仓创建实际链接。
    $blob = (Invoke-OssGitText $source @('rev-parse', "${sourceCommit}:src/main.ts")).Trim()
    Invoke-OssGitText $source @('update-index', '--add', '--cacheinfo', "120000,$blob,src/link") | Out-Null
    $linkTree = (Invoke-OssGitText $source @('write-tree')).Trim()
    $linkWork = Join-Path $temp 'link-work'
    New-Item -ItemType Directory -Path $linkWork | Out-Null
    Assert-Rejected { Export-OssProjection -Repo $source -Tree $linkTree -WorkRoot $linkWork } '普通文件'
    Invoke-OssGitText $source @('read-tree', $sourceCommit) | Out-Null
    Write-Output 'PASS: 禁发/畸形路径拒绝、相邻前缀/穿越/junction 边界、公开 symlink 拒绝'

    # 扫描器替身只验证编排与硬闸，真实 gitleaks 的调用参数保持现行规则。
    $scanner = Join-Path $temp 'gitleaks-fixture.ps1'
    @'
Add-Content -LiteralPath $env:OSS_TEST_SCAN_LOG -Value ($args -join ' ')
if ($env:OSS_TEST_INJECT_PATH -and $args[0] -eq 'dir') {
    $path = Join-Path $args[1] $env:OSS_TEST_INJECT_PATH
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    [IO.File]::WriteAllText($path, 'injected by fixture')
}
exit 0
'@ | Set-Content -LiteralPath $scanner -Encoding utf8
    $env:OSS_TEST_SCAN_LOG = Join-Path $temp 'scans.log'
    $env:OSS_TEST_INJECT_PATH = ''
    Write-Fixture $target '.docs-old/previous.md' ([byte[]] @(111, 108, 100))
    Write-Fixture $target 'obsolete.txt' ([byte[]] @(111, 108, 100))
    New-FixtureCommit $target | Out-Null
    $publish = Join-Path $PSScriptRoot 'publish-oss.ps1'
    $output = & $pwsh -NoProfile -File $publish -SourceRepo $source -Ref $sourceCommit -TargetRepo $target -GitleaksPath $scanner 2>&1
    Assert-True ($LASTEXITCODE -eq 0) "发布编排失败:$output"
    Assert-OssGitTree -Repo $target -Tree HEAD
    $targetPaths = @(Get-OssGitEntries $target HEAD | ForEach-Object { $_.Path })
    $difference = @(Compare-Object ($docs + $sources) $targetPaths -CaseSensitive)
    Assert-True ($difference.Count -eq 0) "目标提交树错误:$($difference | Out-String)"
    foreach ($relative in ($docs + $sources)) {
        Assert-True ([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $target $relative))) -ceq $expected[$relative]) "目标字节改变:$relative"
    }
    Assert-True (@(Get-Content -LiteralPath $env:OSS_TEST_SCAN_LOG).Count -eq 3) '扫描器调用次数错误'
    $head = (Invoke-OssGitText $target @('rev-parse', 'HEAD')).Trim()
    $output = & $pwsh -NoProfile -File $publish -SourceRepo $source -Ref $sourceCommit -TargetRepo $target -GitleaksPath $scanner 2>&1
    Assert-True ($LASTEXITCODE -eq 0 -and "$output" -like '*no-change:*') "无变更编排失败:$output"
    Assert-True ((Invoke-OssGitText $target @('rev-parse', 'HEAD')).Trim() -eq $head) '无变更新增了提交'
    Assert-True ((Invoke-OssGitText $source @('status', '--porcelain=v1', '-z')) -ceq $sourceStatus) '源仓状态改变'
    Write-Output 'PASS: 发布编排、清除目标旧档案、扫描调用、无变更复用、源仓只读'

    $gate = Join-Path $PSScriptRoot 'oss-projection-policy.ps1'
    $output = & $pwsh -NoProfile -File $gate -Repo $target -Ref HEAD 2>&1
    Assert-True ($LASTEXITCODE -eq 0) "独立门禁误拒:$output"
    $output = & $pwsh -NoProfile -File $gate -Repo $source -Ref $sourceCommit 2>&1
    Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*禁发路径*') "独立门禁漏拒:$output"

    # HEAD 已删档案，祖先仍保留；模拟旧公开 clone 的历史被合并回来。
    $output = & $pwsh -NoProfile -File $gate -Repo $target -Ref HEAD -History 2>&1
    Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*.docs-old/previous.md*') "历史门禁漏拒已删除的祖先档案:$output"
    $cleanTree = (Invoke-OssGitText $target @('rev-parse', 'HEAD^{tree}')).Trim()
    $cleanCommit = (Invoke-OssGitText $target @('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
        'commit-tree', $cleanTree, '-m', 'clean root')).Trim()
    $output = & $pwsh -NoProfile -File $gate -Repo $target -Ref $cleanCommit -History 2>&1
    Assert-True ($LASTEXITCODE -eq 0) "历史门禁误拒干净历史:$output"
    # 浅克隆截断祖先，完整历史门禁必须拒绝而不是跳过缺失提交。
    $shallowFile = Join-Path $target '.git/shallow'
    Assert-OssSafePath -Root $target -Path $shallowFile
    [IO.File]::WriteAllText($shallowFile, $cleanCommit + "`n", [Text.UTF8Encoding]::new($false))
    try {
        $output = & $pwsh -NoProfile -File $gate -Repo $target -Ref $cleanCommit -History 2>&1
        Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*拒绝浅克隆*') "历史门禁接受浅克隆:$output"
    }
    finally { Remove-Item -LiteralPath $shallowFile }
    $mergeCommit = (Invoke-OssGitText $target @('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
        'commit-tree', $cleanTree, '-p', $cleanCommit, '-p', $head, '-m', 'merge old public history')).Trim()
    Assert-OssGitTree -Repo $target -Tree $mergeCommit
    $output = & $pwsh -NoProfile -File $gate -Repo $target -Ref $mergeCommit -History 2>&1
    Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*.docs-old/previous.md*') "历史门禁漏查合并侧支:$output"

    $newlineTree = New-FixtureNewlineTree -Repo $source -Blob $blob
    $newlineCommit = (Invoke-OssGitText $source @('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
        'commit-tree', $newlineTree, '-m', 'newline path')).Trim()
    $newlinePaths = @(Get-OssGitEntries $source $newlineCommit)
    Assert-True ($newlinePaths.Count -eq 1 -and $newlinePaths[0].Path -ceq "source`nAGENTS.md") '换行路径被拆记录'
    $output = & $pwsh -NoProfile -File $gate -Repo $source -Ref $newlineCommit -History 2>&1
    Assert-True ($LASTEXITCODE -eq 0) "历史门禁误拆换行路径:$output"

    # 接收仓仅在本地，硬闸失效时也不会访问真实公开远端。
    $receiver = Join-Path $temp 'receiver.git'
    New-Item -ItemType Directory -Path $receiver | Out-Null
    Invoke-OssGitText $receiver @('init', '--bare', '--quiet') | Out-Null
    Invoke-OssGitText $target @('config', 'remote.origin.pushurl', $receiver) | Out-Null
    $output = & $pwsh -NoProfile -File $publish -SourceRepo $source -Ref $sourceCommit -TargetRepo $target -GitleaksPath $scanner -Push 2>&1
    Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*.docs-old/previous.md*') "推送前历史门禁漏拒:$output"
    Assert-True ([string]::IsNullOrWhiteSpace((Invoke-OssGitText $receiver @('for-each-ref', '--format=%(refname)')))) '历史门禁拒绝后仍推送'
    Write-Output 'PASS: 干净历史接受、祖先已删除档案及合并侧支拒绝、换行路径保留、Push 前历史阻断'

    $env:OSS_TEST_INJECT_PATH = '.docs-injected/blocked.md'
    $output = & $pwsh -NoProfile -File $publish -SourceRepo $source -Ref $sourceCommit -TargetRepo $target -GitleaksPath $scanner 2>&1
    Assert-True ($LASTEXITCODE -ne 0 -and "$output" -like '*禁发路径*') "暂存树硬闸漏拒:$output"
    Assert-True ((Invoke-OssGitText $target @('rev-parse', 'HEAD')).Trim() -eq $head) '硬闸失败后仍提交'
    Write-Output 'PASS: 独立门禁接受/拒绝、注入禁发路径时暂存树阻断提交'
}
finally {
    $env:OSS_TEST_SCAN_LOG = $scanLogBefore
    $env:OSS_TEST_INJECT_PATH = $injectBefore
    Assert-OssSafePath -Root ([IO.Path]::GetTempPath()) -Path $temp -Recurse
    Remove-Item -LiteralPath $temp -Recurse -Force
}

# 预期拒绝会留下非零原生命令状态，成功完成时显式返回给 CI。
exit 0
