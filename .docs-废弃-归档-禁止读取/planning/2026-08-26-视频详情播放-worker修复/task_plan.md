---
status: 施工中
type: 工作记忆
line: 视频详情播放-worker修复
created: 2026-08-26
---

# 任务计划:视频详情播放-worker修复

## 目标
让画廊视频详情页能够稳定完成播放准备，开发版与发布版均能定位并启动 `video-worker`，且打包门禁能阻止 sidecar 再次漏发。

## 当前阶段
阶段 4:回归验证(自动化已完成，待 GUI 手动验收)

## 阶段

### 阶段 1:开发启动链路
- [x] 增加 `video-worker` 构建/启动接线，保证 `resolve_video_playback` 能完成 probe
- **状态:** completed

### 阶段 2:发布 sidecar
- [x] 增加 release sidecar 构建、Tauri `externalBin` 声明和运行期路径解析
- **状态:** completed

### 阶段 3:门禁与单测
- [x] 更新 bundle 内容校验和 worker 路径测试
- **状态:** completed

### 阶段 4:回归验证
- [ ] 运行 Rust/前端门禁并完成悬停与详情播放手动验收
- **状态:** in_progress(自动化门禁已通过，GUI 需真机点击确认)

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 补齐 `video-worker` sidecar，不把详情页临时改成绕过播放准备直接播原文件 | 详情链路还承担容器改封、转码和缓存产物职责，绕过会保留不兼容格式故障 | D-001 |
| 开发版保留 `EXOTIC_VIDEO_WORKER_PATH` 覆盖，并默认使用 target/debug 同目录 worker；发布版使用主程序同目录 sidecar | 兼容自定义 worker，同时让 Tauri dev 与 release 都能走同一 builtin worker 解析链 | D-002 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 详情页显示“播放准备失败” | 日志确认 `video_worker_unavailable`，不是浏览器 `<video>` 解码错误 | 补齐 worker 构建、启动和发布打包链路 |
| 首次加入 Tauri `externalBin` 后主程序编译提示缺少 `video-worker-<triple>.exe` | 仅 `cargo build -p video-worker` 不会把文件放入 `src-tauri/binaries` | 增加 `build:video-worker:dev`，debug 构建后同步暂存 sidecar |

## 已验证

- `npm run build:video-worker:dev`：debug worker 编译并暂存成功。
- `cargo check -p scrollery --release`、`cargo fmt --all -- --check`：通过。
- installer 单测：8 passed、2 ignored；`cargo test -p video-worker`：65 unit + 2 e2e 通过。
- `npm run tauri:build:lite`：MSI/NSIS 均生成；`node scripts/verify-bundle-content.mjs` 确认两个安装包载荷均含 `video-worker.exe`、运行库和法律资源。
