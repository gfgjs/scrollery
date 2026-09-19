---
status: 快照
type: working-memory
line: 无人值守清账波
created: 2026-07-21
---

# 发现与决策:无人值守清账波

## 需求
- 无人值守清账波:入口 docs/todo.md + MEMORY.md + docs/planning/ 各线遗留(⏸/余/defer);目标=清掉所有无需人拍板的遗留项;验收=全库 cargo test + vitest 绿且新增项全部有 commit。禁提问/禁 push/只 stage 显式路径。

## 发现
- convertFileSrc 直连从 07-11 记载的 5 处缩至 3 处(全在 useHoverPreview.ts),✓consumed→93fe1e9(收口后清零)
- thumbCacheDir.ts 模块头注自证「SettingsView 暂未迁」,且模块**无失效机制**;深审证实旧行为改目录后模块缓存滞留旧值、架空 minimap 失效链 ✓consumed→81404b7
- scanStore 裸 listen 在异步惰性函数 ensureEnrichmentListeners 内,useTauriListen 要求同步 effect scope 上下文 → 迁移方向不唯一(转 U-1)
- 导出 album/view source 类型在 exportStore.ts:19-20 预留且 spec 已测 album kind,但零消费(转 U-2)
- npm @tauri-apps/plugin-opener 零真实 import(唯一出现=PluginGate.vue:141 注释「不用」);Rust 侧 tauri-plugin-opener 是 S 线有意不注册的自由函数依赖 ✓consumed→bcdb817(只删 npm 侧)
- .gitignore 裸 `models`(L4)当前零实效命中(根 models 不存在、.models/ 由 L35 独立覆盖);锚定 /models/ 不解除任何现存忽略 ✓consumed→60398a3。另:L1 `!.models/*.py` 白名单排在 L35 目录排除之前,按 gitignore 语义父目录被排除后无法 re-include,疑似死规则(未动,记录)
- src-tauri/.models 无任何 ignore 规则覆盖(若创建会被跟踪;记录,未动)

## 池核实(2026-07-21)
| 编号 | 主张 | 现状 |
|------|------|------|
| R6 | LocalFs 裸 join | 仍成立(storage/local.rs:24-30) |
| F5 | viewportMeta 120ms O(n) 拷贝 | 仍成立(mediaStore.ts:48-63) |
| F9 | MediaFilter 双定义 | 仍存在(types/ui.ts:32-40+media.ts:276-279,或系 S 线有意 UI/API 双型,需核) |
| F10 | — | 已修(pdfjs.ts:20-24) |
| F11 | — | 已修(uiStore.ts:525-537) |
| C41 | prettier 脚本 | 仍在(package.json:11) |
| C42 | — | 已清(本波) |
| C43 | updater 占位 | 仍在(设计如此) |
| C44 | — | 已清(本波) |
| C45 | sync-oss key 无擦除 | 仍成立 |
| C46 | 无 workspace.dependencies | 仍成立 |
| C47 | 注释半边 | 已清(本波),类型感知未启用 |
| C48 | — | 已改进(既有后续工作,非本波;devtools 已接线注释齐) |

## 复核发现处置
#02(thumbCacheDir+SettingsView)深审 1 警告(epoch 竞态,已修)+1 存疑(展示形态,裁不动);其余四轮复核零发现。

## 最终报告(阶段 4 产出,2026-07-21)

### A. 施工报告
总账:**共 22 项 → 已施工 6 / 已裁决 2 / 未裁 10 / 阻塞 2 / no-go 2(聚合)**。门禁终态:cargo clippy --workspace -D warnings **全仓复绿**(本波清掉最后一个 pre-existing 告警)+ cargo test --workspace 835 过 + vitest 1336 过 + vue-tsc/eslint 零错(均本地非 CI)。

