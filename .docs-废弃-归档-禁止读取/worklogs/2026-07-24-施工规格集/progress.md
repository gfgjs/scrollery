---
status: 快照
type: working-memory
line: asbuilt-spec
created: 2026-07-24
---

# 进度日志:施工规格集

## 前情(接续先读这段)
- 当前:16 篇终态全产出 + 三轮复核(样板/波1/波2)+ 波3 收口(本次:跨篇计数修订 + 全集链接一致化)全部完成,**待主线提交**
- 关键指针:frontmatter 方案 = task_plan.md「frontmatter 样板」节;引用判定表 = findings.md
- 基线:本次未 git add/commit(收口任务约束禁 git),变更均为工作树内新增/编辑

## 终态汇总(波3 收口)
- 16 篇规格集(Spec00–Spec14 正文 + README 导航)全产出,经样板/波1/波2/波3 三轮复核零遗留 P0/P1
- 承重事实已核实并钉定:Spec10 = 229 条 IPC 命令(`grep -oE 'ipc::[a-z_]+::[a-z_]+' registry.rs | sort -u | wc -l`)/ AppError 40 变体;Spec14 已如实记录生产 CSP 缺 `tauri:` scheme 的平台 gap;Spec01 = 30 表 schema V23
- 全集跨篇计数漂移本次(波3)逐处回码复核并修订:
  - Spec00:后端模块数 24→**26**(`grep -c '^pub mod' src-tauri/src/lib.rs` = 26;涉 :31/:52/:81/:83);IPC「230 命令/32 个 `*_commands.rs`」→「**229 命令/27 个命令模块**」以与 Spec10 对齐(`ls src-tauri/src/ipc/*_commands.rs | wc -l` = 27;涉 :100/:114/:231)
  - Spec10:「26 个 `*_commands.rs`」→「**27**」(同上实测;涉 :15/:17/:96),辅助模块括注补全 `model_download.rs`/`reveal.rs`(此前只列 registry/blocking/mod 三个,实际 `src-tauri/src/ipc/` 下非 `_commands.rs` 文件共 5 个)
  - Spec12:`SETTING_DEFS`「34 项」→**67**(`grep -c 'SettingDef {' src-tauri/src/config/schema.rs` = 67,均在 `#[cfg(test)]` 边界前,非测试断言误计;涉 :18/:73/:343);`STATE_KEYS`「18 项(12+6)」→**19 项(给定 13+补充 6)**(数组实测 13+6=19;涉 :78);`StartupConfig`「34 字段」→**37**(`config_commands.rs` 内 `pub struct StartupConfig` 字段实数 37,非注释内「共 34 键」的历史批次注记;涉 :324);`:406` 断链 `[map-backend 依赖表]` 已删,改指 `src-tauri/Cargo.toml`
  - 顺手同源修订:Spec12:318 行「全仓 230 条命令」同步改「229 条」(与 Spec10 权威值对齐,未在原清单点名但属同一漂移源)
- 全集跨篇占位链接一致化:`docs/spec/*.md` 全扫 `尚未产出`/`暂不加`/`待其发布`/`暂用文件名` 等占位说明,共命中 2 个文件、8 处,逐处转真链后复扫零命中:
  - Spec14(6 处::30/:31/:63/:198/:199/:200)→ Spec10/Spec12/Spec13 真链
  - Spec01(2 处::396/:433)→ Spec10 真链
  - 已是真链的引用未动

## 会话:2026-07-24(接续 C — #3 store_doc_thumbnail 守卫立项施工)
- 触发:用户采纳接续 B 的建议,#3 单独立项施工(带单测)
- 落地:
  - `db/queries/derivations.rs`:新增 `FRONTEND_DOC_THUMB_FORMATS=&["pdf","svg"]`(单源白名单)+`is_frontend_doc_thumb_format`+`validate_frontend_doc_thumb_item(conn,id)->Result<String>`(可测守卫核心:非白名单→Internal、不存在→MediaNotFound);`list_pending_doc_thumbs` 的 `IN('pdf','svg')` 改由常量插值单源生成(编译期 ASCII 常量,消除泵↔守卫双向漂移);新增 `doc_thumb_guard_tests` 3 测
  - `ipc/doc_commands.rs`:`store_doc_thumbnail` spawn_blocking 首行加守卫早退(经 db_read_pool 校验,覆盖空 body 失败分支);成功分支 `subtype` 复用守卫已校验 `file_format`,删去二次 `get_item_file_format` 查库;刷新旧「不校验」注释
