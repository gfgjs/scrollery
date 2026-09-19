---
id: 2026-08-11-status-ai-worker发货闭包修复
status: active
type: rolling-status
line: ai-worker发货闭包修复
created: 2026-08-11
---

# ai-worker发货闭包修复 · 滚动状态

- ✅ ai-worker 断链已修(2026-08-11):`bundle.externalBin` 补 `binaries/ai-worker`;`bundle.resources` 声明 ORT 四件套(onnxruntime/DirectML/dxcompiler/dxil);`scripts/build-ai-worker.mjs` 挂 beforeBuildCommand 生产构建暂存;`scripts/verify-bundle-content.mjs`(三层断言:staging/conf/7z 拆包载荷,内建 selftest)接线 release.yml(拆包硬验)/ci.yml smoke(staging+conf)/build-internal-installer.ps1。本地重建 NSIS 拆包断言通过(15 条目,ai-worker.exe + 四 DLL 同根目录)。
- ⬜ F-003 残余:enhance-worker 同型断链未修——模型清单仍 `PENDING_USER_REPO` 占位(enhance/registry.rs),无端到端可用路径;待模型就绪后按同模式补 externalBin + staging(与 ai-worker 共用四 DLL)。
- ✅ video-worker prod 闭包已补齐(2026-08-26):`bundle.externalBin`、`build:video-worker` release 暂存、builtin 运行期路径解析和 `verify-bundle-content.mjs` 均已接线；本地 MSI/NSIS 实包校验确认 `video-worker.exe`、四项 AI runtime 与法律资源齐全。GUI 真机播放仍待验收。
- ⬜ MSI 本地重建被卡死 msiexec(锁旧 MSI,无活动事务)阻塞;CI 干净 runner 不受影响,发布流水线产出 MSI+NSIS 双包。
