---
status: 快照
type: working-memory
line: 阅读器页UI优化
created: 2026-07-17
---

# 进度日志:阅读器页UI优化

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:建档+摸底;阶段 2-4 全部施工落地——
  - A 窄窗折叠:DocumentViewer 工具栏 14 控件包 fold-item 进 useToolbarOverflow 折叠容器(复用顶栏引擎,非自造),⋯ 钮+UiPopover 溢出菜单(select 项保留原控件、动作项点击即关);进度 span nowrap+ellipsis 根治竖排;旧 spacer 由容器 justify-end 接替。
  - B 侧钮:BookReader 通栏 48px 列改 overlay 浮动圆钮(lucide chevron),阅读面回收 96px。
  - C 沉浸:immersive prop 下沉 BookReader;applyImmersiveChrome 切 renderer margin '0px'/'48px' 去 foliate 头/脚带;侧钮沉浸全隐+自身 hover/focus 唤出;hostBg 绑阅读主题背景(挂载即着色)。
  - i18n:doc.moreTools 双语。
- 验证:`npm run typecheck` 0 错;`npx eslint <4 改动文件>` 0 报;`npm run test` 全量 88 文件 1184 测试全绿(localeIntegrity 含在内)。均为本地结果,非 CI。
- 门禁注记:`node tools/check_docs.mjs` 本地跑红,但红项为**全仓 HEAD 既有文件**(status 枚举 active/snapshot vs 中文枚举之争),非本任务引入;按既定红线「docs 门禁 HEAD 基线即红属收编线,勿代修」不动,本三件套沿用模板的 `status: active` 与全仓一致。
- 遗留:仅 GUI 真机手测(下方清单,不自动化);过 GUI 后可提议收口。

### GUI 手测清单(⏸真机,不自动化;编译+本地门已过)
1. txt/epub 拖窄窗口:控件按尾序折进 ⋯(0.28s 动画),拖宽逐项还原、⋯ 消失;进度「N%·章名」只省略不竖排。
2. ⋯ 菜单:折叠项齐全,select 可调不关窗,动作项点击即关;Esc/点外关闭;窗口临界宽度反复拖不震荡。
3. 侧钮:浮动圆钮垂直居中,默认半透明、悬停实体化;点击翻页正常;正文可用宽比旧版宽 96px。
4. 沉浸(paginated):顶部运行头+底部页脚带消失;侧钮全隐、指针悬其落点或 Tab 聚焦唤出;底色与阅读主题一致(试羊皮纸/夜间);退出全还原(头带回 48px)。
5. 沉浸(scrolled):上下 padding 归零全出血;右缘滚动条与 next 钮是否打架(若挡,再调 next 钮 right 偏移)。
6. pdf:翻页模式 select 照常工作、窄窗可折;编辑态(txt)进出时工具栏项集变化不错乱。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
