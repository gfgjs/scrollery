---
status: 快照
type: 工作记忆
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-19
---

# 会话日志:方案 A 施工

## 2026-07-19 会话 1

- 读方案 A 定稿(`方案A-导出整理成果.md`,last-verified 2026-07-19)+ 线级 `task_plan.md`(阶段 4:待裁决,复审建议 B→A→C)。
- 核实用户提醒的"代码较大改动":对比方案假设与当前 HEAD(0170382,backup 线 5018c76..0170382 六批已交付)——
  - 文件任务门闩(`FILE_JOB_EXPORT` 常量、`file_job_owner`、`RunTokenSlot`)已由 B 落地,A 直接复用,方案假设成立。
  - 方案 §4 前端部分假设有误:backup IPC 零前端接入,`BackgroundFileJobIndicator` 实际从零建。
  - 错误码 `export_already_running`(方案 §3.3 猜测)与实现惯例 `file_job_busy`(域共享稳定码)不符,改用后者。
  - 模块拆分改为 `export/core.rs` + `ipc/export_commands.rs`,对齐 `backup/core.rs` + `ipc/backup_commands.rs` 惯例。
- 开新施工线目录 `docs/worklogs/2026-07-19-导出整理成果施工/`,三件套建立完毕,登记 F-001..F-003、D-002..D-003。
- 下一步:阶段 1 后端核心引擎(error.rs → state.rs → db/queries/export.rs → export/core.rs)。

## 2026-07-19 会话 1(续):后端落地

- 阶段 1+2(后端核心引擎 + IPC 接线)全落地:`error.rs`(`AppError::Export`)、`state.rs`(export 三件套)、
  `db/queries/export.rs`(分块批量元数据+tags+albums)、`export/{core,naming,manifest,mod}.rs`、
  `ipc/export_commands.rs`(preflight/start/status/stop)、`registry.rs`+`constants/ipc.ts` 接线。
- 新增依赖 `filetime = "0.2"`(mtime 跨平台显式保留,std::fs::copy 不保证)。
- 施工中订正一处方案假设:核心取消语义改用 backup 既有的 `Err(AppError{code:CODE_CANCELLED})`
  幂等姿态(而非自造 `Ok(outcome{cancelled:true})`),与 backup_commands.rs 的 `finalize_payload`
  分流模式保持一致——发现于写 core.rs 时对照 backup/core.rs 的 `cancelled()` 精读。
- 测试:`cargo test --lib` 706→729(新增 23,全绿);`cargo clippy --lib -- -D warnings` 除
  `scan_commands.rs:96`(会话前已存在、非本次改动触发的 `manual_inspect`,未触碰)外零告警;
  `rustfmt --check` 仅对本次新增/改动的 6 个文件核实(未跑全仓,`backup/core.rs` 等文件本就有
  与当前 rustfmt 版本的格式漂移,不属本次范围)。
- 三处测试自查中发现的实现小 bug(均已修):`sequence_width` 断言算错位宽期望值、
  db 层测试的 `seed_item` 未做 (root,rel) 幂等致 UNIQUE 冲突、mtime 断言误比对了源文件实时
  fs mtime 而非应验证的「DB `file_mtime` 字段被显式写入」契约本身。
- 下一步:阶段 3 前端(exportStore → ExportDialog → BackgroundFileJobIndicator → 入口接线 → 文案)。

## 2026-07-19 会话 1(续二):前端落地

- 摸底既有约定(未先假设,逐个核实):`derivationStore.ts`(轮询姿态,不适用)vs `scanStore.ts` 的缩略图生成进度恢复(app 事件 + 快照,**同构**,采用此姿态镜像出 `exportStore`)、`useTauriListen`/`listen` 直用两种监听方式(选了 store 场景下与 scanStore 一致的直用 `listen` + 共享 Promise 去重)、`UiDialog`/`UiField`/`UiSelect`/`UiCheckbox`/`UiButton` 五个 S1 原语的 props 契约、`SelectionCommand[]`(选区工具条数据驱动)与 Command 注册表(`resolveMediaContextCommands`,右键菜单)两套机制的边界、`@tauri-apps/plugin-dialog` 的 `open({directory:true})` 折叠文件夹选择器先例、`open_directory` IPC(既有,SettingsView 已用)复用为「打开导出目录」。
- 落地文件:`exportStore.ts`(+ `exportStore.spec.ts` 5 例)、`ExportDialog.vue`、`BackgroundFileJobIndicator.vue`;接线 `App.vue`(挂载 + `restoreExportProgress`)、`AppStatusBar.vue`(右区插入指示器)、`MediaGrid.vue`(`selectionCommands` 新增 `export` 项 + 右键本地命令 `grid.contextExport`,`localMoveCopy` 顺手更名 `localOrganizeCommands` 反映其已不止 move/copy);`zh-CN.ts`/`en-US.ts` 各新增 `export` 顶层节 + `selection.export` 单键。
- 施工中一处 eslint 拦下的真实规范违规:`vue/no-bare-strings-in-template` 挡住摘要行里裸露的中点分隔符与全角括号(拼接 `t()` 输出的字面量分隔符仍算「未翻译裸串」),改法=把整行摘要收进单个 i18n key `export.summaryWithSize`(值内含分隔符),而非把符号也套 `t()`——教训:i18n 门禁按「模板里出现的字面量字符」判,不是按「有没有调用 t()」。
- 验证:`npm run typecheck` 净;`npx eslint`(逐文件点名跑,含新建 spec)净;`npx vitest run`(locale integrity + stores 子集,随后**全量**)1234→**1239** 全绿,94 个测试文件。
- 未做且已记录为范围裁剪(非遗漏,见 task_plan D-004/D-005):相册/当前视图工具栏入口未接线(已于 2026-07-21 清账波 U-2 落地,commit 5a97bd4);崩溃遗留 staging 目录的「启动时可清理」列表 UI 未做。
- 已回写:`docs/todo.md` 新增「2026-07-19 增补」条目(与既有 2026-07-17/18 条目同构:摘要+验证证据+三件套指针+范围裁剪说明+⏸真机验收清单)。
- **未做**:真机运行(`npm run tauri dev`)实际点验导出流程——本任务全程只跑到 typecheck/lint/单测/编译这一层,没有启动过真实 Tauri 运行时验证 IPC 往返、对话框弹出手感、状态栏指示器视觉、toast 的「打开导出目录」按钮实际点击效果。这是本项目一贯做法(全部同类线在 todo.md 里都标"⏸真机验收"),但要如实说清:**typecheck/lint/单测全绿不等于功能在真实 GUI 里工作正常**,尤其是本次新写的表单交互(UiSelect 的 `naming`/`conflict` 双向绑定用了 `:model-value` + `@update:model-value` 手动桥接而非直接 `v-model`,原因是类型收窄,这段桥接本身未经真实浏览器/WebView2 事件触发路径验证,只过了 TS 类型检查)。

## 回顾(收口时补全)

(待补——本次会话未达用户"收口"指令,line 仍开放供接续:真机验收 + D-005 相册/视图工具栏入口补线)
