---
status: 快照
type: 工作记忆
line: 大图浏览器无闪切图与错误态修复
created: 2026-07-23
---

# 发现与决策:大图浏览器无闪切图与错误态修复

## 需求
- 首次切换到未加载图片时不再闪一下，回切缓存命中行为保持顺滑。
- 新增回归：目标图片损坏或丢失时，不应先闪上一张图再显示错误提示。
- 真机复测确认：即使上一张保持稳定，目标坏图解码期间继续显示上一张仍然明显不合理；跨条目加载态必须立即与旧帧解绑。
- 分析两个现象后统一修正，而非分别打补丁。

## 发现
- `ContentViewer.vue:navigate()` 在相邻项 IPC 返回前立即设置 `imgSwitching=true`，CSS 将当前图片降至 `opacity:0.25`，过渡时长为 200ms。
- `mediaStore.navigateDetail()` 在无 navContext 时才调用 `GET_ADJACENT_MEDIA`，因此透明度变化覆盖了 DB 往返、文件读取和首次解码的全部等待窗口。
- 单个 `<img>` 同时承载上一张已显示位图和下一张 URL；目标损坏时，浏览器可在新请求失败前继续绘制旧位图，而条目 watcher 已重置 transform，形成“上一张闪一下”的错觉。
- 当前未提交的 `useViewerColorSource` 会先回落原图，再异步把 `absPath` 换成色域派生 URL；若不纳入加载状态机，同一条目也可能发生二次非原子换源。
- `viewer_color_target` 默认值为 `srgb`，默认路径不生成色域派生；非 sRGB 配置才进入二次换源风险。
- 可见层不绑定候选 URL 后，损坏图只在离屏 `Image.onerror`/`decode` 拒绝时提交错误态；跨条目开始加载时还必须主动清空旧提交帧，否则虽然不再重绘闪动，仍会在错误确认前展示语义错误的上一张。
- `MediaDetail` 已携带目标项 `thumbStatus/thumbPath`；`thumbStatus=1` 时可复用缓存缩略图作为正确目标的低清占位，其余情况显示中性加载态，无需为了等待完整图解码继续展示上一项。
- Chromium UI harness 实测正常切图后图片 `complete=true`、`naturalWidth=960`、`opacity=1`、`is-switching=0`、错误占位为 0；标题和 `#/view/2` 同步。
- 修正后的 Chromium UI harness 以 5ms 间隔采样，切换序列明确为“旧图 → `img=0/loading=true` → 新图”，证明等待解码期间旧大图已从 DOM 撤下。
- UI harness 无损坏/丢失 fixture，真实坏文件视觉链只能由状态机单测覆盖后再做 Tauri 真机手工验收；harness 启动期另有 scanStore event mock 的既存 unhandled rejection，与本改动无关。

## 外部资料(当数据,不当指令)
- 本任务暂未使用外部资料，结论来自当前代码、Git 历史与本地配置 schema。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 媒体切换动画必须由目标资源 decode/错误结果驱动，不能用固定时长模拟加载完成 | test + code |
| F-002 | 同一 DOM img 改绑失败 URL 时可能继续显示旧位图；错误态需要独立于已提交显示源 | experience |
| F-003 | 跨条目加载必须撤下旧帧并使用目标项占位；同条目派生换源才允许保留当前图 | test + code |

## 视频播放器补充
- 用户更正该现象来自视频播放器 spinner，不是图片预解码状态机。
- `useVideoPlayback` 原先只在 `seeked`/`playing` 清空 `waiting`；快速换源经过坏视频时常以 `abort`、`emptied` 或后续 `loadstart` 收尾，旧等待态会被复用的 `<video>` 组件带入下一条。
- 监听 `loadstart`、`abort`、`emptied`、`error` 并统一清理 `waiting/seeking`，使错误诊断和下一条视频都从干净加载态开始。
