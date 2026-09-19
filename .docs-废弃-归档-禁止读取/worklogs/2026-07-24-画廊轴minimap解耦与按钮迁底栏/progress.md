---
status: 快照
type: working-memory
line: 画廊轴minimap解耦与按钮迁底栏
created: 2026-07-24
---

# 进度日志:画廊轴minimap解耦与按钮迁底栏

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:R-5 落地——time 坐标滑窗回归(线性对齐方案,用户裁),接在 R-3/R-4 三轮回退(4a38ba2)之后
- 未解错误:无
- 关键指针:findings「裁决(2026-07-25):R-5」节=方案规格+可证事实+语义代价;C-1 已裁接受现状(见记忆线)
- 剩余:⏸GUI 真机验收(含 R-5 time 坐标滑窗手感);收口须用户明示才发起
- 基线:工作树脏五文件(MediaGrid/VideoSeekBar/ReaderSettings/useBucketVirtualScroll/SettingsView)为用户新 WIP,不碰不入提交

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-24
- 做了:三路摸底(轴门控/底栏+fab/minimap 数据依赖)完;architect 方案设计;两批施工落地——批一 config schema 新增 axis_mode(schema.rs + config_commands.rs StartupConfig)、uiStore 字段更名 showSeamlessMinimap→axisVisible+新增 axisMode、ipcFixtures 加 axisMode;批二 MediaGrid/AppStatusBar/TimelineScrubberCanvas 轴按钮迁底栏
- 验证:全量门禁六项均绿——`cargo fmt --check` exit 0(已格式化);`cargo test`(workspace)exit 0,决定性行 `test result: ok. 959 passed; 0 failed; 6 ignored`(主 crate)+ config::schema/ipc::config_commands 全 9 项测试(keys_are_unique/find_by_key_roundtrips/routing_* 等)通过,axis_mode 新键未破 schema 契约;`cargo clippy --all-targets` exit 0,输出仅 2 条既有基线警告(内测签名根路径提示,非代码 warning),无新增;`npx vue-tsc --noEmit` exit 0 无输出;`npx vitest run` exit 0,决定性行 `Test Files 118 passed (118)` `Tests 1425 passed (1425)`;`npx eslint src` exit 0 零输出
- 遗留:未 commit(归主线);⏸GUI 真机验收(轴按钮迁底栏后的交互/密度带钮定位)

## 会话:2026-07-25(阶段 4 复核收尾)
- 做了:核实施工已落 commit `e1b1789`(在 dev,10 文件 +308/-270);主线亲审 config 契约面
  (agent 委派按 harness 约束未启用),结论=四道防线成立、不落入 2026-07-25 深审 F-04;
  落 2 项实修——R-1 `schema.rs` `seamless_minimap` 的 `comment_zh` 语义过期(会写进用户可见
  config.toml)改写为「轴开合 + 键名沿用历史 + 形态另见 axis_mode」;R-2 新建
  `src/stores/uiStore.spec.ts`(8 测)钉枚举守卫/默认方向/**写盘键名仍是 seamless_minimap**;
  另记 C-1 沉浸态轴钮不可达待用户裁。全程未碰 5 个用户 WIP 脏文件。
- 验证:`npx vitest run src/stores/uiStore.spec.ts` exit 0,`Tests 8 passed (8)`;全量
  `npx vitest run` exit 0,决定性行 `Test Files 120 passed (120)` `Tests 1454 passed (1454)`;
  `cargo fmt --check` 对 `config/schema.rs` 零 diff(首次改动引入一处 rustfmt 折行、已按其建议
  改 `comment_zh:` 换行;**全仓仍红 20 处,全在 `enhance/service.rs` + `ipc/enhance_commands.rs`,
  系 dev 既有债非本线**);`cargo test -p scrollery config::` exit 0,决定性行
  `test result: ok. 32 passed; 0 failed`;`npx vue-tsc --noEmit`(仓根)exit 0 无输出;
  `npx eslint src/stores/uiStore.spec.ts` exit 0 零输出。
- 遗留:C-1 待用户裁;⏸GUI 真机验收(轴钮迁底栏交互/密度带钮定位/沉浸态手感);
  收口归档未发起(须用户明示)。

## 会话:2026-07-25(R-5 time 坐标滑窗回归——线性对齐方案)
- 做了:用户要求 time 坐标(时间轴+谱+时)补回滑窗,并明确规格「轴内指针与画廊内指针对齐+
  同步贴顶/贴底」;同时裁定 R-3/R-4 的「三条死路」分析来自低级模型不可信。据规格推出唯一解:
  处处对齐 ⇒ 与画廊指针同映射 ⇒ 线性滚动比例式(详见 findings「裁决 R-5」节)。落地
  TimelineScrubberCanvas.vue 两处:视窗 `v-if` 去 time 排除;drawAxis 指示线删
  `logicalYToTimeFrac` 分支恒走 `scrollFrac×cssH`。helpers/测试零改动。
- 验证:`npx eslint src/components/media/TimelineScrubberCanvas.vue` exit 0 零输出;
  `npm run typecheck`(vue-tsc)exit 0 无输出;全量 `npm test` exit 0,决定性行
  `Test Files  120 passed (120)` `Tests  1454 passed (1454)`。
- 遗留:⏸GUI 真机验收新增 R-5 三查:①时+谱下滑窗在且可拖(跟手/两端可达);②轴内指示线与
  MediaScrollbar 标尺线全程重合、同步贴顶/贴底;③点轨跳日期后指针停线性滚动位属刻意语义,
  确认可接受。
