---
status: 施工中
type: 工作记忆
line: UI-多主题系统
created: 2026-08-21
---

# 任务计划:UI清透感与材质体验升级

## 目标

在保持既有业务逻辑、交互契约与性能底线的前提下，针对 UI 整体“闷、碎、平、脏、噪”的痛点进行系统级材质升级，打造具备光感透叠、大表面收敛、呼吸感留白与低噪声边界的现代化清透视觉体验。

## 当前阶段

阶段 4:全量门禁与六主题对比度验证（阶段 1-3 已完成施工）

## 阶段

### 阶段 1:材质基础与微光漫反射模型 (Material & Shadow Refinement)
- [x] 在 `material.css` 中为 surface 与 float recipe 注入微妙的顶部内高光（Inset Rim Light），消除生硬暗色死边；
- [x] 优化浅色/暗色主题中的阴影 token（`--shadow-*` / `--popover-shadow`），由生硬单层黑底过渡为色相协调的双层柔和漫反射阴影（Tinted Soft Shadows）；
- [x] 同步更新 `theme-contract.spec.ts` 契约断言，确保六主题键集与单向别名闭环。
- **状态:** completed

### 阶段 2:顶栏控件减负降噪 (AppToolbar & Chips De-noising)
- [x] 优化 `GalleryFilterChips.vue` 与 `index.css`：未激活 Filter Chips 采用 Ghost 静息态（无常驻描边，仅保留清爽文字图标），仅在激活或 hover 时浮现胶囊背景；
- [x] 降低顶栏同时存在 8 个实体胶囊时的密集轮廓噪声；
- [x] 弹层（FormatFilterPopover / date-popover）统一接入 float material recipe。
- **状态:** completed

### 阶段 3:设置页大表面收纳与呼吸感留白 (Settings Surface Consolidation)
- [x] 将 `settings-card` 全局基座升级接入 surface material recipe，消除旧实底死框；
- [x] 优化 `SettingsView` 纵向节奏（Vertical Rhythm），扩大分类与内容间距（gap 36px / 28px），拉开呼吸感。
- **状态:** completed

### 阶段 4:全量门禁与六主题对比度验证 (Verification & Quality Gates)
- [x] 执行 `typecheck`, `vitest` 全量测试（135 文件 1549 测）、`check:contrast` 六主题全矩阵（126/126 项）、`lint` 与 `build`；
- [x] 确保在 6 套内置主题下均保持高可读性与无障碍对比度合规。
- **状态:** completed

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 内高光通过 box-shadow inset 实现，不新增额外 DOM 节点 | 零 DOM 开销、GPU 渲染开销极低且完全由 CSS 驱动 | D-010 |
| 未激活 Chip 采用 Ghost 静息态，仅 active 时呈现胶囊 | 大幅降低资产管理器高信息密度下的视觉轮廓噪声 | D-011 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 暂无 | — | — |
