---
status: 施工中
type: 工作记忆
line: 文字浓度设置
created: 2026-09-06
---

# 任务计划:文字浓度设置

## 目标
文字灰度像主题色浓度一样可配置:新增热配置键 `theme_text_strength`,把六主题的文字 ramp
(text-primary/secondary/tertiary/placeholder)改为受 `--theme-text-scale` 缩放的 color-mix
表达式,默认值明显比出厂柔和,解决「文字太瞎眼」。

## 当前阶段
阶段 5:收尾(全部门禁绿,文档已回写)

## 阶段

### 阶段 1:消费面摸底
- [x] 文字 token 清单:六主题各 4 个(primary/secondary/tertiary/placeholder);inverse/on-accent/on-*/accent-text/sidebar-active-text 不参与(极性翻转或属 accent 族)
- [x] JS 直读点:mediaGridCanvas.palette.ts(sepText/textPrimary raw)、TimelineScrubberCanvas.vue(text1/2/3 raw)、BookReader.vue(iframe 注入 text raw)——都要换 resolveTokenColor 并在浓度变化时重读
- [x] 门禁:check-theme-contrast.mjs 的 tint 正则须扩为匹配两个 scale;文字调稀是对比度变差方向,须按默认浓度实评(引入 oklch mix)
- [x] palette 脚本:--color-text-primary 目前裸 hex 直出,扩为 text 锚点组 + textExpr
- **状态:** done

### 阶段 2:参数定型
- [x] 对比度实算完成(Ottosson oklab 一次性脚本,默认 tint 60 底上):文字 75 档 primary 6.1–8.0、secondary 3.3–3.7 全守门槛;tertiary/placeholder 在 ≤85 档即破 3.0,定为「元数据文字允许随 knob 变浅」的取舍
- [x] 定型:默认 75、范围 40–100(min 防正文不可读,max=出厂原值)
- [x] 门禁策略改判:不在脚本里复刻 oklch,仍按满浓度锚点评估出厂色板,方向不对称写进注释;默认档实测数字记录在 status 文档
- **状态:** done

### 阶段 3:Rust + 前端核心
- [x] schema.rs SettingDef(theme_text_strength, UInt, 默认 75, hot) + config_commands.rs StartupConfig(→43 键)
- [x] tint.ts 改名 strength.ts(clampStrength 私有共用);uiStore 水合/setter/快照增 text;theme-snapshot.js 预写 --theme-text-scale;index.html --startup-text/--startup-muted 同配方(14 处声明)
- **状态:** done

### 阶段 4:CSS + 脚本 + 消费点
- [x] generate-theme-palette.mjs:text 锚点组 + textExpr(strengthExpr 泛化共用);--write 落 24 声明 + 六文件头注释更新
- [x] mediaGridCanvas.palette sepText/textPrimary → gc;MediaGridCanvas 增 textToken prop+watch;MediaGrid 透传;TimelineScrubber text1/2/3 → gc 且 MO 扩观察 style;BookReader text 探针解析 + watch 增文字浓度
- **状态:** done

### 阶段 5:设置 UI + harness + 测试 + 文档
- [x] settingsMap(Type 图标)/DynamicSettingControl/SettingsView/i18n 双语
- [x] runtime uiHarnessText + ipcFixtures &text= + capture --text=/文件名后缀
- [x] 单测:strength.spec(tint+text 两 describe)、uiStore.spec 文字浓度 3 用例、theme-snapshot.spec 文字 2 用例 + 启动层文字锚点断言改锚点提取
- [x] 全门禁绿;截图核验;status/todo 回写
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 只 scaling 中性文字 ramp 四 token,inverse/on-accent/on-*/accent-text/sidebar-active-text 不参与 | inverse/on-* 极性随底色翻转,混向中性会破坏按钮可读;accent 族属主题色不属灰度 | |
| 亮主题向 #ffffff 混,暗主题向 oklch(from 锚点 l 0 h) 混 | 与底色 wash 表达式同构;文字调稀=向底色极性靠拢=降对比 | |
| 默认 75、范围 40–100 | 75 档默认底色上 primary 6.1–8.0/secondary 3.3–3.7 仍守 AA 且观感明显变柔;40 下限防「文字消失」 | D-001 |
| 门禁按满浓度锚点评估(不在脚本复刻 oklch),方向不对称写进注释;默认档实测数字记录进 status 文档 | 脚本保持「出厂色板回归检查」单一职责;文字稀释是用户主动取舍(如玻璃透明度同类),默认组合可读性已有实测背书 | D-002 |
| tint.ts 改名 strength.ts,两机制同模块 | 同一模式(clamp+写 CSS 变量)两份常量,模块名不再只指 tint | |
| TimelineScrubber 用 MO 扩观察 style 而非加 prop | 该组件既有重读机制就是 data-theme MutationObserver,自包含扩展最小 | D-003 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| node -e 单引号脚本里写 '#hex' 被外层单引号截断 | bash 单引号内嵌单引号失败 | 用 \x27 转义单引号 |
| 重启前 dev server 渲染的设置页 label 显示原始键名(i18n 查找未命中) | 疑惑 locale 文件/Vite 均正常 | 上会话遗留的 Vite 进程 watcher 漏了 Edit 工具的文件变更(transform 缓存陈旧);重启 dev server 即恢复 |
