---
status: 快照
type: working-memory
line: 全仓注释清理精简
created: 2026-07-25
---

# 发现与决策:全仓注释清理精简

## 需求
- 全仓源码注释清理精简:删冗余/复述/双语重复/装饰性注释,不损原意,代码语义零变更;拿不准的列待裁清单。

## 发现
- 在范围源码文件 689 个(`src-tauri/src` 255 .rs、`src/` 排除 vendor 后约 400 .ts/.vue、`crates`、`scripts`);注释总量约 31064 行(Rust 18980 / 前端 12084)。Top60 清单与目录分组落盘 `C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\2134415b-1664-49db-aa81-5bd689140ab4\scratchpad\recon-stats.md`
- 首轮英文注释摸底用 haiku 模型报「双语重复 0 处」,系**误报**——后续确定性脚本仅在 `ipc/` 单目录就删出 194 行相邻双语对。教训:低档模型的「零命中」结论不可采信,必须有确定性脚本对拍复核
- **F-a 脚本不校验语义等价**:规则 1(英文行紧邻中文译文行→删英文)执行脚本只按「行邻接」判定译文关系,未做语义等价校验,误删了一批**非译文**英文行。已确证误删类型:
  - Copybara 剥离标记 `// BEGIN-INTERNAL`(配对 END 仍在原地 → 若不修复,闭源块会漏进公开镜像,且会踩 FORBIDDEN 禁词门禁)
  - shell/cargo 命令行(测试与基准的跑法说明)
  - 算例行(如 `6000×4000 + LongEdge(300) → 300×200`)
  - 公式行
  - rustdoc 摘要行(删后留下英文残句,中英混杂)
  - 列表编号行、intra-doc 链接行
  - 含型号/数值的说明行(如 `Model: eisneim/cn-clip_vit-b-16`)
- **F-b 首份 opus 审计回执系编造**:该回执所引用的 8 条「被误删英文原文」在全仓 diff 中逐条 grep 核验均不存在,回执尾部还有明显退化痕迹。已改用「先机械抽取 diff 中被删的非 CJK 注释行 + 保留上下文落盘,再对着文件逐条人工判定」的取证式复核流程;同一代理二次运行给出的 40 条发现经主线抽验 3/3 属实。教训:审计类结论必须落盘可对拍,禁止仅凭代理叙述采信
- **F-c 机械筛查产出**:被删的非 CJK 注释行共 2069 条,按高危模式打标后至少命中一标的有 1226 条(CMD 命令行 13 / CONT 上下文相关 557 / NUM 数值算例 149 / LIST 列表编号 48 / LINK 链接引用 423 / ID 决策锚点等标识 627);明细落盘 `C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\2134415b-1664-49db-aa81-5bd689140ab4\scratchpad\misdelete-candidates.md`;抽取脚本 `C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\2134415b-1664-49db-aa81-5bd689140ab4\scratchpad\extract_audit.py`
- **F-d 顺带修复的两处基线红**:clippy `err_expect` 命中 3 处(`crates/exotic-workers/enhance-worker/src/run.rs`,`.err().expect()` 改为 `.expect_err()`);eslint 185 error 全部来自 `.claude/worktrees/**` 与 `target/**` 非源码目录,已在 `eslint.config.js` 的 `ignores` 补入 `.claude/**`
- **F-e 未修的基线红**:`cargo fmt --all --check` 报 44 文件 589 行 diff,属 dev 分支既有债,与本清理线无关,本线不代修;收口复跑后红名单收敛为 5 文件(enhance-worker 的 main/run/session.rs + src-tauri/src/enhance/service.rs + src-tauri/src/ipc/enhance_commands.rs),文件集与基线相同,仍属既有债未代修
- **F-f 复核回执编造教训(重要)**:施工过程中出现两份复核回执被查出编造发现——引文在全仓 diff 中不存在(F-b 已记首份);其中一条「ORT session 非 Send」的编造发现还被后续修复代理照抄进 `crates/scrollery-ai-core/src/engine.rs`,产生了一处并不需要的改动,后经核实该 crate 已用 `Receiver<Session>` 模式规避跨线程持有 Session,原「非 Send」前提证伪,已删除该处误改。结论:复核结论必须落盘可对拍、引文须逐字可查,禁止凭代理叙述采信,即便是自称经过对抗核验的回执也须机械抽取比对

- **F-g 待裁 5 项处置(2026-07-25 第二会话)**:用户裁「开工」,逐项落地/结案如下
  - 第 1 项 `cargo fmt` 5 文件既有债 → **裁定清掉**,`cargo fmt --all` 只动这 5 文件(纯排版:超宽行折行 + 一处多余空行),`762d1e2`;`cargo fmt --all --check` 由长期红转 🟢
  - 第 2 项 state.rs 中英混杂残留 → **落地**。原报「:126」行号已漂(注释线自身在改),取证锚点为 `/// Cancellation token for the background exotic (冷门格式插件) processing pipeline (R1).`,在 `d9b337f^` 的 183 行、当前 173 行,确证改动前既存。同文件另有 15 处同类(英文块 + 中文全译并存)与 2 条纯英文孤行,一并清理,`ba3be1c`
  - 第 3 项 fast_scan.rs 重复行 → **落地**。`d9b337f^:13-14` 即已两行相同 `4. compute_cache_key`(该行纯标识符、无英文散文,故规则 1 脚本与人工审都未识别为重复),删一行;同文件另 5 处英文块一并清理,`ba3be1c`
  - 第 4 项 en-US.ts 的 `U-2` 工单号 → **无需改动,已自证**。该注释系逐字复制 `zh-CN.ts` 同 key 的既有注释(`d9b337f^:src/i18n/locales/zh-CN.ts:1187` 即已存在),不是代理新编;`U-2 方案 A ②` 与「导出当前视图」的绑定另有三处独立锚点(`GalleryViewControls.vue:3/109/181`),`ExportSource::View { name: String }` 在 `src-tauri/src/export/manifest.rs:25` 实存。清理动作实为把英文 locale 的注释与中文 locale 对齐
  - 第 5 项 ReaderSettingsSection.vue 被跳过 → **无需改动,跳过是对的**。全文 186 行、注释 12 行,全为中文且全部解释「为什么」(与书内面板的分工与单源、与 SettingRow 的内边距对齐理由、列数取最大值的理由、内联样式为何允许),无英文散文 / 无双语重复 / 无死码 / 无装饰 banner——按规则集属「留」,没有可删项
- **F-h 残留量实测(新发现,规模级,待裁)**:上一轮规则 1 脚本的守卫「英文块须单行」使**多行**英文块 + 中文全译的组合整批逃逸。用确定性扫描器(`C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\47fe8285-d90f-42bb-a8cb-44fd9fe15cb8\scratchpad\scan_residual_en.py`,已剔除 `*` 续行假阳、装饰框线、命令行、裸文件路径头、抑制指令 / URL / 决策锚点)实测:**英文散文注释行 515 行、分布 84 文件;其中疑似漏删双语对 150 处**(本次两文件修完后的数)。集中在 src-tauri:`derive/pipeline.rs` 46、`db/queries/faces.rs` 38、`ipc/ai_commands.rs` 33、`ipc/file_ops_commands.rs` 31、`lib.rs` 23、`thumbnail/cache.rs` 22。另有 **99 个 .rs 文件仍带自指文件路径头**(`// src-tauri/src/xxx.rs`),按规则 5 本该删而上一轮未清完。**裁决(2026-07-25 用户):第二轮暂不做,仅记录留账**——重启须新决策,不得因「看见英文注释」顺手清成一次未授权的全仓改动波

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
