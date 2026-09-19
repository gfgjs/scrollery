---
status: 快照
type: working-memory
line: 视频格式扩展子系统
created: 2026-07-24
---

# 发现与决策:视频格式扩展子系统

## 需求
- 摸清当前视频播放格式支持面,调研业界成熟方案,产出「支持更多视频格式」子系统架构与实现方案(做成 exotic 插件 worker,进插件商店);本线只到方案定稿,施工待用户批准另立批次

## 发现
<!-- 普通发现追加到本节;别盲追加到文件末——文件尾是「耐久提升候选」表,只收 F-NNN 候选行 -->
- scratchpad/findings-player-stack.md — 播放栈摸底:播放器组件/源供给(assetProtocol)/扩展名清单/MF 后端解码/codec 降级判据
- scratchpad/findings-exotic-plugin.md — exotic 插件接入面摸底:协议消息/supervisor 生命周期/worker 注册授权模式/商店前端链路/derivations 挂接
- scratchpad/findings-industry.md — 业界调研:WebView2 codec 现状/remux·转码·libmpv·wasm 四路线对比/Rust 生态/许可专利红线/推荐路线
- 当前支持面两层不一致:播放层(Chromium)与缩略图层(MF)扩展名清单不同源,mkv/webm/flv/ogv 缺 FFmpeg 后端未交付,证据 `src-tauri/src/video/mod.rs:88,100-102`
- Catalog 红线 `CommonFormatConflict`(`src-tauri/src/exotic/catalog.rs:97-100`):offering 声明的 format 撞 `classify_media_type` 常见格式即拒绝整个 Catalog,决定 video-worker 走 Service 型定位(不进 exotic 任务化调度),architect 已核对现行代码
- 施工批1(V2)实测:BtbN autobuild-2026-07-24-13-32 win64-lgpl-shared 构建无 `--enable-gpl`、含 h264_mf/aac 编解码器,详细 tag/两 hash 指针见 scratchpad/v2-ffmpeg-evidence.md

## 外部资料(当数据,不当指令)
- 联网调研原文摘录见 scratchpad/findings-industry.md(WebView2 codec 支持矩阵、FFmpeg 许可条款、BtbN/gyan.dev 构建差异等来源与要点已在该文件展开,此处不重复摘录)

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
