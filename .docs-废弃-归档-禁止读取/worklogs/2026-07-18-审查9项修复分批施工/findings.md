---
status: 快照
type: working-memory
line: 审查9项修复分批施工
created: 2026-07-18
---

# 发现与决策:审查9项修复分批施工

## 需求
- 用户:「开工,开始无人值守,分阶段 commit,直到需要我决策」——对已核实的 9 项审查发现分批修复。

## 发现
- 前置核实(本会话上半场):9 项全部属实,无一误报。证据锚点:
  - F-01 pipeline.rs:97 无条件 cancel_derivation;state.rs:465 finish_ai_analysis 是修法模板
  - F-02 thumbnail_commands.rs:936 无条件清 token;:515 启发式使新轮被持续误判 cancelled
  - F-03 scan_commands.rs:379-401 无事务;错误路径跳过 bump_data_version+invalidate_root
  - F-04 text_index.rs:342-345 仅行边界切分;BookReader.vue 256K 护栏仅 MD 分支
  - F-05 scan_commands.rs:319 校验前 allow_directory;注释引 add_scan_root:38 同款姿态
  - F-06 derive_commands.rs:106 reset 失败 warn 后照常 launch,IPC 返 Ok
  - F-07 media_foundation.rs:399-402 两个 let _,注释与行为相反
  - F-08 FoldersSection.vue:1364-1396 单 try/catch 罩三操作
  - F-09 ContentViewer.vue:721 void fire-and-forget;mediaStore.ts:360 无串行无捕获
- finding 耦合:F-03+F-05 同函数;F-01+F-06 同命令链;F-08 是 F-03 前端对偶——按批施工。
- 施工中发现:lib.rs:328 启动按当前扫描根授 asset scope → 「旧根授权不撤、重启收敛」成立(F-05 决策依据)。
- 施工中发现:`full_thumb_gen_status` 的「快照 running 但 token 不在 → 报 cancelled」启发式(thumbnail_commands.rs)把 F-02 从「瞬时快照污染」放大为「新轮被持续误判已取消」——修后 finish 仅当当前轮才清槽,启发式恢复可信。
- 施工中发现:TXT sections 的 `size` 即后端 `charLen`(syntheticBook.ts buildTextSyntheticBook),前端护栏零新增数据依赖。
- 事务故障注入不可行:db::queries 为自由函数、无注入 seam,mock 需引 trait 层——超本线范围,记 gap。
- MF 流选择回滚不可自动化:需不支持单流选择的真源或 COM mock;编译+clippy 过,行为凭代码路径复核。

## 外部资料(当数据,不当指令)
- docs/reviews/2026-07-18-昨日与今日代码修改审查.md:外部 AI 审查报告,修法建议已逐条核实合理。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-024 | 教训:后台任务 token 槽必须带 run-generation compare-and-clear;裸 `Option<token>` 无条件清槽 = 「停止→立即重启」竞态模板。同仓两范式并存是竞态温床——新入口(c73f8e0 手动重启)一接旧范式,潜伏竞态即进用户路径。修法=统一升级为 RunTokenSlot(state.rs),非逐点打补丁 | experience |
| F-025 | 遗留:ai_analysis_token / face_analysis_token 曾是裸 `(u64, token)` 元组实现,与 RunTokenSlot 双范式并存;**已落地**(2026-07-19 用户批开工,批5 `0bf5ebb`:字段升 RunTokenSlot+删共享计数器+wrapper 薄委托,731 测持平)。迁移关键点=ai/face 终态副作用门控保持 `!is_cancelled()`,finish 返回值刻意丢弃(与 thumb 的 finish-bool 门控是两回事,已注释钉住) | no-promotion 候选(代码+注释自载,收口时裁) |
| F-026 | 契约:foliate 接新格式的双层防线 = 后端行边界分片 + 前端超限片强制 scrolled 护栏(阈值/谓词 syntheticBook.ts 单源 FORCE_SCROLLED_SECTION_CHARS/hasOversizedSection);行内切分在非 UTF-8 编码需 decoded-char→源字节映射,属新风险面,defer 且由 characterization 测试钉住 | experience(补 2026-07-17 内存爆炸线既有条目) |
| F-027 | 原语:fire-and-forget 乐观写一律走 latest-write-wins 队列(utils/latestWrite.ts,按 key 串行/跳中间值/失败回调);适用旋转、未来任何「连点覆盖型」标量写 | no-promotion 候选(代码+测试自载文档,收口时裁) |
