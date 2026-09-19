---
status: active
type: working-memory
line: 超长文件拆分施工
created: 2026-07-25
---

# 任务计划:超长文件拆分施工

## 目标
按方案线 `docs/planning/2026-07-25-超长文件拆分方案/analysis/` 14 份方案施工:Tier A 详案 10 件 + Tier B 简案 16 件(「缓」项只做方案列出的最小动作),每批过复核后 commit;C 观察名单不施工。

## 当前阶段
阶段 1(CSS 首刀)∥ 阶段 3(Rust 详案)并行开跑

## 依赖 DAG
```
P1 CSS首刀(MediaGrid.vue, D-451 实测 Vite 构建链)
  └─ P2 CSS铺开×5(ContentViewer/DocumentViewer/FoldersSection/MediaGridCanvas/SettingsView)
       └─ P4 Vue结构拆×6(同文件依赖各自 CSS 外置先落)
P3 Rust详案×5(layout/faces/scan/lib/worker_client;互不相交,worktree 并发)   ← 与 P1/P2/P4 全程无依赖
P5 tierB Rust×14(除 SettingsView 归 P4、derivations.rs 待裁)                  ← 与前端线无依赖
P6 全量清算+收口(依赖全批落地)
```

## 阶段

### 阶段 1:CSS 首刀(MediaGrid.vue style 外置 + Vite build 实测)
- [x] MediaGrid.vue `<style scoped src>` 外置,commit ba3abde(byte-identical,typecheck/build/eslint 全 0,快扫无发现)
- **状态:** complete

### 阶段 2:CSS 铺开 ×5
- [x] ContentViewer/DocumentViewer/FoldersSection/MediaGridCanvas/SettingsView 同法外置,commit fa6e8e2(同法,快扫无发现)
- **状态:** complete

### 阶段 3:Rust 详案 ×5
- [x] P3 Rust 详案 5/5 全进 dev——layout(5bc6fba+c2de682)、faces(33a4cd8+63e22d5)、worker_client(f6f2932)、scan(c2bb738+b9d666b)、lib(e92e2b2+de95b78,深度复核 0 严重 1 警告 1 建议 1 存疑全处置:StartupFailure 走 thiserror+删死字段=de95b78,段 c/d 序采方案 §2.3 显式裁决入 commit);全部深度复核 0 严重
- **状态:** complete

### 阶段 4:Vue 结构拆 ×6
- [x] MediaGrid(14 composable)——3e8559e,复核 0 严重 0 警告,轴测试补 f72f71b 28 用例
- [x] ContentViewer(11 件)——8cde88f,复核 0 严重 0 警告,toggleFav 守卫偏差已注 commit,特征测试 7f54155 补齐(faceToken 并发丢弃+pendingInitialRotation 一次性复原)
- [x] DocumentViewer(12 件)——3e07612,复核 0 严重 3 警告全修(capture-first 注释表述/printWidth×2/卸载次序注释);方案 §5 五 spec 补 9f02a5f 27 用例
- [x] FoldersSection(7 composable)——2a73de5,复核抓 1 严重已修(索引入参 computed 包装掐死 triggerRef 穿透,改直传 ref+2 例回归锁,增量核验确认);fileTitle 参数化
- [x] MediaGridCanvas(7 件)/ SettingsView(3 composable)——9047a12,复核 0 严重 5 项全修
- **状态:** complete
- 裁决记录:方案子组件抽取(MediaGrid 3 件+ContentViewer 4 件)因宿主 scoped 后代选择器约束跳过留账,须单独授权 CSS 选择器块迁移才能做(先例裁量,两案复核均认可)

### 阶段 5:tierB Rust ×14
- [x] tierB-1——b65e6d3,复核 0 严重 2 警告=可见性 11 处已收窄 pub(super)
- [x] tierB-2/tierB-3——联合 commit 7430abe(video/mod.rs 双组声明叠加,故联合提交保 commit 可编译);组2 复核 0 严重 8 小项全修,组3 复核无发现
- [x] tierB-4——260ebd4(models 六域/justified geometry+grid_pack/face_pipeline 四阶段;items_cache 按方案判缓未动)
- [x] derivations.rs **跳过待裁**(D-454,维持不施工,待用户裁 WIP 归属)
- **状态:** complete(除 derivations.rs 按 D-454 明示跳过外全落)

