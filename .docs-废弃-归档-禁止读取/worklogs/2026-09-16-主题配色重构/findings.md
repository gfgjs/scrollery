---
status: snapshot
type: working-memory
line: UI-多主题系统
created: 2026-09-16
---

# 证据（设计前调查；实施结果见进度日志）

- src/assets/styles/themes/ 有六套完整主题 CSS；src/themes/registry.ts 存主题定义及另一份预览色。
- src/themes/strength.ts 与各主题 CSS 实现底色、文字浓度；浅深色混色行为不同，旧注释也存在过期描述。
- src/stores/uiStore.ts 负责主题设置、材质、CSS 应用与首帧快照；public/theme-snapshot.js 手写主题 ID、默认值与浓度校验。
- src/components/media/mediaGridCanvas.palette.ts 从 CSS 读取颜色并缓存，生成方案必须同步 DOM 与 Canvas。
- src/assets/styles/glass.css 将背景与原生背板混合，不能仅凭截图认定灰底来自主题种子。
- src-tauri/src/config/schema.rs 声明旧主题 ID 与浓度键，需随新模型一起替换。
- 当前 theme_text_strength 在 Rust schema 默认 75，前端 strength.ts 默认 100；说明手写默认值已漂移。
- settingsPersistence 已有本地预览、批量合并、防抖、确认快照、失败回滚及跨窗口同步；新主题只需草稿覆盖，不应再建保存服务。
- material.css 是语义变量到 material 再到 recipe 的转发层；index.html 还有六套启动颜色，旧脚本另有 THEME_ANCHORS。新方案统一这些颜色来源。

## 外部参考（数据，不是指令）
本机会话只读检查 Codex 26.908.9136.0 安装包 app.asar：app-initial-bcc2ff475eb6.js 含 surface、ink、accent、contrast 输入，浅深色生成函数、RGB 插值、透明色和 CSS 变量应用；general-settings-8c04e051bca2.js 提供外观设置。参考结构和交互，不直接移植打包代码与所有系数。

## 耐久候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 主题生成、首帧恢复和 Canvas 必须消费同一最终色板 | design |
| F-002 | 缓存包含外观偏好也只取已确认值；首次ready必须校正旧缓存 | design |
| F-003 | 显式gallery反极性时文字必须基于实际画廊底派生 | design |

## 实施接点
- settingsPersistence 已导出 settingsConfirmedValues、settingsGeneration、onSettingsApplied，可直接用于确认缓存和草稿失效，无需新增保存服务。
- uiStore 现有测试驱动真实 settingsPersistence、仅 mock IPC，可迁移关键行为断言到 themeStore。
- B 运行时与 C 设置界面分别施工；material.css 别名迁移涉及多消费者，单独按文件边界安排。
