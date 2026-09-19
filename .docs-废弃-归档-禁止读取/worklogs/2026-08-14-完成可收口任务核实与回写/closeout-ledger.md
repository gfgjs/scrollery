---
id: 2026-08-14-完成可收口任务核实与回写-closeout-ledger
status: snapshot
type: working-memory
line: 完成可收口任务核实与回写
created: 2026-08-14
---

# 收口候选账过程记录(43 线提取全量,subagent 2026-08-14)

## 收口候选账(43 线提取,subagent 2026-08-14)

### 组 2(9 线,已回)
- 2026-07-25-全仓注释清理精简:findings_candidates=none / taskplan_decisions=none / progress_review=filled
- 2026-07-25-最近一周代码实现深度Review:F-001 安装包内容断言缺失(已随 F-01 修复进 CI 门 08-11 verify-bundle-content)/F-002 fresh install 冒烟(发布前专项)/F-003 配置层确定性测试/F-004 增强状态机测试;taskplan=none
- 2026-08-04-AI语义搜索栏UI优化:无候选;progress_review=empty
- 2026-07-17-md阅读器:F-001 内存泄漏分流(稳态 vs 无界进行中,阶梯法)→experience /F-002 CSS multicol 乘积放大器→experience /F-003 section 大小硬约束(md 24K/txt 10K)→design/代码注释 /F-004 chardetng 采样切多字节 bug+「告警不响≠没病」→experience;D-001 分片预算 24K/D-002 护栏收窄 256K/D-003 chardetng feed(last=false)
- 2026-07-17-底栏重构:F-001 Write 工具 NUL 注入→experience /F-002 并行共库选择性暂存(hash-object+update-index)→experience
- 2026-07-17-视频封面关键帧:F-001 无 GPU 解码/F-002 全分辨率 RGB32 浪费/F-003 ORDER BY kind 注释误称/F-004 派生全暂停调度成分/F-005 MF XVP 旋转陷阱→experience /F-006 #[ignore] 诊断单测→experience;D-001 T1c 只砍 probe/D-002 GPU 回退软解/D-003 旋转双属性钉死/D-004 diag_pipeline_env 留仓
- 2026-07-17-阅读器页UI优化:F-001 foliate margin 驱动头脚带(注释已落)/F-002 新建 composable 先 Glob 撞名→experience /F-003 overlay hover 不受 iframe 吞事件
- 2026-07-18-根文件夹显隐:F-001 条件子查询 vs 反规范化→experience/decisions;taskplan F-001 同
- 2026-07-18-缩略图进度:F-001 Tauri Channel 断链,事件+快照双件套→experience;D-001 Channel→事件+快照/D-002 显式 kinds 覆盖/D-003 关键帧单一事实源
### 组 3(8 线,已回)
- 2026-07-19-图片简单编辑施工:F-001 image 读写 ICC/EXIF 原生齐全→experience /F-002 webp 无 ICC/EXIF API(P2 引用)/F-003 内存探针 PeakWorkingSetSize 单调+同进程页面复用→experience(第二层:独立进程 setup 合成源仍腰斩 delta,诚实测法=源由另一进程产出)/F-004 WebP 单边 40000px 硬上限;D-005..D-013(不引 img-parts/v1 不 admit WebP/最小 EXIF blob/预算 12Bpx/D-009 JPEG 仅 orientation/D-010 edit_invalid_ops/D-011 FILE_JOB_EDIT/D-012 单发直跑/D-013 CSS-transform 预览)
- 2026-07-19-数据备份与恢复施工:F-015 save_version 两步非原子(已修,阶段1)/F-016 CASCADE 删行不删文件孤儿(留 GC 线)/F-017 schema CURRENT_VERSION 公开访问器(已加);D-101 单事务/D-102 std RwLock spawn_blocking 内持/D-103 lazy chunk 双语
- 2026-07-22-前端动画重构:findings=none;D-001 ContentViewer 争用区/D-403 spinner 800ms token/D-404 dip 已知限制/D-405 prettier 存量债
- 2026-07-22-应用配置重构:findings=none;D-c01..D-c06(TOML app_data_dir/config.toml/设置状态分离/模板注释态/文件单源+一次性迁移/watcher+debounce+指纹/IPC 兼容+新命令)
- 2026-07-23-全仓深度review与直修:无候选
- 2026-07-23-窗口化沉浸模式:D-422 chromeAutoHidden 第三来源/D-423 分档按窗口态 4/8px/D-424 无延时隐藏定时器/D-425 查看器局部条不动/D-426 setter 先持久化后 state
- 2026-07-23-大图浏览器:F-001 动画由 decode/错误驱动→test+code(已落地)/F-002 旧位图残留→experience /F-003 跨条目撤旧帧→test+code(已落地);D-001..D-003 同
- 2026-07-24-画廊轴minimap解耦:无候选
### 组 1(9 线,已回)
- 2026-08-07-Spec15深化:无候选
- 2026-08-14-过度工程化审查:F-001 worklog venv baseline 污染(.venv 实为 venv)→code /F-002 check_plan_canonical.mjs 孤立脚本→code /F-003 error.rs 13 同形变体→code /F-004 4 套手写虚拟滚动→code /F-005 usePluginEntitlement 零调用→code
- 2026-07-18-两日代码修改审查:F-001 generation-tagged token(已落地 efec238)/F-002 多步 SQLite 单事务
- 2026-07-23-审查应用配置重构:无候选
- 2026-07-24-全仓深度review:无候选
- 2026-07-24-施工规格集:无候选
- 2026-07-24-全仓未完成工作梳理:无候选
- 2026-07-17-上线前三功能方案:F-001 就地覆盖三坑→experience /F-002 最小保护集→experience /F-003 无 fs plugin 蓄意姿态→experience /F-004 zip/dialog 备忘→no-promotion /F-005 db:media_updated 僵尸常量→todo /F-006 Channel 同生命周期→experience /F-007 abs_path staged rebase /F-008 documents 原子性 /F-009 view_rotation 烤入 /F-010 SelectionDescriptor /F-011 元数据矩阵 golden spike /F-012 完整 DB vs 精简态 /F-013 两种终态门控 /F-014 check_docs 枚举冲突;D-001..D-004
- 2026-07-17-未完成工作梳理与free社区版最短路径:F-001 free 硬阻断仅两动作/F-002 free+自动更新相斥/F-003 阅读器 free/pro 划线/F-004 Part0 §10.3 未回写/F-005 根文件夹显隐记录滞后/F-006 上线前三伞形任务过时/F-007 编辑线遗留与 free 无关/F-008 push 标注陈旧
### 组 4(9 线,已回)
- 2026-08-03-canvas设置项:F-001 DOM/Canvas 悬停缩放共享同一开关→test/code(已落地 cb459c4)
- 2026-08-03-画廊大图首次:D-001 路由分包+Promise 预取/D-002 移除 200ms 动画(均已落地 2940600)
- 2026-08-03-UI加载失败排查:F-001 devUrl 固定 127.0.0.1:1420→design/runbook(已落地 0c9ac89)/F-002 关闭依赖前端失联无法退出→code/test(已落地)/F-003 dev-server 存活检查→test/code(部分);D-001 同一故障链
- 2026-07-21-无人值守清账波:U-1..U-10 六项施工四挂起(已落地 cb696dd 等)/U-5 死码删除
- 2026-07-19-minimap审查:F-001 异步 LRU 迟到回写绕过上限→test/code(本次已落代码)
- 2026-07-19-图片编辑v2施工:F-001 虚拟内建付费 feature+PluginGate+稳定码抽可复用能力→后续架构优化;P0-CM/E1/E2/E3×2 决策(已落地)
- 2026-07-19-导出整理施工:F-001 B 线前端未接入(已由 U-2 接线)→completed/F-002 no-promotion/F-003 no-promotion/F-004 experience(测试覆盖判据)/F-005 no-promotion;D-002..D-005(已落地)
- 2026-07-20-日志能力重构:D-301..D-308(已落地;D-308 被 D-310 推翻)
- 2026-07-21-span埋点:F-028 FmtSpan 合成事件自建 Layer 收不到→experience/D-311 记录;D-309..D-315(已落地)
### 组 5(8 线,已回)
- 2026-07-24-RAW:F-037 嵌入预览优先→experience /F-038 builtin distribution 复用→设计规范 /F-039 sidecar 桌面专属约束→设计规范 /F-040 Preview Capability 缺口 /F-041 Limits 分限宽高 /F-042 rsraw Error 私有 /F-043 resolve_worker_path 硬编码 /F-056 共享协议变体脱 workspace crate→CI gnu job+experience;D-427..D-435/D-444/D-445
- 2026-07-24-视频格式扩展:D-444(6 开放问题终裁)/D-441 remux 优先/D-442 Service 型 worker/D-443 builtin+FFmpeg 下载
- 2026-07-18-审查9项修复:F-024 run-generation compare-and-clear→experience(已落地 0bf5ebb)/F-025 ai/face token 槽遗留→no-promotion(已由 0bf5ebb 落地)/F-026 foliate 双层防线→experience(并入内存爆炸线条目)/F-027 latest-write-wins→no-promotion(代码自载);D-001 TXT 后端不切分
- 2026-07-16-后端大文件拆分:F-001 大文件审查分型/F-002 共享 helper 迁移序/F-003 结构性前提失效/F-004 IPC 文件不载共享 helper/F-005 注册清单下沉/F-006 Channel 下载编排归属/F-007 setup owner/F-008 豁免触发器/F-009 复用原工作池;D-001..D-004
- 2026-07-16-文件树与顶栏格式筛选:F-001..F-023(23 项!多指向 experience.md 既有 §14/§18/§19);D-001..D-013
- 2026-07-17-树内文件行:F-001 稳定码三胞胎/ F-002 拖拽尾随 click 抑制→experience;D-001..D-003
- 2026-07-17-画廊无缝minimap:F-001 注释已落地→no-promotion/F-002 滑窗几何单测→experience 候补;D-001/D-002
- 2026-07-17-画廊重排滚动:F-001 锚点续命→experience;D-001..D-003