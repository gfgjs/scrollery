---
status: 施工中
type: 工作记忆
line: 玻璃模式内容底面修复
created: 2026-09-06
---

# 任务计划:玻璃模式内容底面修复

## 目标
按 [2026-09-06-玻璃模式内容底面修复方案](../../designs/2026-09-06-玻璃模式内容底面修复方案.md) 施工:玻璃模式新增 `--glass-content-fill`(bg-primary 90% 半透明)内容底面 token,落到文字密集视图根(opt-in),`.settings-header` 拼布修复,新增 `glass_content_opacity` 配置键(scale 20–120 默认 100)全链路接线,收口时同次修订上游 2026-08-24 方案决策记录正文。

## 当前阶段
全部施工阶段完成;**剩余方案 §7 真机手动验收(仅用户可执行)**,故三件套留在 planning/ 不迁 worklogs。

## 阶段

### 阶段 1:视图现状核查(为 CSS 落点核实)
- [x] 盘点各视图根类名与现有背景(8 视图,结论见 findings.md)
- [x] 核查 `.audio-player` / `.log-window` / `.hlab` 根面:三者根均自铺不透明 bg-primary,按「根面裸则加 fill」判据**不动**;log-window 另因独立窗口不吃玻璃管线
- **状态:** done

### 阶段 2:CSS 核心(glass.css)
- [x] `html[data-glass]` 共享块加 `--glass-content-scale: 1` 与 `--glass-content-fill`(90%)
- [x] `.app-content` 保持 transparent(独立规则),`.settings-view` 改 `var(--glass-content-fill)`
- [x] 新增 `.settings-view .settings-header` 玻璃下置 transparent
- [x] 新增 opt-in 清单:settings-view/collections-view/persons-view/plugin-store/doc-viewer;另按方案 §1「等残留不透明子面」补 doc-viewer 工具条(文档流上下关系,置透明安全)
- [x] 文件头注释补「视图根 opt-in」接入手册
- **状态:** done

### 阶段 3:配置链(第 6 个玻璃键)
- [x] schema.rs — `glass_content_opacity` 插在 control 与 gallery 之间(UInt,默认 "100",hot,注释含 120 钳制语义)
- [x] config_commands.rs — StartupConfig 字段 + get_startup_config 取值/构造(共 41 键)
- [x] uiStore.ts — 接口/ref/applyGlassOpacityConfig/setGlassContentOpacity/水合解析/store 导出
- [x] uiScale.ts — GlassOpacitySurface + GLASS_OPACITY_VARIABLES 加 content
- [x] settingsMap.ts — glassContentOpacity 项(20–120,general 组)
- [x] DynamicSettingControl.vue — numberBindings 接线
- [x] SettingsView.vue — generalSettingGroups.appearance 数组插键
- [x] ipcFixtures.ts — StartupConfig 夹具补 glassContentOpacity: null
- [x] zh-CN.ts + en-US.ts — label/desc 同步(localeIntegrity 过)
- [x] 核查:Rust 侧无其它 glass opacity 消费点;useConfigFile 热更新走 refreshFromBackend→hydrateFromStartupConfig 自动覆盖新键
- **状态:** done

### 阶段 4:测试
- [x] uiStore.spec.ts — 新增两用例:水合损坏值回 100 + 写 content scale;setter 写 `glass_content_opacity` 键 + CSS 缩放
- [x] glass.spec.ts(计划外发现)— 旧契约测试钉住合并透明规则,已按新契约更新:拆分后 app-content 透明 + fill 清单 + settings-header 透明;scale 测试补 content token 断言
- [x] schema 单测遍历式覆盖新键,无需新增(方案 §5.11 预期一致)
- **状态:** done

### 阶段 5:验证
- [x] `cargo test --workspace --locked` — 1355 passed(23+22+40+8+1262),0 failed
- [x] `npm run typecheck` — 通过
- [x] `npm test` — 155 files / 1747 tests 全过(1745 原 + 2 新)
- [x] `npm run lint` — 通过
- **状态:** done

### 阶段 6:文档收口(代码侧)
- [x] 修订上游 `docs/designs/2026-08-24-窗口材质毛玻璃方案.md`:文首修订标记 + §1 摘要 + §3 架构图/边界 + §9 决策记录第 7 条(正文直改,AGENTS 规则)
- [x] 回写滚动状态:`docs/status/UI-多主题系统.md` 顶部新条目 + `docs/todo.md` 线索引 ⏳1→⏳2
- [ ] **真机手动验收(方案 §7,用户执行)**——背板三态矩阵/画廊回归/三风格×亮暗×壁纸/热切换/托盘往返
- [ ] 手动验收过后:三件套迁 docs/worklogs/ 收口
- **状态:** in_progress(代码侧 done,验收侧待用户)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| fill 源 token = `--color-bg-primary`,mica/acrylic 共用 90% 基准 | 与非玻璃画布同源;少一个自由度 | 方案 §9 已录,不另立 |
| fill 落视图根 opt-in,不铺 `.app-content` | 保护画廊 0% 透出已交付决策 | 方案 §9 已录 |
| audio-player/hlab/log-window 三视图**不加** fill | 根面自铺不透明 bg-primary 非裸面,自带承重面;log-window 为独立窗口不吃玻璃管线 | 阶段 1 实查裁定 |
| doc-viewer 工具条一并置透明 | 方案 §1「等残留不透明子面」授权;根面领 fill 后工具条不透明即成拼布 | 会话内有效 |
| glass.spec.ts 契约测试随新契约更新 | 旧测试钉住被修订的合并透明规则,不更新即红 | 会话内有效 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| (无施工错误;一次自纠:task_plan 初版误把未开工阶段标成 done,立即重写) | — | — |
