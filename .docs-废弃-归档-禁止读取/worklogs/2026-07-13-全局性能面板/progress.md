---
status: snapshot
type: working-memory
line: 全局性能面板
created: 2026-07-13
---

# 进度日志:全局性能面板

## 会话:2026-07-13
- 做了:完成源码复核与基线；实现固定容量 recorder、刷新率自校准、静默/实时录制、历史与 JSON 导出、全局快捷键和设置入口。
- 做了:接入画廊标准往返基准；为画廊 draw/bitmap/Image fallback/cache 及时间轴 draw/static/loupe 添加仅录制期生效的埋点。
- 验证:全量 Vitest 71 files / 855 tests、vue-tsc --noEmit、ESLint、生产 build、docs/index/canonical 三项门禁全部通过；源码提交 81cef21。
- 验证:应用内浏览器 skill 已按流程尝试，但 Node 内核被 apply deny-read ACLs 终止，未取得 UI 交互证据。
- 遗留:应用内浏览器交互证据因 ACL 沙箱初始化故障未取得；用户可在 Release 构建、关闭 DevTools 后以 Ctrl+Shift+P 运行静默基准做真机终证。
- 保护:未触碰任务外未提交文件 docs/experience.md。

## 回顾
- 亮点:把采集器和面板渲染彻底分离，静默基准不做响应式快照；高刷新率帧预算、Canvas 专项跨度和标准往返形成可重复证据链。
- 教训:Canvas draw 计时只能表示 JavaScript 提交成本，不能替代 GPU 完成时间；绝对对比必须在同一 Release 环境关闭 DevTools。
- 意外:工作区少数旧归档 ACL 导致内建 apply_patch、普通 exec 和应用内浏览器内核共同失效；经授权只用精确 unified diff 施工，并保留未验证边界。
