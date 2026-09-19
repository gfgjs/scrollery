---
status: active
type: working-memory
line: 批量15项问题清单
created: 2026-07-17
---

# 进度日志:批量15项问题清单

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:建三件套;五路并行摸底 15 项全收(证据在 findings.md);排序分批(task_plan「施工序列」);易批 6 项施工完:
  - #3 MediaGrid onActivated 补焦点收回(nextTick 后焦点无主才 focus,preventScroll)
  - #14 AppSidebar 容器查询锚点 + AccordionSection 标题 3.5em 保底/gap 2px + FoldersSection 头部钮压 24px、窄于 224px 藏两低频钮
  - #4 viewer-sidebar-toggle 低调化(去边框投影、静息 opacity .55、hover/focus 显形;immersive-exit 属 S5 硬编码豁免未动)
  - #8 AppStatusBar 侧栏隐藏时显设置入口钮(22px 紧凑档)
  - #11 scanStore:enrichmentDone 终态账本防 Channel/事件跨传输乱序复活 + 10min 富化怠速看门狗(视频批静默期长,不可低于此)
  - #2 后端抽 run_thumbnail_generation(reset_all) 共享流水线,新命令 start_incremental_thumbnail_generation;前端 store 双包装 + ToolsSection 双钮(增量 Play/全量重建 RotateCcw) + i18n 双语
