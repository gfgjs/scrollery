---
status: 快照
type: 工作记忆
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-17
---

# 进度日志:上线前三功能方案(导出 / 数据备份 / 图片简单编辑)

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:建线;三路只读 agent 摸底并回填 findings(数据模型 / 图像管线 / 文件操作与 IPC 面);todo.md 核对无既有同类线;三份设计稿落盘(方案A-导出整理成果 / 方案B-数据备份 / 方案C-图片简单编辑);task_plan 汇总建议(施工顺序 B→A→C + 两条跨案裁决);F-001..F-005 候选登记。
- 验证:纯文档线,无代码改动。docs 门禁两道均为 **HEAD 基线既有红**(属收编线,勿代修——记忆红线):①check_docs 全库 216 处同一类违规(frontmatter `status: active` vs 中文合法值「现行|施工中|快照|已归档|已废弃」),含 HEAD 已入库同类样本(2026-07-17-树内文件行打开与拖拽/设计-path预览与文件行拖拽.md、批量15项/designs-难项方案.md);本线 3 份设计稿同类 +3,**无新违规类**,三件套本身豁免未标;②check_docs_index 红=docs/README.md 缺「下一空闲字母」声明,本线零触碰 README,既有漂移。
- 依据:全库 0 个文件使用中文 status 值,`active` 为全库惯例;收编线裁定前随惯例,若收编线改门或改值,本线 3 稿随大流一并迁。
- 遗留:待用户审阅三稿并逐项裁决(A-1..A-4 / B-1..B-4 / C-1..C-4 + 跨案两条);裁决后拆施工线、本线收口。

## 会话:2026-07-18

- 用户要求:审阅 A/B/C 三方案并更新对应文档;特别提醒方案形成后代码已有大量改动。
- 复审范围:以方案提交 `247d05c` 为旧基线,检查当前 schema/selection/长任务状态/AppStatusBar/documents 存储/图像编码与查看器旋转等相关实现。
- 已更新:
  - 方案 A 改为后端 `SelectionDescriptor` 有序解析、专属 staging 目录、manifest 隐私约束、V20 显示旋转提示、event+snapshot+generation 可恢复任务模型。
  - 方案 B 改为完整 DB+documents 快照,增加 documents 一致性 guard、跨机绝对路径 rebase、预恢复回滚包、phase marker 崩溃矩阵和动态 schema gate。
  - 方案 C 增加 V20 `view_rotation` 物理烤入、元数据/色彩 P0 golden spike、峰值内存基准、GIF/HEIC/AVIF 延期、temp 内完成元数据后发布以及 `savedNeedsIndex` 部分成功语义。
  - task_plan 更新施工估算与待裁决索引;findings 把 2026-07-17 摸底标为历史快照并登记 F-006..F-012。
- 边界:纯文档复审,未改业务代码;未触碰工作树中并行的 `.worklog`、其他 planning/status/lines 变更。
- 验证:首轮 `node tools/check_docs.mjs` 失败 219 项,其中本线 A/B/C 三稿仍是旧 `active/design`;按当前 schema 改为 `施工中/设计方案` 后复跑降至 **216 项既有红**,输出不再包含本线 6 文件。`node tools/check_docs_index.mjs` 两次均失败 1 项:README 缺「下一空闲字母」声明,本线未改索引。`git diff --check -- <本线目录>` 通过(仅换行转换提示)。
- 遗留:门禁/diff 复核;用户裁决 A-1..A-6、B-1..B-6、C-1..C-6 后另拆施工线。

## 会话:2026-07-19

- 用户要求:再审 A/B/C 三稿并更新相关文档。
- 复审范围:5963c9a..HEAD 仅 3 提交(35b1201 收编翻值 / 0bf5ebb F-025 / 5f92f1d 回写),业务面只动 state.rs 与 ai/face 命令。逐项复核三稿承重断言(选区契约/错误模式/状态栏分区/文档两步写/zip·sha2·image·webp 依赖/PRAGMA/relink 参数/V20-V21/交互信号与后台限流),全部成立,证据登记 findings「二次核对」节。
- 已更新:三稿 banner 加 07-19 复核行、last-verified 齐 bump 2026-07-19;A §3.1/B §5.1 补「终态门控姿态采 thumb/derive 的 finish 返回值,勿仿 ai/face 的 is_cancelled」精度(F-025 后 state.rs 两姿态并存);task_plan 加阶段 3b 与汇总行;findings 登记二次核对节 + F-013/F-014;补写 lines 实体使命句与 status 分片滚动状态(两者原为 worklog upgrade 占位)。
- 边界:结论与工程量不变,无被推翻结论;三稿 frontmatter `active/design` 为 35b1201 收编线所定,本线不回翻(F-014)。
- 技术坑:三稿正文含真全角标点(U+FF0C/U+FF1B),Edit 参数层全角被静默转半角致锚串失配,改用 Node 脚本 `\u` 转义字节级替换(toolcall 管道既知坑);三件套本身是半角世界,Edit 直改无碍。
- 验证:`node tools/check_docs.mjs` 末行「✗ docs 门禁失败:220 处违反」——与本轮编辑前基线**完全持平**(全库同类 active 枚举红,含被 35b1201 翻回的本线三稿,属收编线);`node tools/check_docs_index.mjs` 末行「✗ docs 索引新鲜度门失败:1 处漂移」(README 缺「下一空闲字母」,既有漂移,本线零触碰)。`git diff --check` 通过(仅 CRLF 转换警告,同 07-18)。
- 遗留:不变——用户裁决 A-1..A-6 / B-1..B-6 / C-1..C-6 后拆施工线;check_docs 中文枚举 vs worklog-kit 英文值两门冲突待收编线统一(F-014)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
