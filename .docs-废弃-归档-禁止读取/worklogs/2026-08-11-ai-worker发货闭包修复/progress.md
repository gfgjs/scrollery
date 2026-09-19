---
status: 快照
type: 工作记忆
line: ai-worker发货闭包修复
created: 2026-08-11
---

# 进度日志:ai-worker发货闭包修复

## 会话:2026-08-11
- 做了:现状调研(运行时报错链路、tauri.conf.json、git 历史、安装包拆包、ort 依赖、CI 结构),证据固化进 findings.md。
- 验证:`7z l` 拆 MSI/NSIS 实证无 ai-worker;`rg` 定位 ai_worker_exe/enhance_worker_exe/resolve_ort_dylib;git log 核实 externalBin 历史。
- 遗留:阶段 2 施工(脚本 + conf + CI 接线)。

## 会话:2026-08-11(施工与验证)
- 做了:新增 scripts/build-ai-worker.mjs(release 编译 + staging exe + ORT 四件套)、scripts/verify-bundle-content.mjs(三层断言 + selftest);tauri.conf.json externalBin/resources;package.json build:ai-worker;release.yml/ci.yml smoke/build-internal-installer.ps1 接线;docs/todo.md + docs/status 回写。
- 验证:
  - `npm run build:ai-worker` → 22.7s 编译,staging 6 文件齐(ai-worker-x86_64-pc-windows-msvc.exe 6.5MB + 4 DLL)。
  - `npx tauri build --bundles nsis` → 成功;`7z l` 拆包 15 条目,scrollery.exe/ai-worker.exe/raw-worker.exe + onnxruntime/DirectML/dxcompiler/dxil 全在根目录;`verify-bundle-content.mjs` 对新 NSIS「✓ 安装包载荷完整」,对旧 MSI 精确报缺失 5 项(证明断言有效)。
  - `node --check` 两脚本 0 错误;verify selftest 内置通过。
  - 全量 `npx tauri build`(msi+nsis)卡在 WiX light:`拒绝访问 (os error 5)`——卡死 msiexec(PID 19504,Session 0,CPU 0,无 InProgress 事务键)锁住旧 MSI(rename 实证 Access denied);MSI 本地重建阻塞,CI 干净 runner 不受影响。
- 遗留:MSI 本地重建需锁释放(杀卡死 msiexec 或重启);enhance-worker/video-worker 断链残余(F-003)。

## 会话:2026-08-11(收口)
- 做了:closeout.md 逐候选处置;git mv 至 worklogs;worklogs README 登记;todo/status 回写;两 commit(实现 + 收口)。

## 回顾(收口时填)
- 亮点:发货闭包这类「只有真打包才能证伪」的断链,修完当场拆包断言(7z 载荷核对)+ 把断言写进 CI/本地脚本,杜绝 `cargo test` 绿灯漏检;断言脚本先喂正反样本 selftest 防空壳。
- 教训:externalBin/resources 是静态配置,侧车与 DLL 必须由构建脚本统一 staging 并硬失败,dev-only 构建(beforeDevCommand)与发货构建(beforeBuildCommand)的不对称正是断链温床。
- 意外:①ORT 走 load-dynamic,光带 ai-worker.exe 不够,必须四件 DLL 同目录(resolve_ort_dylib 拒绝回退 System32);②MSI 会被卡死 msiexec 锁住,本地重建直接 os error 5;③Tauri NSIS 会把 target/release 里的额外 exe(mock_data)带进包,属既有行为。