- 验证:`cargo test --lib derivations` **11 绿**(3 新守卫测 + 既有 `hidden_root_derivation_tests` 覆盖 list_pending_doc_thumbs 证 SQL 插值未破)、`cargo clippy --lib` 净(仅无关 keyset 构建注)、`cargo fmt --check` 净(改动文件零重排)
- 注:纯 src-tauri 逻辑,未 git commit(待主线随其它变更批);findings.md 已把 #3 从「留置」移入「已修」

## 会话:2026-07-24(接续 B — 修复<文档层顺手发现>)
- 任务:逐条回码核实 findings.md「文档层顺手发现」11 项,修可修、debunk 误报、留置需裁项
- 做了(3 项零风险真值修复已落地):
  - #1 `src-tauri/src/db/connection.rs`:池注释 4→8(核实 `lib.rs:314` 传 8/理由 :312-313)+ 补 8 的理由 + `min_idle` 处「4× open」→「至多 pool_size 次」
  - #2 `src-tauri/src/db/schema.rs` image_meta:`dominant_*`+`is_monochrome` 5 列**全仓无写入路径**(仅 SELECT 读回 queries/media.rs:206),加「预留列」注,不删列免多余迁移
  - #11 `.worklogrc.jsonc`:`profile`(值 brownfield vs 注释「MVP 仅 strict」)、`index.mode`(值 generated vs 注释「invariant MVP 默认」)两处陈旧注释改如实
- 核实结论(无施工):
  - #8 `@tanstack` = **误报**:实为 `@tanstack/vue-virtual`(非 react-table)且已用于 `LogVirtualList.vue:10`;顺带结案 Spec11:134「待核实」
  - #9 Ed25519 in `scrollery-exotic-trust` = 核实**准确**,纯信息项
  - #3/#4/#5/#6/#7/#10 = 留置(理由逐条记 findings.md:见「留置」小节;#4 撞记忆红线 defer、#6 CSP 硬约束、#3 IPC 行为变更须专项+测试、#5/#7/#10 属他线/Copybara 风险)
- 验证:仅改注释(Rust 注释 + SQL `--` 行注 + JSONC 注释),未改逻辑/结构;未跑 cargo(改动零逻辑面)。**注:此为独立 hygiene 修复,非规格集收口;3 处代码/配置改动待主线随其它变更一并批 commit**
- 遗留:留置 6 项待主线/对应线裁;规格集正文 16 篇仍「待主线提交」(见上「前情」)

## 会话:2026-07-24
- 做了:
  - 阶段0 摸底 + 治理已完成(本次):读 Part1/Part6 frontmatter、`.worklogrc.jsonc`、`docs/README.md`、`docs/lines/refactor_2026.md`、`docs/status/refactor_2026.md`、既有 planning 三件套格式;定 canon 类 frontmatter 精确方案(id 命名同构 `PartN_标题.md`→`SpecNN_标题.md`)
  - 阶段1 脚手架完成(本次):`.worklogrc.jsonc` dirs 加 `spec`;`docs/README.md` 目录职责表加 `spec/` 行;建 `docs/lines/asbuilt-spec.md`、`docs/status/asbuilt-spec.md`;建 `docs/spec/README.md`(16篇 TOC 纯文本清单,暂不加链接);建本 planning 三件套
- 验证:未跑门禁(任务约束"不跑 git"),仅人工核对 frontmatter 字段与既有文件逐字段对齐、`.worklogrc.jsonc` JSON 结构未破坏(手工检视,未跑 JSON parser)
- 遗留:阶段2起(样板2篇 01数据层+06AI、子系统篇分批、聚合篇复核、封顶、收口)均未开始;16 篇正文文件尚未创建
