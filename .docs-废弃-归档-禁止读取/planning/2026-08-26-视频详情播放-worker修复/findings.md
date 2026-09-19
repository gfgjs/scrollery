---
status: 施工中
type: 工作记忆
line: 视频详情播放-worker修复
created: 2026-08-26
---

# 发现与决策:视频详情播放-worker修复

## 需求
- 用户确认修复画廊视频悬停预览正常、点击详情后显示“播放准备失败”的问题。

## 发现
- 悬停预览通过 `useHoverPreview` 直接解析原视频 asset URL，不依赖 `video-worker`。
- 详情页 `useVideoSource` 首先调用 `resolve_video_playback`，后端 `resolve_impl` 在缓存未命中时先调用 `VideoWorkerService::probe`。
- 本机 2026-08-25 日志记录 `video_worker_unavailable`，同一时段此前记录开发环境未设置 `EXOTIC_VIDEO_WORKER_PATH`。
- `target/debug/video-worker.exe` 存在，但 `src-tauri/tauri.conf.json` 的 `beforeDevCommand` 没有构建/注入该 worker。
- `src-tauri/tauri.conf.json` 当前 `externalBin` 只有 `raw-worker` 和 `ai-worker`；发布版 `installer.rs` 对 `video-extended` 明确返回不可用，导致发布包同样无法启动 worker。
- `scripts/verify-bundle-content.mjs` 当前只检查 `ai-worker`、`raw-worker` 及 AI 运行库，没有视频 sidecar 断链检查。

## 实现结果

- 新增 `scripts/build-video-worker.mjs`：Windows 下支持 debug/release 两种 profile，按 Rust host triple 暂存到 `src-tauri/binaries`。
- `beforeDevCommand` 改为先执行 `build:video-worker:dev`；开发 resolver 保留 env 覆盖，并自动找主程序 `target/debug` 同目录的 `video-worker.exe`。
- `beforeBuildCommand`、`externalBin` 和 release resolver 已接入 `video-worker`；bundle 校验覆盖 staging、配置、MSI/NSIS 载荷和法律资源。
- 实际 lite bundle 已生成 MSI/NSIS，两个安装包的 sidecar 载荷检查均通过。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | builtin sidecar 必须同时具备构建脚本、Tauri externalBin、运行期路径解析和 bundle 门禁 | test/runbook |
