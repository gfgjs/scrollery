# scripts/build-internal-installer.ps1
# 内测安装包构建(2026-07-05 内测链)——把两个部署配置以编译期注入方式烘焙进 Release 安装包:
#   PICASA_EXOTIC_KEYSET_FILE     → exotic 信任根 = 占位生产集 + 内测键超集(build.rs 装配)
#   PICASA_REGISTRY_BASE_DEFAULT  → 插件商店默认发行源 = 内测公开仓 raw 直链(option_env!)
# 产物:target/release/bundle/{msi,nsis}/ 下的 MSI 与 NSIS 安装器(基座 conf,无 updater)。
# 前置:node scripts/exotic-internal-registry.mjs(生成 keyset;三件套须已推到发行源)。
#
# ⚠ 本脚本产出的安装包信任根是**内测键**,不得作为正式渠道分发(正式信任根/发行源归 Part8)。

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$keyset = Join-Path $repo '.internal-signing\internal-keyset.json'
if (-not (Test-Path $keyset)) {
    throw "缺 $keyset —— 先运行:node scripts/exotic-internal-registry.mjs"
}
$env:PICASA_EXOTIC_KEYSET_FILE = $keyset
if (-not $env:PICASA_REGISTRY_BASE_DEFAULT) {
    $env:PICASA_REGISTRY_BASE_DEFAULT = 'https://raw.githubusercontent.com/gfgjs/scrollery-registry/main/exotic/v1'
}
Write-Host "内测注入 keyset   : $env:PICASA_EXOTIC_KEYSET_FILE"
Write-Host "内测注入 registry : $env:PICASA_REGISTRY_BASE_DEFAULT"

Push-Location $repo
try {
    npx tauri build
    if ($LASTEXITCODE -ne 0) { throw "tauri build 失败(exit=$LASTEXITCODE)" }

    # 发货闭包断言(F-001 候选落地,2026-08-11):ai-worker sidecar + ORT 四件套必须落进安装包。
    # 安装包内容校验必须有安装包及解包工具，不将 staging 检查当作发行验收。
    node scripts/verify-bundle-content.mjs --require-bundle
    if ($LASTEXITCODE -ne 0) { throw "安装包内容断言失败(exit=$LASTEXITCODE)" }
}
finally {
    Pop-Location
}

# 列出安装器产物(路径即交付物;后续人工分发给内测者)。
Get-ChildItem -Recurse (Join-Path $repo 'target\release\bundle') -File |
    Where-Object { $_.Name -like '*.msi' -or $_.Name -like '*-setup.exe' } |
    ForEach-Object { Write-Host ("产物: " + $_.FullName + "  (" + [math]::Round($_.Length / 1MB, 1) + " MB)") }