- #01 scan_commands manual_inspect 修复 已施工 commit d07c119(map→inspect,cavecrew 复核无发现)
- #02 thumbCacheDir 变更同步+epoch 竞态防护 已施工 commit 81404b7(reviewer 深审→精修→复审;展示保持 OS 原生形态)
- #03 useHoverPreview 3 处 convertFileSrc→resolveAssetUrl 已施工 commit 93fe1e9(三入参实证后端绝对路径,不走透传分支)
- #06 删 npm 死依赖 @tauri-apps/plugin-opener 已施工 commit bcdb817(仅 npm 侧;Cargo 侧系 S 线有意依赖不动)
- #07 eslint.config.js 头注如实化 已施工 commit 0262bfe(仅注释,零配置变化)
- #08 .gitignore 裸 models 锚定 /models/ 已施工 commit 60398a3(check-ignore 实证零忽略解除)
- #12 F10/F11/C48 池条目核实为已修 已裁决(todo.md 注记,commit 1da1a17)
- #21 两审查报告余池其余项 已裁决(维持既有「Boy Scout 随线并入」处置,不独立清账)
- #04/#05/#09-#11/#13/#15-#18 未裁(见决策清单 U-1..U-10)
- #19 cargo fmt --check 基线红 阻塞(backup/core.rs、backup/restore.rs、db/queries/export.rs、export/core.rs、ipc/export_commands.rs 共 5 文件,均非本波改动;归属备份/导出线)
- #20 worklog-kit check 基线红 阻塞(9 处 frontmatter 违反,分属图片编辑线与日志重构线 docs;本波路径全过,index 门绿)
- no-go(聚合):①push/收口/归档类=铁律禁令;②GUI 真机验收类+各 defer 红线类+待拍板类=既有裁决与授权边界(C43 updater 占位、WAL 慢源「暂不改」、U-P1-b「不自动开工」等均维持原判)

### B. 决策清单(本波裁决 D + 移交用户 U)
- D-1 #02 展示形态不动,迁移收窄为「变更同步+epoch」:无人值守不改用户可见形态;深审证实同步是真值。单提交可 revert;影响面=thumbCacheDir 四消费方。
- D-2 池类维持既有 Boy Scout 裁决,仅清 chore 档确证项:避免与既有 defer 裁决冲突。
- D-3 两处门禁基线红不代修记阻塞:本波铁律+记忆红线「docs 门禁基线红属收编线勿代修」。
- D-4 C42 只删 npm 侧:S 线 reveal 裁决(自由函数依赖有意不注册)为准。
- U-1 scanStore 2 处裸 listen 迁移(低):A=移到 store setup 同步注册(改惰性语义) B=effectScope 手工包装(新模式)。推荐 B。卡点=useTauriListen 需同步 scope 上下文,方向不唯一。
- U-2 导出 album/当前视图 入口接线(中):A=CollectionsView 卡片右键+当前视图工具栏各加一入口 B=只做其一。推荐 A。卡点=UI 放置与选区描述符形态需拍板,GUI 面无法自动验收。
- U-3 R6 LocalFs::abs 裸 join 无越界检(中偏高,安全卫生):A=组件级 starts_with 边界检(export 线同款) B=canonicalize(会破不存在路径)。推荐 A。卡点=方向需设计,storage 层影响面广。
- U-4 F5 viewportMeta 120ms O(n) 深拷贝(低):随画廊/canvas 线并入,需实测。
- U-5 F9 MediaFilter 双定义(低):先核是否 S 线有意 UI/API 双型,再定合并与否。
- U-6 C41 npm run format 脚本(低):A=删脚本(CLAUDE.md 已禁用) B=收窄范围。推荐 A。卡点=动他人工具链。
- U-7 C45 sync-oss deploy key 无擦除(低):加 always() 擦除步。卡点=CI 管线本地不可验。
- U-8 C46 workspace.dependencies+thiserror 1/2 并存(低):大重构,随版本升级波做。
- U-9 C47 启用类型感知 lint/noUncheckedIndexedAccess(低):会爆量错误,需专项。
- U-10 C3/C4 查询索引类(低):加索引=schema 迁移,触闸门④,随 DB 波做。
- 移交提示:docs 门 9 处基线红是否转交图片编辑/日志重构两线各自收口,待用户示下。

