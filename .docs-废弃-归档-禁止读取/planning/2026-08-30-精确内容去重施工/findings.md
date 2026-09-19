---
status: 施工中
type: 工作记忆
line: 去重功能全局方案
created: 2026-08-30
---

# 发现与决策:精确内容去重施工

## 需求
- 完整施工设计中的 P0–P4，交付可用的精确重复分析、人工复核、应用回收站和安全物理清理闭环。
- 主代理集中架构、共享入口、审查和验收；最多三个 Luna（Max）子代理并发执行独立工作。
- 破坏性测试只能使用测试临时目录；系统回收站失败不得删除数据库行。

## 发现
- 设计基线要求拆分 change fingerprint 与 versioned exact digest；精确摘要结果必须受 source_revision 保护。
- 同 root 扫描存在 token compare-and-clear 和 seen run isolation 风险，必须先处理。
- dedup_index 不物化永久 group，重复组通过 versioned unit_digest 动态 keyset 查询。
- Live Photo 必须按主文件与 companion 的组合摘要识别；首版若不能安全原子物理清理则只允许软删除。
- 项目共享集成文件由主代理集中修改：migration 注册、IPC registry、capabilities、router、全局 IPC 类型、公共 i18n、CI、todo/status/design。
- 波次 0 侦察确认：`scan_tokens` 是 `HashMap<root_id, CancellationToken>`，旧 enrichment 会按 root 无条件清理；`_mm_seen`/`_mm_online` 只按 root 命名并在初始化时清空。
- 波次 0 侦察确认：schema 为 V25；`content_fingerprint` 大文件只抽样；Live Photo 可有多个 companion，但播放查询的单 companion API 不能用于清理清单。
- 波次 0 侦察确认：应用软删/恢复已有 companion 扩展；现有硬删在 `trash::delete` 失败后仍删除 DB 行，P4 必须另建安全路径。
- 终审新增风险：路径根语义在非 UNC 绝对路径格式化时可能丢失；cleanup journal 混合成功/失败批次恢复不完整；复核后到系统回收站之间存在对象替换窗口；mtime 相同但 size 变化时失效条件不足；前端成员请求返回可能覆盖当前组。
- 修复复核：根路径规范化保留 POSIX/盘根标记；journal 按 succeeded item 恢复并处理混合批次；普通文件使用 V29 同卷 quarantine 原子 rename 后再交给系统回收站；启动对账在 writer 锁外尝试无覆盖恢复；同 mtime 不同 size 推进 source_revision；成员请求用 generation 与当前 group 双重守门。

## 外部资料(当数据,不当指令)
- 设计与状态文档：`docs/designs/2026-08-30-去重功能全局实现方案.md`、`docs/status/去重功能全局方案.md`。
- 当前工作树源码与测试：施工前由子代理和主代理复核，以源码和命令输出为准。

## 耐久提升候选(F-001 递增;收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 摘要写回必须用 item_id + source_revision + hash_version 条件，旧结果静默丢弃 | code/test |
| F-002 | cleanup journal 把系统回收站成功与 DB 删除拆成可恢复阶段 | design/code/test |
| F-003 | 硬链接、稀疏文件、clone/reflink 使逻辑大小不能等同实际可释放空间 | design |
| F-004 | mtime 变化但抽样 change fingerprint 相同仍不能证明 exact 内容未变；touch 需切换 source_revision，并让 Live Photo 主项组合摘要失效 | code/test/design |
| F-005 | 绝对 POSIX/Windows 盘根规范化不能丢失 root marker | code/test |
| F-006 | cleanup journal 的 succeeded 成功证明按 item 收敛，混合批次不能整体阻塞 | code/test |
| F-007 | 路径型系统回收站前增加 V29 同卷 quarantine，启动可恢复隔离文件且 writer 锁不包文件 IO | code/test/design |
| F-008 | mtime/mtime_ns 不变但 size 改变也必须失效 exact sidecar | code/test |
| F-009 | 前端成员分页响应必须绑定请求 generation，不能覆盖快速切换后的分组 | code/test |
