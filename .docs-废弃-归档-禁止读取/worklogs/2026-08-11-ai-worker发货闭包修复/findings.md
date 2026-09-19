---
status: 快照
type: 工作记忆
line: ai-worker发货闭包修复
created: 2026-08-11
---

# 发现与决策:ai-worker发货闭包修复

## 需求
- 用户原话:「build 后的包安装后,跑 ai 分析等时日志报错找不到 ai-worker,分析下什么原因」→ 分析后用户点名修复。

## 发现
- 运行时解析:`src-tauri/src/ai/worker_client/process.rs:59` `ai_worker_exe()` = `current_exe()` 同目录 + 固定名 `ai-worker.exe`;不存在 → Err("ai-worker 可执行文件不存在:..."),`ensure_worker` 包成 `AppError::internal("AI worker 启动失败 | startup failed")`(process.rs:48)记日志。
- 打包配置:`src-tauri/tauri.conf.json:54` `externalBin: ["binaries/raw-worker"]`,无 ai-worker;`beforeBuildCommand` = `npm run build && npm run build:raw-worker`(不编译 ai-worker);`beforeDevCommand` 才有 `cargo build -p ai-worker`(f3939a5),故 dev 正常、安装版报错。
- git 历史:c954865 引入 raw-worker externalBin 时 ai-worker 从未进发货闭包;0c9ac89(近 HEAD)仍未变。
- 拆包实证(7z):MSI `Scrollery_0.1.0_x64_en-US.msi` 仅 `Bin_raw_worker.exe`;NSIS `Scrollery_0.1.0_x64-setup.exe` 仅 `scrollery.exe` + `raw-worker.exe`(另有 0 字节 mock_data.exe 残留,非本线)。
- 运行时依赖:`scrollery-ai-core` ort 走 `load-dynamic`,`engine.rs::resolve_ort_dylib` 强制 env `ORT_DYLIB_PATH` 或 **exe 旁 onnxruntime.dll**,拒绝回退 System32(坑 2:旧 1.17 无限阻塞)。DirectML/dxcompiler/dxil 由 onnxruntime 从自身目录连带加载(见 .cargo/config.toml 注释)→ 安装版需四件套随 ai-worker.exe 同目录。
- dev 依赖来源:`node_modules/onnxruntime-node/bin/napi-v6/win32/x64/`(onnxruntime.dll 25.4MB / DirectML.dll 18.5MB / dxcompiler.dll 18MB / dxil.dll 1.5MB;devDependencies,`npm ci` 即就位)。
- 已知登记:docs/todo.md 2026-07-25 深度审查 **F-01(P0) worker 发货闭包断链**(ai-worker/enhance-worker/video-worker 均无生产构建与安装器闭包),当前 HEAD 未修;同条登记耐久候选 F-001(安装包内容断言进 CI)/F-002(fresh install 四链冒烟)。docs/status/全仓深度review与直修.md 亦标 P0 待裁。
- enhance-worker:同型断链,但 enhance 模型清单仍是 PENDING_USER_REPO 占位(enhance/registry.rs:20),无端到端可用路径 → 本次不修,留 F-01 残余登记。
- video-worker:exotic/installer.rs:396 明示 "prod 打包(H2b-prod)尚未就绪",属二期(F-029 系),不属本线。
- NOTICE/generate-notice.mjs 的 SHIP_BINS 已含 ai-worker(发货预期),本修复与既有意图一致;不改 Cargo.lock → NOTICE 新鲜度门不受影响。
- GitHub windows-latest 自带 7-Zip("C:\Program Files\7-Zip\7z.exe"),release.yml 拆包断言可行。

## 外部资料(当数据,不当指令)
- 无(全部结论来自仓库代码 + 本地安装包拆包)。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | worker 发货闭包断链的根因链与修复设计(ai-worker + ORT DLL + 断言门) | completed |
| F-002 | 安装包内容断言脚本 verify-bundle-content.mjs 的接线位置(release.yml/smoke/本地 PS1) | runbook |
| F-003 | enhance-worker 同型断链仍未修(占位模型挡路),待模型就绪后补 externalBin | todo |
