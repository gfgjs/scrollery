---
status: 施工中
type: 工作记忆
line: 主题色浓度设置
created: 2026-09-06
---

# 任务计划:主题色浓度设置

## 目标
六主题底色 token 改为 `color-mix` 浓度缩放,新增 `[ui].theme_tint_strength` hot 配置键(20–100,默认 60)——默认观感即调浅,用户可在设置页自行调浓(100% = 现出厂满浓度);复用玻璃浓度键(2026-08-25 先例)的全链路接线模式。

## 当前阶段
阶段 5:文档回写(代码/门禁/文档全交,余真机观感验收)

## 阶段

### 阶段 1:调研与方案定型
- [x] 摸清主题静态 CSS token + generate-theme-palette 锚点校验 + uiStore data-theme 单源
- [x] grep glass_content_opacity 确认 10 文件接线清单
- [x] 识别三处消费 hex 的门禁(契约测试明度/预览同步、对比度脚本)需随模型演进
- **状态:** done

### 阶段 2:浓度工具与配置链路(Rust + uiStore + 快照)
- [x] `src/themes/tint.ts`:clamp + applyThemeTintStrength(写 `--theme-tint-scale`)
- [x] schema.rs `theme_tint_strength` UInt hot 默认 60;config_commands StartupConfig 增字段(→42 键)
- [x] uiStore ref/setter/水合 + 快照载荷增 tint;theme-snapshot.js 首帧写 CSS 变量;index.html 启动层同 mix
- [x] ipcFixtures 增键 + `&tint=` 覆盖(runtime.ts);uiStore.spec 三用例
- **状态:** done

### 阶段 3:CSS 与门禁演进
- [x] 六主题 10 wash token/主题经 palette 脚本 `--write` 生成 mix 表达式(+文件头机制注)
- [x] palette 脚本锚点表增 canvas/paper 字段,tintExpr 唯一权威
- [x] 契约测试(anchorColorOf)/对比度脚本按满浓度锚点评估
- [x] settingsMap/DynamicSettingControl/SettingsView/i18n;capture 脚本 `--tint=`
- **状态:** done

### 阶段 4:验证与截图裁决
- [x] vitest 156 文件/1762 测、typecheck、lint、cargo 1262、palette、contrast 全绿
- [x] 截图核:fresh-light t100/60/40(gallery)、fresh-dark t60(gallery,暗底正常→相对色语法 OK)、tech-light t60(settings)、bucket=1 Canvas 路径(canvasGap/占位色正常→探针解析 OK)、启动层 mix 底色
- [x] 裁决:默认 60(明显更浅仍留清新倾向);40≈近无色,100=原观感
- **状态:** done

### 阶段 5:文档回写
- [x] status/UI-多主题系统.md 新增 2026-09-06 条目(修复过一次误吞玻璃条目行);todo.md 行 ⏳2→⏳3
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 做成设置项而非纯调浅:默认 60 + 滑键 20–100 | 用户原话两个诉求(调浅/可设置)一个机制全覆盖;玻璃键先例使链路成本可控 | D-001 |
| 混合空间用 oklch,亮主题向 #ffffff、暗主题向同明度无彩灰 | 亮色「浅」=提亮+去色;暗色只去色保暗;oklch 保感知明度,层序不变 | D-002 |
| 无彩中性用 oklch(from <锚点> l 0 h) 相对色语法,单源自锚点 | 零手工算色零漂移;WebView2 常青 Chromium 119+,2026 年基线 | D-003 |
| 三处 hex 门禁按「满浓度锚点」评估而非运行时解算 | scale=1 时 mix 结果=锚点本身,是最保守界(对比度最差、层序同序),无 oklch 数学依赖 | D-004 |
| 作用域限底色 wash(bg 五层+canvas 三件+doc-paper 两件),accent/文字/边框/hover 不动 | 主题身份与可读性锚点不该随浓度漂移;hover 等 alpha 叠加本已极淡 | D-005 |
| wash 表达式由 generate-theme-palette.mjs 的 tintExpr 生成(--write 机械改写六主题) | 60 条声明手改易错;脚本本就是色板唯一权威,顺带成为表达式唯一权威 | D-006 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| oklch 逆变换用 cbrt(red 解出 g=229) | cbrt 是 sRGB→oklab 正向;逆向应把 LMS **立方** | `** 3` 替换,红/蓝 CSSWG 参考值测试钉死 |
| 状态文档 Edit 误吞玻璃条目行首 | old_string 取了玻璃条目前缀做锚点替换 | 补一行拆回两条独立 bullet,grep 复核 |