- 验证:npm run typecheck ✅ 零错;eslint 改动 11 文件 ✅ 零警;cargo check ✅ 37.35s Finished;cargo clippy ✅ 零输出;cargo fmt --check ✅;vitest 88 文件 1184 测试全过 2.84s
- 遗留:GUI 手感项(⏸ #3/#4/#8/#14 真机)待用户验收;接续中批 #6 #5 #12 #1 #10 #15

## 会话:2026-07-17(续,中批+难项)
- 做了:
  - 中批 6 项施工完(#6 扫描期宫格 / #5 手柄开关 / #12 Ctrl+W / #1 无缝分组 / #10 EXIF 提速 / #15 Canvas 卡顿),提交 1866ea0+aa59b7f+e63c1d1;决策表补 D-001~D-004。
  - 难项 #7/#9/#13 设计稿落 designs-难项方案.md(#13 发现零期现成:filename 信息元素已存在)。
  - 候选 F-001~F-006 登记;todo.md 加「进行中未登记」指针行(循 2026-07-12 uiux 线先例,收口才正式回写)。
- 验证:
  - 中批四项批次:vue-tsc ✅ / eslint 14 文件 ✅ / cargo clippy+fmt ✅ / vitest 88 文件 1184 全过 / cargo test --lib layout:: 90 过(含新增 2 个 seamless 特征化)。
  - #10 批次:cargo test --lib scanner:: 42 过 + scanner::metadata 8 过(含新增 3 个 HeaderBuf 测试);clippy 零告警;fmt 整形后复检过。
  - #15 批次:vue-tsc ✅ / eslint ✅ / vitest 1184 全过。
- 遗留(等用户):
  - **真机 GUI 验收清单(⏸)**:#3 退大图按方向键;#14 窄侧栏「文件夹」全显+窄于 224px 藏两钮;#4 看图台左上钮观感;#8 藏侧栏后状态栏设置钮;#11 加文件夹后「正在扫描…」按时消失;#2 工具区双钮增量/重建;#6 扫描期宫格↔扫毕等高切换;#5 设置关手柄后选中格无手柄且 Canvas 拖不动;#12 Ctrl+W 按 closeBehavior 反应;#1 分组下开无缝 toggle 视觉连续且组序保持;#10 富化速度对比(原 500 张/1-2s);#15 Canvas 按住 ↑/↓ 边界卡顿是否消失。
  - **待裁决**:D-A relink 立项 / D-B content_hash 地基(解锁 #7 自动识别+#9) / D-C #13 一期文本卡画文件名 / D-D #13 二期预算;ESC 全局兜底做不做;#1 无缝下是否要保时间轴(需单独产月桶)。
- 门禁备注:`node tools/check_docs.mjs` 当前在 **HEAD 基线即红**(212 违规+1 索引漂移,实测 stash 对比确认)——旧门期待中文 status 枚举,而语料/.worklogrc 已 ASCII 化,属 worklog-kit 收编中间态(第三段待办),非本线引入;本线新档循新 ASCII 约定,未新增违规类别,未越线代修(收编线红线)。

## 会话:2026-07-17(续,用户裁决三难项)
- 用户裁决:**#7 = 方案 A** / **#9 = 暂不做** / **#13 = 新建三件套,新会话单独接续做**。
- 做了:
  - **#7 方案A 施工落地(a66d01a)**。两路 Explore 先摸底,施工首步即推翻设计稿三处旧信息
    (InvalidMove 非预留 / Unchanged 判据只比 mtime / path 有 UNIQUE 且无对应 DAO)——详见 findings
    「#7 方案A 施工实证」表与 designs 稿「地面事实更正」表。
    后端:`AppError::Relink` 三稳定码 + `update_scan_root_path` / `sample_root_items` DAO +
    `relink_scan_root` 命令(校验目录 → 查 UNIQUE 冲突 → 抽样 100 项 stat 比对 size+mtime、
    命中率 <95% 即拒 → 改路径 + 重探卷 + bump + 树快照失效);抽样在读池做(含 N 次文件 IO 不可持写锁)。
    前端:根节点右键「文件夹已迁移…」(判据 `parentKey === null`)+ 二次确认 + 按 code 分流三种话术 +
    成功后补发增量重扫兜底。
  - **#9 no-go 结案**:designs 稿加裁决横幅,评估正文保留作未来重启的现成账(重启前置 = D-B 拍板)。
  - **#13 拆独立线**:`docs/planning/2026-07-17-文档缩略图叠加标题与章节/` 三件套已建。一路 Explore
    核实旧稿 6 条(5 真 / 1 语义偏松),**揪出 3 处过度承诺**(零期非零操作 / epub title 有 `?` bail
    陷阱、恰对最需要它的无封面 epub 失效 / text_index 整读全文不可复用于烘焙路径)。D-C/D-D 随该线裁。
  - **顺带修 exotic 三个陈旧档位测试(76ad9ea)**,非 15 项——详见「错误账」与 F-007/F-008。
  - 候选补登 F-007~F-009、D-005~D-007。
- 验证:
  - #7 批次:vue-tsc ✅ 零错;eslint 4 文件 ✅ 零警;cargo clippy ✅ 零告警;cargo fmt --check ✅;
    vitest 88 文件 1184 全过(含 i18n 双字典完整性锁,新增 13 键两侧对齐);新增 6 个 Rust 测试全过。
  - exotic 批次:**`cargo test --lib` 628 全过 0 失败**(修前 625 过 / 3 红;基线红已 stash 对拍确认非本线引入)。
    clippy 零告警,fmt --check 过。
  - 工作树干净;两个 untracked planning 目录属别的线,未纳入(显式 pathspec 提交)。
- 遗留(等用户):
  - **真机 GUI 验收清单(⏸)**:上一会话 12 项 + **#7 新增 1 项** —— 根节点右键「文件夹已迁移…」→
    选迁移后的新路径 → 核对通过应免重扫免重生成(观察缩略图是否秒出、无重生成进度);
    另测三种拒绝话术(选个不存在/非目录 → not_a_dir;选已被别的根占用的目录 → path_taken;
    选一个不相干目录 → mismatch 且带统计数)。
  - **待裁决(仅剩 3 项,均不阻塞)**:ESC 全局兜底做不做(D-001);#1 无缝下是否要保时间轴(D-003,已消解:落地为无缝 minimap 轴线);
    D-B content_hash 地基(现只余「#7 自动识别」这一个可选收益——#9 已 no-go,方案A 已覆盖迁移主场景)。
  - **#13 的 D-C/D-D 不在本线**,随 `2026-07-17-文档缩略图叠加标题与章节/` 线裁。
- 门禁备注:`node tools/check_docs.mjs` 仍在 **HEAD 基线即红**(旧门期待中文 status 枚举 vs 已 ASCII 化语料,
  worklog-kit 收编中间态,第三段待办)——本线新档循新 ASCII 约定,未新增违规类别,未越线代修(收编线红线)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
