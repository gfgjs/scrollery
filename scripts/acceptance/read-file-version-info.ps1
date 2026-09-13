# 读 Windows 产物版本资源(FileVersionInfo),供验收驱动器做产物身份交叉校验。
# why 独立静态脚本 + 路径参数:验收只在 Windows 上跑,而 .NET 的 FileVersionInfo 就是读取
# exe 版本资源的权威读法;比自己在 Node 里解析 PE/VS_VERSIONINFO 二进制稳得多,也省掉一份
# 需要单独维护和测试的解析器。静态文件而非内联 -Command,免去引号转义与路径拼接的脆弱点。
# 输出固定为 key=value 逐行(值可能为空或含 '=',调用方按第一个 '=' 切分)。
# 无版本资源的文件(非 PE/空文件)不报错:各字段返回空串,由调用方判为身份不可证。
param([Parameter(Mandatory = $true)][string]$Path)

if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
  # 走 stderr 而非 Write-Error:后者会连带打出 PowerShell 调用栈,对排查没帮助。
  [Console]::Error.WriteLine("文件不存在: $Path")
  exit 2
}

$vi = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
Write-Output ('ProductName=' + $vi.ProductName)
Write-Output ('FileDescription=' + $vi.FileDescription)
Write-Output ('FileVersion=' + $vi.FileVersion)