### 阶段 6:全量清算 + 收口
- [x] P6 全量门禁(全量结果见下,dev 尖端 9047a12,基线工作树净):
  - `cargo fmt --check`(src-tauri 全仓)exit 0,零输出
  - `cargo clippy --all-targets --quiet` exit 0,1 条既有 warning(faces/wall.rs items_after_test_module,非本线新增)
  - `cargo test --quiet`(1072 用例)exit 1→单测 `error::tests::app_error_serialize_emits_full_envelope_with_error_chain_array` 失败(`--lib` 单独重跑通过,证实测试间日志捕获竞态导致的既有 flaky,非本线改动文件 error.rs 未触碰)
  - `npm run typecheck` exit 0,零输出
  - `npx vitest run` 1516 用例,既有红 1 例`alignment-grid.contract.spec.ts` 等式②(FoldersSection.vue 找不到 TREE_INDENT——sidebar 线既有问题,与口径一致),除它外全绿
  - `npm run build` exit 0,`✓ built in 3.72s`
  - `npx eslint src/` exit 0,零输出
  - B 段全角折损对拍(python FF01-FF5E 规整化+difflib,13 组 commit 覆盖全部清单文件):发现 2 处候选,1 处(scan/media_upsert.rs L87)已在 b9d666b 修过维持原状,1 处(exotic/worker_log.rs L187 `EOF:残段(若非空)...`)本轮还原为全角(`EOF：残段（若非空）...`),还原后 fmt/build 复跑绿
- **状态:** complete

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 施工另立项本线,承方案线 D-449;方案锚点按符号名(D-448,行号已漂勿信) | 方案线明令;注释线已再动文件 | D-452 |
| Rust 写类代理 isolation=worktree ≤3 并发、代理 worktree 内 cargo check 自验、主线单执行者合并;前端写类主树每波独占 1 代理(worktree 无 node_modules 跑不了 vue-tsc/vite) | 同树并发时半成品互相打红全 crate check;前端构建链依赖 node_modules;修订:弃 isolation 参数,主线手建 worktree+基点硬门(stale 基点事故) | D-453 |
| derivations.rs 施工跳过,挂待裁 | review-round2 红线「真 userWIP 勿碰=仅剩 derivations/doc_commands/RAW 线」;树净不代表 WIP 解除,须用户确认 | D-454 |
| 施工代理不 commit;主线按批显式 pathspec commit;每批至少一档复核,不裸提交 | git 写单执行者 + 复核随批走 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| Agent isolation:worktree 机制断续从 stale 基点 b1cba5d(2026-07-06,落后dev 959)建树,四个旧机制 worktree 全中招 | layout/faces 代理自行 ff-only 自救,scan 自察停工,worker_client 首派被主线停掉丢弃,重派两件被硬门拦停 | D-453 修订——主线手建 worktree(C:\workspace\scrollery-wt\<name>,建自 dev 尖端)+任务卡开工硬门 `git rev-list --count HEAD..dev`=0 |
| cherry-pick lib 撞 worker_service.rs(lib 尾部 bootstrap 追加 vs tierB-1 拆分) | git 三方冲突 635-1319 | 主线手解,ours(拆分态)+自 theirs 提取 bootstrap 段 14 行插 cfg(test) 声明前,cargo check 0 后 continue(e92e2b2) |
| FoldersSection 拆分引入 P1 级回归(computed 包装 shallowRef,原地 mutate+triggerRef 下版本不 bump,树索引静默死) | 施工代理四门验证全绿未暴露(无组件级测试) | 深度复核实测复现抓出,直传 ref 本体+useFolderTreeIndices.spec 2 例回归锁——「门禁绿≠正确,深度复核随批走」再获实证 |
