---
status: active
type: working-memory
line: 超长文件拆分方案
created: 2026-07-25
---

# 发现与决策:超长文件拆分方案

## 需求
- 用户原话要点:建三件套;全仓查找超长代码文件(例:MediaGrid.vue 126KB);分析拆分方案并落盘;**并行会话在执行精简注释任务,先出方案不动代码**。

## 发现
- 全仓 ≥40KB 代码文件共 45 个;完整榜单见 attachments/尺寸榜单-2026-07-25.md(git ls-files + stat,2026-07-25 快照,并行注释精简会使数字缓降)。
- Tier A 详案 10 件(≥70KB,KB 值):layout.rs 137 / MediaGrid.vue 128(用户例称 126KB,吻合)/ faces.rs 112 / ContentViewer.vue 86 / scan.rs 86 / FoldersSection.vue 83 / MediaGridCanvas.vue 79 / DocumentViewer.vue 77 / lib.rs 72 / worker_client.rs 72。
- 排除:vendor foliate-js(mobi/paginator/epub.js)第三方勿动;i18n locale(en-US/zh-CN.ts ~87/84KB)为数据文件——如需瘦身,拆法是按顶层命名空间分模块文件 + index 聚合,属独立小活。
- git 基线(2026-07-25 开工):并行会话 dirty = enhance-worker/src/run.rs、eslint.config.js、docs/worklogs/2026-07-25-全仓注释清理精简/——本线不碰,收口 pathspec 只提交本任务目录。
- 各文件拆分方案正文在 analysis/ 子目录(详案 10 件 + tierB 简案 4 组),findings 不复述。
- 方案速览表(一文件一行,一文一方一险):