### 本波未覆盖
- docs/planning 28 条线三件套未逐线深挖内部小尾巴(只扫索引面+todo/MEMORY 提及项)——量大且多数遗留=GUI/push/待裁,边际低。
- 两审查报告非抽样部分(07-06 R4/R12-R22/F3/F6/F7/F14-F19、07-10 C1-C40 非 chore 档)未逐条时效核实,维持池内。
- 仓外目标(worklog-kit 仓、官网仓、私有 pro 域)与 CI 真跑(自托管 runner 因主机硬件不稳冻结)不在本波范围。

## 接续报告(阶段 5 产出,2026-07-21 同日第二次会话)

### C. 决策清单 U-* 终态(接续阶段 4 决策清单;上方 A/B 节按当时状态存档不改)
| ID | 阶段 4 推荐方向 | 终态 | commit |
|----|----|------|--------|
| U-1 | A/B 两案,推荐 B | 采纳 B:scanStore 富化裸 listen 迁 detached effectScope+useTauriListen | cb696dd |
| U-2 | A/B 两案,推荐 A | 采纳 A:相册卡右键+当前视图工具栏两导出入口 | 5a97bd4 |
| U-3 | A/B 两案,推荐 A | 采纳 A:LocalFs::abs 组件级路径边界检+9 测 | e3fe2b4 |
| U-4 | 随画廊/canvas 线并入 | 维持挂起,去向未变 | — |
| U-5 | 先核双型意图再定 | 考据=非有意双型,零消费死码→删 | 00f45bb |
| U-6 | A/B 两案,推荐 A | 采纳 A:删 repo-wide format 脚本+.prettierignore 头注如实化 | 78a611b |
| U-7 | 加 always() 擦除步 | 采纳落地(CI 未实跑,YAML safe_load 语法级验证) | 473311c |
| U-8 | 随版本升级波 | 维持挂起,去向未变 | — |
| U-9 | 需专项 | 维持挂起,去向未变 | — |
| U-10 | 随 DB 波(schema 迁移) | 维持挂起,去向未变 | — |

### U-5 考据结论
非有意双型设计。`types/ui.ts` 版本自 2026-06-02 `d71789d` 引入即零消费者;07-10 审查报告 C36 已判定为死码在先。本波直接删,非合并契约变更。

### 深审处置(三项施工经独立 reviewer 深审)
- **U-1**:深审 2 建议已采、1 存疑(注册 fire-and-forget 时序弱化)裁「接受+注释存档」——理由:富化事件远滞后 + `enrichmentDone` 幂等,竞态窗口无实害。
- **U-2**:两轮深审。首轮 3 严重(系统夹误入 collection scope、右键未按 kind 门控、ViewStale 重试借用全局选区换靶)+2 建议+1 存疑,全修;复审 0 严重、3 建议全采(头注、close 清回调、`resolveViewStaleRetry` 可测种子+特征化 spec)。裁决落地:系统夹不开导出入口(三层防御,导出路径改为「打开后导出当前视图」);ViewStale 重试回调分流,缺回调宁失败禁借全局选区。
- **U-3**:2 严重(Linux CI 假红,已修)+1 建议(CurDir 例,已采)。

### 顺手发现(backlog,仅登记不修,本波未动)
- `.github/workflows/sync-oss.yml:74` known_hosts 无限追加,建议改覆写/去重。
- `scanStore.ts:359` thumbGen 裸 listen 平行写法(`??=` promise 幂等),与 U-1 迁移模式不同但非缺陷,未纳入本次迁移范围。
- `AppToolbar.vue:299-300` 注释「1 个 data-toolbar-item」与 G8 拆分后现状不符(既存偏差,非本波引入)。

### 用户裁决(2026-07-21):docs 门基线红转两线挂账
docs 门 9 处 frontmatter 基线红(worklog-kit check 实测,见下)全部分属图片编辑线(3 文件 4 处)与日志重构线(5 文件 5 处),用户裁决转交两线各自收口自办,todo.md 对应段落各挂一条「修自家 planning docs frontmatter(清账波移交)」,本波不代修。

