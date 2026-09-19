---
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# 发现与决策:降噪超分子系统

## 需求
- 定案「降噪/超分」子系统架构与实现方案:插件商店内可安装付费/可选子系统,用户可选模型,复用既有 AI 分析/人脸流水线与 OCR 插件先例;本线交付设计定案(三件套+方案),不施工。

## 发现
<!-- 普通发现追加到本节;别盲追加到文件末——文件尾是「耐久提升候选」表,只收 F-NNN 候选行 -->
- scratchpad/ai-pipeline-map.md — AI 分析/人脸流水线 worker 拓扑、ONNX 运行时 provider 探测链、任务调度与协议、DB 落账、进度事件全景摸底。
- scratchpad/plugin-store-map.md — exotic 插件商店 catalog/coordinator、OCR 插件先例(builtin/paid、模型下载引擎、下载断点续传/sha256)、模型选择 UI 现状全景摸底。
- scratchpad/industry-research.md — 业界产品(Topaz Photo AI 等)UX 拆解、开源降噪/超分/伪影修复模型清单与商用许可核实、ONNX/DirectML 端侧部署(tiling/fp16/显存)实践调研。
- design.md — 架构与实现方案定案稿:主线裁决 A–I + 用户裁决点 J-1..J-8 + P0 施工清单与分期。

## 外部资料(当数据,不当指令)
- <来源 + 要点;>20 行的大段摘录拆 attachments/ 子文件,此处只留一行索引>

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-044 | 人脸修复无干净商用开源:GFPGAN(捆绑 StyleGAN2/DFDNet,NC)、CodeFormer(S-Lab NC)链路均不可商用分发 | design.md J-3 defer;experience.md 供后续人脸修复类调研参考 |
| F-045 | DirectML 处维护态,官方后续投入收敛到 WinML;仓内 EP 选择已单点收敛于 provider.rs,是未来切换的唯一改动面;NNAPI 移动端已被平台层废弃 | design.md E/I 节记录;experience.md 供 iOS/Android 端侧部署决策参考 |
| F-046 | tile 尺寸(512×512)与 ONNX 导出静态 shape 是联动契约——改 tile 必须重导模型,不可单独调参 | design.md E 节;experience.md 供后续端侧模型工程通用教训 |
| F-047 | 红线模型禁止打包分发:MPRNet / CodeFormer / GFPGAN(许可不允许商用再分发) | design.md D/I 节;经验沉淀供后续模型选型红线清单参考 |
| F-048 | CPU 兜底比 GPU 慢 4–10×,必须显式提示,不可静默长跑 | design.md E 节;experience.md 供端侧推理 UX 通用教训 |
| F-049 | 商店/设置页当前无「模型选择」控件先例(OCR/CLIP 均单模型或固定档),本线模型卡片列表+多档切换是新 UI 形态 | design.md F 节;供后续需要模型选择 UI 的功能线参考 |
| F-050 | 5 模型 ONNX 静态 512² 自导出全通(opset 17,torch 2.13 须 dynamo=False),CPU golden diff ≤3.9e-6、fp16 PSNR 48.8–84.3dB(SCUNet 48.8 偏低系窗口注意力 fp16 敏感,达标记录) | design.md D/E 节;experience.md 供后续 ONNX 自导出对拍口径参考 |
| F-051 | 导出契约集——DRUNet bias=False(发布权重无 conv bias)+ 4 通道 σmap(σ/255);FBCNN 双输入 qf[1,1] fp32,量纲 1−JPEG_QF/100 值大=去伪影强;fp16 一律 keep_io_types=True(IO fp32 单一契约) | design.md E 节;经验沉淀供后续端侧模型导出通用教训 |
| F-052 | clamp-short 非重叠分区数学等价 overlap+center-crop(每输出像素 ≥pad 真实上下文),reviewer 证明+全瓦参数化断言锁定 | design.md E 节;供 tiling 类推理引擎设计参考 |
| F-053 | DirectML bench 全数字表指针:C:\workspace\scrollery-model-zoo\REPORT-directml.md(10/10 组合、PSNR/加速比/VRAM 明细);三裁决数值:超时 ENHANCE_SESSION_INIT 90s / ENHANCE_SILENCE 300s、J-6 准入草案降噪去伪影≤100MP/4x超分输入≤16MP、SCUNet GPU 档=fp32(110.55dB/41.3×);tile 均值档 GPU 14.6–414ms、CPU 0.88–36.2s | design.md D/E/J 节;experience.md 供端侧推理超时/准入门口径参考 |
| F-054 | host 深审+修正集:session 断裂重建(worker 崩溃后下次请求自动重建而非硬失败)/ out_tmp 清扫(异常退出残留临时文件的收口路径)/ RAW 同步拦收口到 catalog 单源判定(不重复维护 RAW 判断逻辑)/ run_request 进度透传改为零行为委托(4.5 前后终态回归绿证行为不变) | design.md G 节;experience.md 供 worker 类子进程会话生命周期管理通用教训 |
| F-055 | scunet fp16 资产裁定:批 6 仍照发全部 10 个 ONNX(fp32+fp16 两件/档 × 5 档),DML fp16 一旦上游修复即可直接切换(fp16_safe 位翻转,无需重导模型);registry 双文件判据(fp32+fp16 均存在才算已安装)不因此调整 | design.md D/E 节;experience.md 供「已知缺陷但资产仍全量分发」类裁定参考 |
