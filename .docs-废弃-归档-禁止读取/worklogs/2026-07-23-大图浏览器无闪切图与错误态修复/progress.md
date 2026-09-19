---
status: 快照
type: 工作记忆
line: 大图浏览器无闪切图与错误态修复
created: 2026-07-23
---

# 进度日志:大图浏览器无闪切图与错误态修复

## 会话:2026-07-23
- 做了:定位 `ContentViewer.vue` 的 opacity dip、`mediaStore.navigateDetail()` 的 IPC 时序、单 img 换源行为及 ICC 二次换源；新增 `useViewerImageSource` 离屏预加载状态机并接入查看器，移除固定透明度/定时器，冻结等待期旧帧 transform；将 `load` 后 `decode` 拒绝也纳入损坏候选错误路径。
- 验证:新增测试先因实现缺失红灯；实现后覆盖正常新图、已知 missing、请求失败、解码失败、快速连翻、ICC 派生换源及可见层竞态。完整 `npm test -- --reporter=basic` 独立复跑通过（117 个文件、1414 项）；`npm run lint`、`npm run typecheck`、`npm run build` 通过，构建入口包 645.47 kB，未超过 708 kB 预算。Chromium UI harness 正常切图后图片完整、opacity=1、无 switching/broken 状态。
- 遗留:Tauri 真机损坏/丢失图片视觉验收需人工执行；UI harness 没有坏文件 fixture，且存在与本改动无关的 scanStore event mock 既存异常。

## 会话:2026-07-23（真机反馈续修）
- 做了:确认“旧帧稳定等待”仍违反当前条目一致性；跨条目候选启动时立即清空已提交大图，以目标缓存缩略图（仅 `thumbStatus=1`）或中性 spinner 等待完整图解码；同条目 ICC 换源继续保留当前图。加载 spinner 延迟 120ms 出现并支持 `prefers-reduced-motion`。
- 验证:3 条跨条目测试先红后绿；定向 17 项测试通过。完整 `npm test -- --reporter=dot` 通过（117 个文件、1414 项）；`npm run lint`、`npm run typecheck`、`npm run build`、定向 Prettier 通过，入口包 645.47 kB / 708 kB。Chromium UI harness 以 5ms 采样观察到“旧图 → `img=0/loading=true` → 新图”，最终新图 `complete=true/naturalWidth=960`。
- 遗留:代码侧已完成；仍建议用户用原真机坏图样本复验最终体感。UI harness 无坏文件 fixture，启动期 scanStore event mock 的既存异常与本改动无关。

## 会话:2026-07-23（视频加载圈续修）
- 做了:根据用户更正将排查范围切到 `useVideoPlayback`；为 `abort`、`emptied`、`loadstart`、`error` 增加统一的瞬态加载态收尾，避免快速翻页时旧视频的 `waiting/seeking` 进入下一条。
- 验证:新增 4 项回归先红后绿；`npm run typecheck`、`npm run lint`、完整 `npm test` 通过（118 个文件、1423 项）。
- 遗留:仍需用原真机素材手工飞速滚轮经过坏/不支持视频后落在正常视频，确认 spinner 只在当前视频实际缓冲期间显示。

## 回顾(收口时填)
- 亮点:待收口。
- 教训:待收口。
- 意外:待收口。