### 验证终态(本次接续,全本地非 CI)
`cargo clippy --workspace --all-targets -- -D warnings` EXIT 0;`cargo test --workspace` EXIT 0(`storage::` 定向 17 过);`npx vue-tsc --noEmit` EXIT 0;`npm run lint` EXIT 0;`npx vitest run` 全量 109 files/1345 passed EXIT 0(基线 1336→1345,+9);`cargo fmt --check` 仍基线红 5 文件(非本波,不动)。docs 门:`npx --yes --package worklog-kit@0.1.0-alpha.4 worklog-kit check` EXIT 1(9 处基线红全属两线 frontmatter,本波 4 目标文件全过)/`worklog-kit index` EXIT 0。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|

### 决策清单(接续段三,待用户裁决)
- **U-11** docs/status/ 两未跟踪文件(图片编辑功能升级.md/批量15项问题清单.md)处置。紧迫度:中。A=提交入库(status/ 系在册分区 D-016,frontmatter 完好,内容像定稿)B=删除。推荐 A。未自动做原因:基线 untracked 属 dirty 红线,且上一会话未提交意图不明。**✅2026-07-21 用户采 A,已落地 `c2af61f`。**
- **U-12** 12 个 `*.bak-2026*` 备份文件清理(.claude/skills/planning、.worklog、日志线 attachments)。紧迫度:低。A=删除(原件均 tracked 且 clean)B=.gitignore 屏蔽保留。推荐 A。未自动做原因:删除不可逆+基线 dirty 红线。**✅2026-07-21 用户采 A,12 文件已删(untracked,无 commit)。**
- **U-13** 退役 docs 门脚本 tools/check_docs.mjs + tools/check_docs_index.mjs 处置。紧迫度:中(本波已实际造成一次 223 红误判)。A=删除并同 commit 清 docs/README.md 等引用(worklog-kit 门已接管)B=修 STATUS_ENUM 对齐后留作本地工具。推荐 A。未自动做原因:动工具链+删除类;且该欠账归属 oss-worklog-kit 收编第三段既有挂账(批量15项线记忆红线明载「勿在别的线代修」),须随该线或经明示裁决处理。**✅2026-07-21 用户采 A(明示裁决满足红线前提),已落地 `08045d8`(删两脚本+worklogs/README 两处活引用同 commit 更新)。**
- **U-14** 备份线自动化测试硬化专项立项(DB 并发一致/文档屏障交错/跨机 rebase/包完整性/restore crash/retention)。紧迫度:中。A=立独立专项 B=维持现状。推荐 A;注意既有红线「deferred 勿轻补」(#5 指纹守卫/#10 迁移加固)须在专项内单独裁。未自动做原因:超 300 行+需新测试基建。**✅2026-07-21 用户采 A,已立项 `d0f34b9`(三件套 docs/planning/2026-07-21-备份测试硬化专项/+todo 登记;立项≠开工)。**
- **U-15** 审查9项线 F-03 事务故障注入 seam + F-07 真源 COM mock 自动化缺口。紧迫度:低。A=并入 U-14 专项 B=独立小项 C=接受缺口。推荐 A。未自动做原因:注入 seam 设计方向不唯一。**✅2026-07-21 用户采 A,并入 U-14 专项(`d0f34b9`)。**
- **U-16** 2026-07-13-新立项视角全面Review 线(卡阶段4/5:汇总+独立抽查+报告落盘)去留。紧迫度:低。A=标 superseded 归档(其后多轮深审已覆盖主要价值)B=续做。推荐 A。未自动做原因:废线属收口类动作需授权。**✅2026-07-21 用户采 A,已收口归档 `371687f`(closeout F-001~F-008 全处置;实况更正:阶段4/5 主体 2026-07-13 已完成,欠的只是收口;新增待裁 F-008 双综合报告现行归属,见 todo/status)。**
- 既有维持不新裁:D-001(ESC 兜底)、文档缩略图线 D-C/D-D、未完成工作梳理线 F-002/F-003(本波因其三件套基线 dirty 未碰)、上线前三功能方案 trio A/B/C 裁决簿时效对账(建议随各线收口统一)、md 阅读器线收口待授权、清账波 U-4/8/9/10 挂起、fmt 5 文件红与 docs 门 9 红挂账维持。
