---
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# 任务计划:降噪超分子系统

## 目标
定案「降噪/超分」子系统的架构与实现方案:做成插件商店内可安装的付费/可选子系统,用户可选模型,复用现有 AI 分析/人脸流水线与 OCR 插件先例;本线交付设计定案(三件套+方案),不施工。

## 当前阶段
阶段 6:P0 施工(2026-07-24 用户批准开工;J-1..J-8 定案:除 J-2 改为用户自有 HuggingFace 仓托管外,其余全按 design.md J 节推荐)

### 阶段 6 施工 DAG
```
spike-A(env+RealESRGAN 导出对拍) ─┬→ spike-B(SCUNet/DRUNet 导出对拍) ─┬→ spike-D(DirectML 实测+bench) → 模型定案门
                                    └→ spike-C(FBCNN 导出对拍) ────────┘
批1(协议#1+profile#2) → 批2(推理核心#3 tiling/chain) → 批3(worker#4 crate) → 批4(host#9+catalog#5/6/7+超时#8+IPC#10/11) → 批5(前端#12/13) → 批6(资产钉定#14:HF 上传+registry URL/sha256 回填,依赖 spike 门+用户 HF 仓名)
```
- 裁决:spike 与模型无关骨架并行(协议/tiling/worker/host/前端均与具体模型解耦;spike 回炉仅动 profile 数据行与资产面)。
- spike 工作区:仓外 `C:\workspace\scrollery-model-zoo\`(venv/权重/导出脚本,绝不入本仓)。
- 排期门已解除:RAW 线 catalog.json/catalog.rs/coordinator.rs 已提交(6568e27),#5/#6/#8 可进场。
- ⏸ 用户待补:HuggingFace 仓名(批6 前需要,不阻塞批1–5)。
- 进度:spike-A ✅ / spike-B ✅ / spike-C ✅ / spike-D ✅(硬门通过,10/10 组合零崩,scunet-fp16 DML 唯一红降档裁决,短名单无回炉) / 批1 ✅(复核 1 警告已修) / 批2 ✅(深审 0 严重+增量核验通过,已提交) / 批3 ✅(深审+补测完:models_root 校验/白名单/tile 进度落地,cargo test -p enhance-worker 22 通 + exotic-protocol 34 通) / 批4 ✅(host 面全落地,reviewer 深审 0 严重) / 批4.5 ✅(九项修正:session 断裂重建/out_tmp 清扫/RAW 同步拦收口 catalog 单源/run_request 进度透传零行为委托,两轮增量核验通过) / 批5 ✅(前端 11 文件,复核两存疑) / 批5.5 ✅(队列面板+终态 toast 不滞后+两小修,存疑收口) / 批6 ⏸(待用户 HF 仓名)。
- 终态回归+两 commit 落地(本会话):`cargo test -p scrollery --lib -- ai::` 44 passed / `exotic::worker` 9 passed / `enhance` 18 passed;`cargo clippy -p scrollery --lib -- -D warnings` 0 警告;`npx vue-tsc --noEmit` 0 错;`npx vitest run enhanceStore.spec.ts localeIntegrity.spec.ts` 15 passed。commit 标题:`feat(enhance): host 全链——catalog/门控/EnhanceService/七命令 + 深审九项修正(P0 批4/4.5)`、`feat(enhance): 前端全链——EnhanceDialog/设置分节/队列面板/查看器入口(P0 批5/5.5)`。
- 遗留:批6(⏸ 用户 HF 仓名未定)+ ⏸ GUI 真机验收清单(见 progress.md 遗留节:商店卡片/模型下载进度/Dialog 全流程/队列面板+toast/预览 before-after/RAW 拒绝引导/未授权引导)。

## 依赖 DAG
```
阶段1(仓内摸底×2 并行) ─┐
                          ├─→ 阶段3(架构设计) → 阶段4(裁决定案) → 阶段5(收口本批)
阶段2(联网调研)       ─┘
```
阶段 1 与 2 无依赖,同批并发;3 依赖 1+2 全部回执;4 依赖 3;5 依赖 4。

## 阶段

### 阶段 1:仓内摸底(并行×2,haiku 检索代理直落 scratchpad) — complete
- [x] 1a+1b 均完成:AI 流水线 + exotic 插件商店/OCR 先例双映射落 scratchpad/ai-pipeline-map.md + scratchpad/plugin-store-map.md
- **状态:** complete

### 阶段 2:联网调研(researcher,与阶段 1 并行) — complete
- [x] 业界产品/开源模型许可/端侧部署实践三合一调研落 scratchpad/industry-research.md
- **状态:** complete

### 阶段 3:架构设计(architect,依赖 1+2) — complete
- [x] 三份 scratchpad 出方案,A–J 十节落 design.md(worker 拓扑/任务形态/插件商店建模/模型短名单/运行时与大图/UX/数据状态/分期/风险/用户裁决点)
- **状态:** complete

### 阶段 4:裁决与定案(主线) — complete
- [x] 主线裁决 A–I 全采纳(design.md 顶部裁决段);J-1..J-8 留用户裁决(design.md J 节)
- **状态:** complete

### 阶段 5:收口本批(phase-closer) — complete
- [x] findings 蒸馏(F-044..F-049)+ progress 回写 + 显式 pathspec 提交(本批 commit:docs(planning): 降噪/超分子系统三件套+方案定案)
- **状态:** complete

## 关键决策
<!-- 需收口提升的决策编 D-NNN(worklog-kit next-id 取全局序)填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 本线=设计定案,不施工 | 用户指令「分析并设计」;施工另立批次 | |
| A:独立 `enhance-worker` 拓扑(否 ai-worker 第三槽) | 串行阻塞/崩溃隔离/VRAM 争用靠槽不靠进程(design.md A 节) | D-436 |
| B:按需操作产 sibling `-enhanced` 新文件入库为普通 item,P0 零新表(否 derive kind、否 ai_items 表) | 产品语义=选择性付费操作非全库自动跑;产物归宿复用编辑线 naming/io/metadata 契约(design.md B 节) | D-437 |
| C:单 offering `exotic-enhance` builtin+paid,模型为插件内资产(否每模型一 offering) | 避免 SKU/验签 N 倍爆炸;模型迭代不锁 catalog 发版(design.md C 节) | D-438 |
| E:tile 512 静态 shape + 16px overlap 联动契约(改 tile 必重导模型) | 动态 shape 多 EP 明显更慢或不支持;tile 与静态导出形状强耦合(design.md E 节) | D-439 |
| H:spike(ONNX 自导出+golden 对拍+DirectML 实测)为 P0 硬门 | SCUNet/DRUNet/FBCNN 无现成 ONNX,自导出算子风险须先验证不过即回炉短名单(design.md H 节) | D-440 |

排期:P0 施工清单 #5/#6/#8(exotic-catalog.json / catalog.rs / coordinator.rs)须待 RAW 线同三文件落地后进场(design.md 施工排期备注)。

**待用户裁决**:见 design.md J 节(J-1..J-8),不复述。

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
