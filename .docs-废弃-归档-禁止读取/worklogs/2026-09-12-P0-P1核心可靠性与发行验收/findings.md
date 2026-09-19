---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-12
---

# 发现与证据

## 基线
- 起点dev/2118ff52；前轮分析文档未提交，另有用户原先两项删除，全部保留。
- Vitest1781/Rust1495通过（12忽略）；NOTICE/改名红门；未有安装包验收。
- 验收标准源于docs/reviews/2026-09-12-仓库架构与流水线全面梳理.md第8节。
- 文件移动物理动作先于DB事务，媒体卷身份未更新；旧卷下线可误标offline。先表征再修改。
- AI/face会话owner、CPU预算、GPU批许可职责不同；补等待/唤醒，不盲目合并。
- quick按目录mtime剪枝；完整重扫应发现文件级变化。不引入未授权watcher扩建。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | quick中断会留下提前更新的目录mtime，重启后不能仅凭前端Set保证基线完整 | 扫描修复/回归与状态 |
| F-002 | 语义搜索清空只改前端，后端共享表和模型缓存快照需共同受请求身份约束 | 搜索修复/回归 |
| F-003 | move_directory以cache_key相同直接continue，跨根同相对路径时会遗漏source-direct绝对路径更新 | 文件移动修复/回归 |
| F-004 | 独立验收不能只看exe路径/文件名；启动前必须核对验收配置身份和产物hash证明 | 验收脚本与复验记录 |
| F-005 | 新增RAW GNU独立workspace验证与CI；默认Windows验收准备完成，既有perf overlay非法注释键另记边界 | 工程验收/后续状态 |
| F-006 | 目录移动恢复须证明目标归属；同名同尺寸不能证明源残留可删；缺目标不得假收尾 | 移动恢复与回归 |
| F-007 | 新增慢盘await后必须复验布局代次；自动续跑在状态查询及起步命令期间均可被暂停撤销 | 状态顺序回归 |
| F-008 | Tauri app manifest一旦存在，既有custom IPC也进入权限检查；声明新命令须覆盖实际注册清单，防旧API被拒 | 构建权限接线与原生验收 |
| F-009 | parseAppError原只保留code/message，恢复id/路径会被丢；文件恢复后端与UI/撤销必须同次接通 | IPC错误与目录恢复前端 |
| F-010 | 原生验收须对齐实际目录/Channel/缓存/导出接口，锚定本轮备份并先改值再恢复；退出看进程事件，失败必须落报告且非零 | 完成报告/验收驱动 |
| F-011 | 源清单使用NUL避免中文路径漏记；构建源、runner修订及并行草稿分记；安装应与包内载荷比hash | 完成报告/溯源记录 |
| F-012 | 三个externalBin握手不覆盖可选exotic插件，实际PSD hash_mismatch拒拉须保留后续项 | P2格式与供给状态 |

## 实现前补充核验
- start_ai_analysis资源忙目前为AppError::System中文字符串；等待逻辑应使用稳定code而非前端解析中文。共享error文件变更由主会话协调。
- fast_scan.rs有明确中断残留窗注释，ManagementSection只有'重新扫描'；完整scan接口已有quick=false，需暴露准确入口及失败后持久回退。
- semantic_search_cmd先worker编码再host评分，编码锁不覆盖后续评分/DB提交；aiStore空查询仅重置UI，不清DB，也需及时清loading/count并防迟到复活。
- P0主审退回第一版恢复：intent双端存在直接认领、源残留只比尺寸、published未查目标可用性，以及DB锁覆盖FS；尚未通过验收，不可用于原件。
- P1-2启动产物身份已保护；补查CDP端口占用和连接目标身份，防止独立产物启动后附到别的既有实例。
- 跨卷复制的payload凭据必须在发布前成功持久化；不可用let _忽略写入失败后继续发布。恢复测试应覆盖发布前后崩溃以及凭据写被DB拒绝。
- 扫描finalize的缺失检测返回Ok(0)可能意味着不完整/离线守门，不表示基线完整；dirty清理必须另验walk_complete&&volume_online。
