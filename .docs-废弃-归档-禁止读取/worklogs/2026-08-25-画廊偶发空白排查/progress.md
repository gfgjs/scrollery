---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-25
---

# 进度日志:画廊偶发空白排查

## 会话:2026-08-25
- 做了:建立排查记录，检查 `MediaGrid`、`MediaGridCanvas`、双虚拟滚动引擎、历史 Canvas 工作线与测试。
- 做了:用项目安装的 Vue runtime 运行最小 KeepAlive 树，实际输出 `mounted -> activated`；与源代码的 generation/rAF 合并逻辑对照，复现“旧帧作废、新帧未排”的确定时序。
- 验证:`npm test -- src/components/media/mediaGridCanvas.helpers.spec.ts` 通过，1 个测试文件、62 项测试全部通过；该集未覆盖组件级竞态。
- 验证:独立 Canvas 生命周期复核得出相同根因；缩略图加载失败与持续 0 尺寸均无法更好解释轻微滚动即恢复的症状。
- 遗留:产品代码尚未修复。下一步应在 `MediaGridCanvas` 让 generation 变化时替换旧 rAF 或确保激活后强制排入当前代次帧，并新增回归测试。

## 回顾(收口时填)
- 亮点:以 Vue 的实际 KeepAlive 生命周期顺序验证了静态代码推断，而非仅依据症状猜测。
- 教训:只测试生命周期代次对象不足以覆盖其与 rAF 去重器的组合时序。
- 意外:为防止失活旧帧写入而加入的 generation guard，反过来暴露了首帧“被正确拒绝但未补发”的漏洞。