| 文件 | 拆法一句 | 最大风险一句 |
|------|----------|--------------|
| layout.rs | 拆 5 生产+6 测试(目录+facade,测试占 68%) | search.rs 跨域 push_in_predicate 须具名重导出;canonical ends_with 字符串契约动措辞即静默 6.6s 退化 |
| MediaGrid.vue | 14 composable+1~3 子组件,滚动容器+KeepAlive 胶水裁不拆 | useGalleryVirtualEngine 双引擎交汇+轴红线,nextTick 时序一字不挪;MediaGridRow 依赖宿主 scoped CSS 穿透,CSS 不随模板搬 |
| faces.rs | 6 件(facade+status/wall/approval/clustering/recluster) | 尾部测试跨域引用改 crate 绝对路径;唯一可见性改动 in_clause→pub(super) |
| ContentViewer.vue | 11 件(7 composable+4 子组件),媒体舞台不拆 | imgRef/videoRef DOM 测量强耦合;updateZoomRatio→recomputeFaceLayout 隐式调用序 |
| scan.rs | 5 件 scan/ 目录,facade 与外部调用方零改动 | pub(in crate::db::queries) 限定符勿改窄;唯一手改行 use super::faces 转绝对路径 |
| FoldersSection.vue | 7 composable,template/style 不拆 | 三处裸闭包变量须显式重包装,非纯剪切;贴边滚动四态整体搬 |
| MediaGridCanvas.vue | 7 件(palette/painters/infoOverlay/cellRenderer + 3 composable),显式参数工厂 | 工厂调用勿入 rAF 或逐格循环;HANDLE_INSET/SIZE 几何单源 |
| DocumentViewer.vue | 12 件(DocToolbar/DocEditPane + 10 reader composable,零互引) | remount 时序(先 captureCurrentPosition 再 reloadToken++)与 viewerApi 契约须引用透传 |
| lib.rs | 10 件按域 boot 化,run() 退化编排壳(D-450) | 9 条链式启动顺序不变量;capabilities 一一对应不变 |
| worker_client.rs | 5 件目录化,测试 49% 优先搬离 | 5 个私有方法升 pub(super);泄漏隔离与重试预算语义不动 |
| message.rs | 缓——先迁 mod tests(~49%),算子域拆二期 | serde tag 枚举逐字剪切,host+worker 共担契约 |
| worker_service.rs | 小中拆 3 件(tests/types/runner) | Arc<SharedState> 跨线程状态机 review 易看错时序 |
| state.rs | 缓——仅 RunTokenSlot 迁出 | 活跃 WIP 高频接触点,易合并冲突 |
| fast_scan.rs | 缓——四个测试模块迁出+载荷类型可选 | seed_gate_admits 裁决点不碰;private helper 靠调用序耦合 |
| video_commands.rs | 拆缓半步 3 件 | PREPARING_OUTPUTS、PLAYBACK_PROGRESS 两 static 命令壳与内核双向读写,接口划分不清易漏同步 |
| supervisor.rs | 缓——仅 stderr 行日志转发迁出 | 与字节环形缓冲系 D-313 已裁双轨,误合并即违裁决 |
| SettingsView.vue | 拆 3 composable(cacheStats/iccProfile/scrollSpy) | 模板同名绑定,返回值名与响应性不保留即静默错位 |
| enhance/service.rs | 小切口 2 件(EXIF 纯函数簇为主刀) | 准入校验 pub fn 疑被 enhance_commands.rs 直调,搬前须 grep |
| media_foundation.rs | 仅安全尾段 2 件(属性辅助+帧后处理) | MF 异步回调红线区原地不动;tests 经 super:: 双向引用 |
| schema.rs | 按版本纪元三切(early/mid/late) | STEPS 23 项与子文件常量一一对应,人工计数核对 |
| thumbnail_commands.rs | 按流水线两切(全库生成域迁出) | 8 处引用点须 pub use 转发,漏改即编译失败 |
| derivations.rs | 测试分离(53% 为 6 个 cfg(test) mod,零耦合) | 各测试私有治具搬家不得顺手去重 |
| items_cache.rs | 缓——2 件候选(GlobalFilenameRank/patch 写路径) | 「单一事实源」注释跨文件后契约易漂移 |
| models.rs | 拆约 6 域文件+mod.rs 全量 pub use | serde rename_all 属性搬运手误致 wire 格式静默漂移 |
| justified.rs | 拆 3 件(geometry/justified_pack/grid_pack) | layout_groups_parallel 泛型勿改 dyn,热路径勿引入间接调用 |
| face_pipeline.rs | 拆 4 件(producer/decode_source/dispatch/writer) | is_cancelled 门控散落 5 处逐点核对 |

- 顺手发现(本线 D-449 不动码,只挂账):
  - faces.rs r2_6_query_tests 前 doc 注释与测试内容不符(疑复制遗留);并行注释精简线可能顺手消化,收口时核对。
  - layout.rs view_to_sql_tests/selection_resolve_tests 含 collections/faces/media 跨域测试就地挂靠(历史 T 线遗留),归属回迁建议单独立项。
  - MediaGridCanvas.vue idAtClient/pick/pickWithRow 三处坐标换算重复,拆分施工阶段抽 clientToLogical(e) 共享。
- CSS 外置可行性核实(2026-07-25,D-451):全仓 .vue 样式零 v-bind();MediaGrid/MediaGridCanvas/ContentViewer/DocumentViewer/SettingsView/FoldersSection 均单一 <style scoped> 块;MediaGrid style 块行号已从分析时 2313 漂至 2234(注释精简线在缩文件,D-448 符号锚点裁决获实证)。

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-ID 取全仓全局序递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-057 | 全仓超长文件治理分层榜单 + analysis/ 方案索引(45 件:A10/B16/C14/排除5) | 已同步(2026-07-25 施工线 P6 收口):A 详案 10 + B 简案 14 全落 docs/todo.md,C 观察名单 14 件按方案维持不施工未动 |
| F-058 | layout.rs 跨域测试归属回迁(collections/faces/media 测试挂靠 layout)单独立项 | docs/todo.md 或收口裁 |
