---
status: 快照
type: 工作记忆
line: ai-worker发货闭包修复
created: 2026-08-11
---

# 任务计划:ai-worker发货闭包修复

## 目标
`tauri build` 产出的安装包内必须包含 `ai-worker.exe` 及 ORT 运行时四件套 DLL(onnxruntime/DirectML/dxcompiler/dxil),使安装版 AI 分析/人脸/OCR 能拉起 worker;并在 CI/本地打包路径加安装包内容断言防回潮。

## 当前阶段
阶段 2:施工

## 阶段

### 阶段 1:现状调研与证据固化
- [x] 定位运行时报错链路:ai_worker_exe() 同目录查找 → 找不到报 "ai-worker 可执行文件不存在"
- [x] 确认打包配置:externalBin 仅 raw-worker;beforeBuildCommand 不编译 ai-worker
- [x] 拆包实证:MSI 仅 Bin_raw_worker.exe;NSIS 仅 scrollery.exe + raw-worker.exe
- [x] 确认运行时依赖:ort load-dynamic 需 exe 旁 onnxruntime.dll 四件套(engine.rs resolve_ort_dylib)
- [x] 确认此即 2026-07-25 深度审查 F-01(P0),当前 HEAD 未修
- **状态:** completed

### 阶段 2:施工
- [x] 新增 scripts/build-ai-worker.mjs:release 编译 ai-worker + 暂存 exe 到 src-tauri/binaries + 复制 ORT 四件套
- [x] tauri.conf.json:externalBin 加 binaries/ai-worker;resources 声明四个 DLL
- [x] package.json 加 build:ai-worker 脚本;beforeBuildCommand 接线
- [x] 新增 scripts/verify-bundle-content.mjs(含 selftest):断言 staging 文件 + conf 接线;有安装包时拆包断言
- [x] release.yml / ci.yml smoke / build-internal-installer.ps1 接线断言
- **状态:** completed

### 阶段 3:验证
- [x] npm run build:ai-worker 产出 staging 文件(ai-worker-x86_64-pc-windows-msvc.exe + 四 DLL)
- [x] 本地 npx tauri build 重建 NSIS 安装包,7z 拆包 15 条目含 ai-worker.exe + 四 DLL,断言通过
- [x] 相关检查门(node --check 两脚本、verify selftest 内置)跑绿;rust 无改动面
- [x] 边界记录:MSI 本地重建被卡死 msiexec(锁旧 MSI,无活动事务)阻塞;CI 干净 runner 不受影响
- **状态:** completed

### 阶段 4:文档与收口
- [x] docs/todo.md 回写 F-01(ai-worker)已修 + F-001 候选落地;docs/status/全仓深度review与直修.md P0 行更新
- [x] 三件套收口:closeout.md、git mv 到 worklogs、worklogs README 登记、commit
- **状态:** completed

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 修复范围=ai-worker(+ORT DLL),enhance-worker 本次不修 | enhance 模型清单仍是 PENDING 占位,无端到端可用路径;避免无效体积/CI 成本 | D-001 |
| ORT DLL 走 bundle.resources 而非 externalBin | externalBin 只按 exe triple 命名分发;resources 是资源的标准通道 | D-002 |
| 断言脚本独立于 verify-channel-bundle | 后者是渠道合规扫描(conf/dist),发货闭包是打包面,职责不同 | D-003 |
| 非 Windows 跳过 staging(镜像 raw-worker 先例) | mac/Linux sidecar 打包归 F-06 剩余工作,不阻断跨平台 dev/build | D-004 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 安装版 AI 分析报 "ai-worker 可执行文件不存在" | —(接手时已定位) | 发货闭包补全(本线) |
