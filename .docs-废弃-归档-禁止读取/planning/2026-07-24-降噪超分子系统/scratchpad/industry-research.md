---
id: 2026-07-24-industry-research
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# 照片降噪/超分业界方案调研

> 调研目的:为 Scrollery(Tauri+Rust 桌面资产管理器,Windows 优先)设计商用降噪/超分子系统提供输入。所有关键断言附来源;未核实/推断已标注。

## 1. 商业产品拆解

### Topaz Photo AI
- 定价:历史上一次性 $199(Photo AI)/独立 Gigapixel $99;近期已转向订阅制,Topaz Studio 订阅约 $199/年,另有 Personal 套餐 $12/月或 $149/年、Gigapixel 单品 $50/月或 $204/年——**来源冲突**:多篇聚合评测(vhiz.ai、myarchitectai)口径不一致,一次性授权是否仍可购买未查实。来源:https://www.topazlabs.com/pricing ;聚合信息 https://www.vhiz.ai/tools/topaz-photo-ai/ https://www.myarchitectai.com/blog/topaz-ai-pricing
- 能力矩阵:Denoise、Sharpen(去模糊)、Upscale(Gigapixel 引擎,最高 16x,9 个专用模型)、Face Recovery 四大模块整合进单一工作区,内置 100+ 按图像类型/光照/噪声画像训练的专用模型。来源:https://www.topazlabs.com/topaz-photo https://www.topazlabs.com/see-it-in-action/face-recovery
- Autopilot:对每张图片自动分析并选择/应用合适的增强组合(型号+强度),用户可在结果不满意时手动切换模型。Upscale 板块提供 4 个可选模型:Standard、High Fidelity、Graphics、Low Resolution,Autopilot 自动指派其一,用户可手动改选并调强度滑杆。来源:https://parkerphotographic.com/topaz-photo-ai-manual/ (聚合口径,建议对照官方文档核实模型名称是否为最新版本——**未核实**当前版本模型命名)
- 对比交互:官方文档未在检索到的内容中明确描述"前后对比滑杆"是单独浮层还是分屏;Gigapixel AI 中有"最多选 4 个模型并排分屏对比"的 Compare 功能。来源:https://docs.topazlabs.com/gigapixel-ai/functions/compare (Photo AI 是否共享该 UI 组件——**未核实**)
- 处理时长:GPU 下单图 5–15s,中端配置(i7/32GB/RTX 3060)10–30s/图,百图批处理 15–30 分钟;纯 CPU(无独显)可能 1–3 分钟/图,老旧集显甚至 30–60s+。来源:聚合评测口径 https://www.pugetsystems.com/labs/articles/topaz-ai-cpu-gpu-performance-analysis/ (Puget 权威实测,建议以此为准优先于聚合博客数字)
- RAW/JPEG 边界:官方资料未直接给出逐格式边界声明;从产品定位看 Photo AI 同时接受 RAW 与已编码图(JPEG/TIFF/PNG 等),与 DxO PureRAW「仅 RAW」形成对照——**推断**,建议后续用官方支持格式列表核实。

### DxO PureRAW(DeepPRIME / DeepPRIME XD3)
- 定位:RAW 转换工具,**明确不能处理 JPEG 或其他非 RAW 格式**——两来源交叉确认(PhotoWorkout 评测 + 官方 userguide 隐含)。来源:https://www.photoworkout.com/dxo-pureraw-6-cp-plus-2026/ https://www.dxo.com/en/dxo-pureraw/
- DeepPRIME XD3(第四代)是当前旗舰降噪引擎,PureRAW 6 起支持 Bayer 传感器(此前仅限部分传感器类型),这使绝大多数 Canon/Nikon/Sony Bayer 相机机型都能用上最高档降噪。来源:https://www.dxo.com/news/deepprime-xd3-fourth-generation/ https://www.photoworkout.com/dxo-pureraw-6-cp-plus-2026/
- 定价:PureRAW 6 新授权 $139.99,老版本升级价 $89.99,买断制无订阅。来源:https://www.photoworkout.com/dxo-pureraw-6-deal/
- 与 Topaz/Lightroom 差异:DxO 路线是"RAW 解马赛克 + 降噪一体化"专精单一环节,不做超分辨率放大;Topaz 是多任务大一统工作区;Adobe 是编辑套件内嵌功能。

### Adobe Lightroom / Camera Raw:Denoise AI 与 Super Resolution
- **两个功能互斥**且格式边界不同——多来源交叉确认:
  - Denoise:仅支持 Bayer/X-Trans 马赛克 RAW、Linear DNG(含 Lightroom/ACR 内生成的 HDR/Pano DNG)、Apple ProRAW;**不支持 JPEG**。应用 Denoise 会自动叠加 Raw Details。来源:https://helpx.adobe.com/lightroom-classic/help/enhance-details.html
  - Super Resolution:支持 Raw Details 相同格式外,**额外支持 JPEG、TIFF**,可放大到 4 倍。来源:同上
  - 两者不能同时应用于同一张图(选中 Denoise 后 Super Resolution 变灰不可用)。来源:https://community.adobe.com/t5/lightroom-classic-discussions/new-denoise-ai-incompatibility-with-photo-format-why/td-p/13746771
- 这一"RAW 专属降噪 + 非 RAW 也能超分"的边界设计,与 DxO(纯 RAW)、Topaz(RAW+JPEG 通吃)形成三种不同产品哲学,值得 Scrollery 参考:降噪对已编码图更容易产生伪影放大,超分对已编码图相对安全。

### ON1 NoNoise AI
- 定价:$69.99 买断(终身授权);也提供订阅与作为 Photoshop/Lightroom 插件的免费试用版。来源:https://capturetheatlas.com/on1-nonoise-ai-review/ https://sourceforge.net/software/product/ON1-NoNoise-AI/
- 能力:AI 降噪保留羽毛/毛发/细纹理,导出为 DNG 保留 RAW 色彩/影调数据;独有 Micro Sharpening;新版本包含运动模糊消除(deblur)模型。**明确支持对已编辑的 TIFF/JPEG 做后期降噪一遍**(与 Lightroom Denoise 的纯 RAW 限制相反)。来源:https://www.on1.com/products/photo-raw/nonoise/ https://www.on1.com/blog/the-ultimate-ai-noise-reduction-solution-for-photos/

### 四家 RAW/JPEG 边界对照小结
| 产品 | RAW 降噪 | JPEG/已编码图降噪 | 超分 |
|---|---|---|---|
| Topaz Photo AI | 支持 | 支持(**推断**,未逐条核实) | 支持(Gigapixel 引擎) |
| DxO PureRAW | 支持(DeepPRIME XD3) | **不支持** | 不做超分 |
| Lightroom Denoise | 支持(仅 Bayer/X-Trans/Linear DNG/ProRAW) | **不支持** | 走 Super Resolution(互斥功能) |
| Lightroom Super Resolution | 支持 | 支持(JPEG/TIFF) | 是(4x) |
| ON1 NoNoise AI | 支持 | **支持**(含已编辑 TIFF/JPEG 二次降噪) | 未见明确超分主打功能——**未核实** |

## 2. 开源模型清单(超分/降噪/JPEG伪影/人脸修复)

> 权重许可证栏均以仓库自身声明为准;若权重训练数据集本身带 NC 限制而仓库未声明,不在本次核实范围内(标注为"推断/未核实"处)。

### 超分辨率
| 模型 | 任务 | 参数量/权重大小(概数) | 代码许可证 | 权重许可证 | ONNX 可得性 | 口碑/备注 |
|---|---|---|---|---|---|---|
| Real-ESRGAN (x4plus) | 通用超分 x4 | ~16.7M / .pth ~67MB | BSD-3-Clause | 同 BSD-3-Clause(仓库未单列) | 有(Qualcomm/AMD/社区导出多个 ONNX 版本) | 业界事实标准,ncnn-vulkan/Upscayl 默认模型 |
| Real-ESRGAN (x4plus-anime-6B) | 动漫图超分 x4 | 6-block RRDBNet / .pth ~18MB | BSD-3-Clause | 同上 | 有(civitai/HF 均有转换版) | 动漫向轻量版,速度更快 |
| BSRGAN | 真实退化盲超分 | 未见明确公开参数量——**未核实** | Apache-2.0(cszn/BSRGAN 仓库) | 未见单独声明,依 Apache-2.0——**推断**;衍生模型(如 Siax 等社区插值版)常标 CC BY-NC-SA 4.0 | 有(OpenModelDB 有 ONNX 版) | 学术口碑好,真实退化建模是亮点;注意与其衍生社区权重许可不同 |
| SwinIR | 图像复原/超分(Transformer) | 多档(S/M/L) | Apache-2.0 | 未见单独声明——**推断**同 Apache-2.0 | 有(OpenModelDB 多档) | 精度高、推理相对慢,tile 场景需注意窗口对齐(w8) |
| HAT (Hybrid Attention Transformer) | 超分 SOTA | 较大(具体未查实——**未核实**) | Apache-2.0 | 未见单独声明——**推断**同 Apache-2.0 | 少(社区少见现成 ONNX,推断需自行导出) | 精度业界顶尖但推理慢/显存高,桌面端实时性存疑 |
| SPAN (Swift Parameter-free Attention Network) | 高效超分 | 轻量,NTIRE2024 高效赛道冠军 | Apache-2.0 | 需查具体仓库 LICENSE 文件——**未核实**细则,以 Apache-2.0 为准 | 有(社区/OpenModelDB) | 速度快、口碑新兴,适合端侧实时场景 |
| RealPLKSR | 真实退化超分(如 NomosWebPhoto 系列) | 轻中量级 | 取决具体权重,OpenModelDB 上 NomosWebPhoto-RealPLKSR 标注 **CC-BY** | CC-BY(需署名,允许商用) | 有(社区常发布 .pth,ONNX 需自转) | 摄影类真实图像修复口碑不错;CC-BY 商用友好但需署名合规流程 |
| waifu2x(原版,nagadomi) | 动漫超分/降噪(早期开创者) | 小(经典 VGG 风格) | MIT | 未见单独声明——**推断**同 MIT | 有多个第三方转换版 | 历史地位高,画质已被 Real-ESRGAN/新模型超越,不建议作为主力 |
| OpenModelDB 社区权重(泛指) | 各任务 | 参差 | 参差 | **逐模型核实**——平台明确允许贡献者自选任意许可证,从 CC0 到 CC BY-NC-SA 均有,不能按平台统一假设 | 视模型而定 | chaiNNer/Spandrel 生态的权重集散地,选用前必须逐个查证 LICENSE 字段 |

### 降噪
| 模型 | 任务 | 参数量/权重大小 | 代码许可证 | 权重许可证 | ONNX 可得性 | 口碑/备注 |
|---|---|---|---|---|---|---|
| SCUNet (cszn/KAIR) | 真实图像降噪 | 中等 | MIT(KAIR 仓库,含 megvii-model MIT + BasicSR Apache-2.0 双授权说明) | 同 MIT——**推断**未见单列 | 需自行导出(未见现成 ONNX 广泛流通)——**未核实** | 学术口碑稳定,商用友好 |
| NAFNet | 降噪/去模糊/低光增强 | 中等 | LICENSE 文件含双段:megvii-model 部分 MIT + BasicSR 部分 Apache-2.0 | 同上,均为宽松许可 | 少见现成 ONNX,需自转——**未核实** | 结构简洁("无非线性激活")、速度口碑好 |
| Restormer | 高分辨率图像复原(降噪/去雨/去模糊) | 中等 | **MIT**(已直接核实 raw LICENSE.md 文件内容,无 NC 条款) | 同 MIT | 少见现成 ONNX——**未核实** | 用户任务书提示"注意许可",经核实实为 MIT 宽松许可,可放心商用;此前网传"研究限定"说法未在当前仓库 LICENSE 中找到依据 |
| DRUNet (cszn/KAIR 同仓) | 通用降噪(可变噪声水平) | 中等 | 同 KAIR 仓库 MIT | 同上 | 未核实 | 经典可控降噪模型,与 SCUNet 同源仓库 |
| MPRNet | 多阶段渐进复原(降噪/去雨/去模糊) | 中等 | **非商用**——LICENSE.md 明确写明"free for use in noncommercial settings...contact us for commercial" | 同代码许可,**非商用** | 未核实 | **红线**:未获授权不得用于商用产品 |

### JPEG 伪影去除
| 模型 | 任务 | 许可证(代码) | 权重许可证 | ONNX | 备注 |
|---|---|---|---|---|---|
| FBCNN | 可控 JPEG 伪影去除(盲/非盲) | Apache-2.0 | 同 Apache-2.0——**推断**未见单列 | 未核实现成 ONNX 情况 | 商用友好,是「JPEG 重压缩后再降噪超分」链路中的关键前置/独立模块 |

### 人脸修复
| 模型 | 任务 | 代码许可证 | 权重/第三方组件许可证 | ONNX | 备注 |
|---|---|---|---|---|---|
| GFPGAN | 人脸修复+细节增强 | 主体 Apache-2.0 | **关键风险**:依赖的 StyleGAN2(NVIDIA 专有许可,"仅限非商用使用或意图非商用")与 DFDNet(CC BY-NC-SA 4.0)两个第三方组件均带 NC 限制,官方 LICENSE 文件自己列出这些第三方条款 | 有社区转换版 | **红线**:GFPGAN 官方代码本身 Apache-2.0,但因捆绑 NC 组件,商用整体链路存在法律风险,需替换掉受限组件或获得单独授权后才能商用 |
| CodeFormer | 人脸修复(鲁棒退化) | **S-Lab License 1.0**,已直接核实 LICENSE 原文:默认仅限非商用,"如需商用需联系贡献者" | 同 NC 许可 | 有社区转换版 | **红线**:非商用许可,不可直接用于商用产品 |
| RestoreFormer | 人脸修复(Transformer 架构,ROI 字典先验) | Apache-2.0(已核实仓库声明) | 未见类似 GFPGAN 那样捆绑 StyleGAN2/DFDNet 的第三方 NC 组件说明——**未核实**是否内部仍复用了此类组件,需实测其 requirements/依赖树 | 未核实现成 ONNX 情况 | 表面许可证比 GFPGAN/CodeFormer 干净,但因架构与训练数据链路较新,建议施工前专门审查其 pip 依赖是否引入 NC 组件 |

### 商用许可安全短名单(权重可商用、来源明确)
- 超分:Real-ESRGAN(x4plus / anime-6B,BSD-3-Clause)、SwinIR / HAT / SPAN(均 Apache-2.0,代码侧确认;权重侧未见单列但通常从其)、RealPLKSR 系(个别权重如 NomosWebPhoto 标 CC-BY,需署名)
- 降噪:SCUNet / DRUNet(KAIR 仓库 MIT)、NAFNet(MIT+Apache-2.0 双授权)、Restormer(MIT,已核实非 NC)
- JPEG 伪影:FBCNN(Apache-2.0)
- 人脸修复:**暂无可直接商用的成熟开源方案**——GFPGAN 因捆绑组件受限、CodeFormer 明确非商用;RestoreFormer 表面 Apache-2.0 但依赖链未经逐项核实前不建议判定为"安全"
- **红线合集**:MPRNet(非商用)、CodeFormer(非商用/S-Lab License 1.0)、GFPGAN 整体链路(因 StyleGAN2/DFDNet 组件非商用)三者禁止直接商用打包分发

## 3. 端侧部署实践

### onnxruntime execution provider 成熟度
- **Windows DirectML**:官方文档明确 DirectML EP 现处于"sustained engineering"(维护态,不再做新特性开发),微软已将新特性投入方向转向 Windows ML(WinML,面向 Windows 11 24H2+),DirectML EP 当前锁定 DirectML 1.15.2、最高支持 ONNX opset 20。对 Scrollery(Windows 优先)意味着:短期可用 DirectML EP 稳定跑,但架构上应预留切到 WinML 的路径。来源:https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/supported-execution-providers
- **macOS/iOS CoreML**:CoreML EP 要求 iOS ≥13 或 macOS ≥10.15,ONNX Runtime Mobile 预编译包已内置 CoreML EP,成熟度较高,是苹果平台首选。来源:https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html
- **Android NNAPI/XNNPACK/QNN**:存在**平台层面与运行时文档层面的口径分歧**——
  - ONNX Runtime 官方文档仍将 NNAPI 列为 Android 预编译包内置 EP,未在检索内容中提及废弃。来源:https://onnxruntime.ai/docs/execution-providers/NNAPI-ExecutionProvider.html
  - 但 Android 平台侧,Google 官方 NDK 文档明确 **NNAPI 在 Android 15 中已被标记为 deprecated**,建议迁移至 TensorFlow Lite in Play Services 或 AICore;原因是 transformer/diffusion 类新模型迭代太快,NNAPI 更新节奏跟不上。来源:https://developer.android.com/ndk/guides/neuralnetworks/migration-guide
  - **两来源不裁决,并列呈现**:若 Scrollery 要长期维护 Android 端侧推理,直接依赖 onnxruntime 的 NNAPI EP 存在"底层被平台方废弃、上游 EP 仍可用但可能逐步失去硬件厂商适配动力"的风险,应把 XNNPACK(CPU,跨平台稳定)作为兜底,QNN(高通芯片专用)作为高端机型加速路径,谨慎将 NNAPI 当长期依赖。
  - ONNX Runtime 官方给出的选型建议:量化模型优先 CPU EP;非量化模型优先 XNNPACK;若性能不够再尝试 NNAPI/CoreML。来源:https://onnxruntime.ai/docs/execution-providers/Xnnpack-ExecutionProvider.html

### 大图 tiling(重叠 tile + 融合防接缝)
- Real-ESRGAN 官方推理脚本(`inference_realesrgan.py`)提供 `tile`(默认 0=不分块)与 `tile_pad`(默认 10px,用于消除边界伪影)两个参数;显存有限时建议 tile=200–400,8GB+ 显存可以 tile=400 或不分块。来源:https://github.com/xinntao/Real-ESRGAN/blob/master/inference_realesrgan.py (推荐值来自聚合文档 https://xinntao-real-esrgan.mintlify.app/quickstart)
- Real-ESRGAN-ncnn-vulkan(`-t tile-size`)：tile size 需 ≥32 或 0(自动),多数场景用自动即可,显存充裕时可手动调大,理论上略微提升速度与质量但不明显。来源:项目 README(TransparentLC/realesrgan-gui 转述)https://github.com/TransparentLC/realesrgan-gui/blob/master/README.en-US.md
- 通用经验(学术侧,非本项目专属):0% overlap 会在拼接边界出现明显 seam;overlap 越大 seam 越少但重绘算力开销越大;常见做法是 stride=256、tile=512 一类配比,或用"相邻 tile 内容作为 padding"的方案使边界过渡自然。来源:综述性资料 https://www.researchgate.net/publication/397968632_Tile-Based_Image_Upscaling_Using_AI ,Stable Diffusion 生态的 Tiled Diffusion 文档亦佐证同一权衡 https://github.com/pkuliyi2015/multidiffusion-upscaler-for-automatic1111/wiki/Tiled-Diffusion(**注意**:后者是扩散模型场景,非 SR CNN 场景,原理相通但数值不可直接照搬)

### fp16/量化对画质的影响
- fp16(半精度)转换会将模型从 fp32 降到 16 位,显著减小内存占用、提升 GPU 推理速度,但**可能带来"静默"精度损失**,需要针对具体模型评估是否可接受。来源:https://onnxruntime.ai/docs/performance/model-optimizations/float16.html 、行业分析 https://tisankan.dev/ai-model-accuracy-degradation/
- 进一步量化到 int8 相对 fp16 的质量损失普遍"几乎免费"(sub-1%),int8→int4 损失约 1–3%(可用但可见),int4→int3 损失 5–15%(通常不可用,除非做 QAT)。此数字来自 LLM/通用深度学习量化语境的综述,**并非专门针对图像 SR/降噪模型的实测**,直接套用到像素级复原任务前应做画质对拍——**推断性外推,标注风险**。来源:https://zeroentropy.dev/concepts/model-quantization/
- 结论:fp16 对 SR/降噪模型通常是安全默认档(桌面 GPU 场景);int8 量化需项目自行做视觉盲测对拍后才能作为"高速档"提供给用户,不建议无对拍直接上线为默认档。

### 动态 shape 坑
- 官方与社区一致确认:**固定 shape 推理明显快于动态 shape**,尤其在 CPU/GPU 上;对图像识别类模型,动态尺寸单次预测可能超过 1 秒。来源:https://github.com/microsoft/onnxruntime/issues/6978 https://github.com/microsoft/onnxruntime/issues/13198
- 若目标 EP 是 NNAPI 或 CoreML,**动态 shape 可能完全不被支持或严重降速**,官方建议用 `onnxruntime.tools.make_dynamic_shape_fixed` 工具把输入尺寸固定为部署已知的常见 tile 尺寸(如 1,3,512,512)。来源:https://onnxruntime.ai/docs/tutorials/mobile/helpers/make-dynamic-shape-fixed.html
- 对 Scrollery 的含义:tiling 策略天然产出固定 tile 尺寸(如 512x512),恰好可以与"固定 shape 更快"的约束对齐——tile 尺寸选型应与「导出 ONNX 时锁定的静态输入尺寸」联动设计,而不是先固定模型再任意选 tile 尺寸。

### ncnn-vulkan 与 onnxruntime 的取舍
- **ncnn-vulkan 路线**(如 Real-ESRGAN-ncnn-vulkan、Upscayl):Upscayl 是基于 Real-ESRGAN-ncnn-vulkan 二进制的 Electron GUI,通过 Vulkan API 在本地 GPU 上跑张量计算,不需要联网,内置 6 个默认模型(General Photo/UltraSharp/Remacri/Ultramix Balanced/HFA2k 等),支持 2x/3x/4x,"Double Upscayl" 两次链式放大做到最高 16x。前提是**用户机器需要有支持 Vulkan 的 GPU**。来源:https://github.com/upscayl/upscayl https://www.upscayl.io/
- **onnxruntime 路线**:跨 EP(DirectML/CoreML/NNAPI/XNNPACK/QNN)统一 API,更适合需要在多平台(Windows/macOS/iOS/Android)间共享同一套推理代码的场景,且能挂接 Windows ML 未来演进路径;缺点是各 EP 成熟度/性能不均衡(见上文 DirectML 维护态、NNAPI 平台层废弃等问题),需要按平台适配。
- **取舍建议(推断,供 architect 参考)**:若 Scrollery 目标是"Windows 优先、后续多端一致体验",onnxruntime + 多 EP 是更可持续的统一抽象层选择,可承接现有 ai-worker/exotic-workers 架构里已有的 onnxruntime 使用惯例(仓库里 OCR 插件线已采用 onnxruntime 复用 ai-worker,参见项目内部记忆);ncnn-vulkan 路线性能通常更极致(专为该场景优化的二进制),但生态是独立 CLI/二进制形态,与现有 Rust IPC 架构整合成本更高,且需要额外维护 Vulkan 依赖矩阵。

### chaiNNer / Spandrel 生态与 OpenModelDB 作为模型源
- Spandrel 是从 chaiNNer 节点式图像处理 GUI 中拆出的独立库,专门负责"加载各种 PyTorch 超分/复原/图像修复架构的通用推理封装",支持 TorchScript(.pt)、部分 .ckpt、.safetensors 格式。来源:https://github.com/chaiNNer-org/spandrel
- chaiNNer 自 v0.21.0 起用 Spandrel 取代自己内置的架构支持代码,两者与 OpenModelDB 强绑定——OpenModelDB 是面向 Spandrel 生态的社区权重集散地(含调用脚本 `invoke-spandrel.py`)。来源:https://github.com/OpenModelDB/open-model-database/blob/main/invoke-spandrel.py
- 可行性:作为**模型源发现渠道**可行(能快速找到大量预训练 SR/复原权重及其效果样例),但**逐模型许可证核查是刚性前置工作**——OpenModelDB 允许贡献者自选任意许可证(从 CC0 到 CC BY-NC-SA 都有),不能把"来自 OpenModelDB"当作商用安全的代理指标。来源:https://openmodeldb.info/docs/licenses
- Spandrel 本身是 PyTorch 生态封装,**不直接产出 ONNX**;若走 onnxruntime 路线,仍需要额外一步「.pth/.safetensors → ONNX 导出」流程,这一步骤本身有算子兼容性风险(尤其 Transformer 类模型的 attention/window 操作导出到 ONNX 有时会遇到不支持的算子),需要项目自行验证——**未核实**,列为技术风险点。

## 4. 产品形态参照

### 模型选择 UX:自动 vs 手动
- Topaz Photo AI 用 Autopilot 做"自动分析选型 + 一键应用",同时保留手动切换模型(如 Upscale 板块的 Standard/High Fidelity/Graphics/Low Resolution 四选一)与强度滑杆精调,是"自动为主、手动兜底"的典型范式。来源:https://parkerphotographic.com/topaz-photo-ai-manual/
- DxO PureRAW/Lightroom Denoise 走的是"单一最优引擎,不给用户选模型"的极简范式(DxO 甚至不开放模型选择,只有强度/细节参数);Upscayl 则相反,把 6 个具名模型(General Photo/UltraSharp/Remacri 等)平铺给用户手动选,无"自动推荐"层。**三种范式并列**,对应不同目标用户(专业修图师想要控制权 vs 普通用户想要一键出图)。

### 批处理
- Topaz Photo AI 支持批量导入自动应用 Autopilot 结果,官方评测给出"100 张图 15–30 分钟"的量级参考(GPU 场景)。来源:https://www.pugetsystems.com/labs/articles/topaz-ai-cpu-gpu-performance-analysis/(Puget 系列是业界公认的硬件性能评测机构,建议优先信任其数字胜过聚合博客)
- ON1/DxO PureRAW 均支持批处理导出,DxO PureRAW 6 明确将"更快批处理"作为本版本卖点之一。来源:https://www.photoworkout.com/dxo-pureraw-6-cp-plus-2026/

### 前后对比预览
- Topaz Gigapixel AI 有官方 Compare 功能,可同屏分屏对比最多 4 个不同模型的输出效果。来源:https://docs.topazlabs.com/gigapixel-ai/functions/compare
- Photo AI 主产品是否共享同一"多模型分屏对比"组件,检索未能确认——**未核实**,建议若要复刻交互,以实测/官方最新文档为准而非本次聚合信息。

### 处理时间量级(消费级 GPU 与 CPU 兜底)
- 消费级 GPU(如 RTX 3060 级别):单图 10–30 秒量级(含降噪+超分+人脸修复组合任务);更强 GPU(RTX 40 系)有专门 Puget 评测数据可查但本次未展开逐档读取——来源:https://www.pugetsystems.com/labs/articles/topaz-ai-suite-nvidia-geforce-rtx-40-series-performance/ (标题命中,内容细节**未核实**,仅确认该评测存在)
- 纯 CPU 兜底:1–3 分钟/图量级,是 GPU 场景的 4–10 倍耗时,说明"CPU 兜底"必须被视为体验明显下降的降级路径,产品上应该给出耗时预估提示而非静默阻塞。来源:聚合评测(https://www.pugetsystems.com/labs/articles/topaz-ai-cpu-gpu-performance-analysis/ 中提及 Intel Mac 无独显场景)
- Real-ESRGAN 原始推理速度在不同 GPU 架构下从约 0.08 秒/图(小图/低分辨率测试集)到 1–2 秒/图(更大分辨率、不同架构)不等,数字高度依赖分辨率与 GPU 型号,**没有查到统一的"每兆像素"标准化数字**,建议 Scrollery 自行在目标硬件矩阵上跑基准而非直接借用文献数字。来源:https://nhsjs.com/2025/enhancing-super-resolution-models-a-comparative-analysis-of-real-esrgan-aesrgan-and-esrgan/ (未核实该论文测试环境细节)

## 未覆盖
- Topaz Photo AI 当前(2026-07)最新版本的确切模型清单与命名(是否仍是 Standard/High Fidelity/Graphics/Low Resolution 四选一)——建议后续直接读官方 docs.topazlabs.com 最新版而非聚合博客
- RestoreFormer 依赖链是否间接引入 StyleGAN2/DFDNet 等 NC 组件——需要拉取其 requirements.txt/环境配置逐项核实
- SCUNet/DRUNet/NAFNet/FBCNN/SPAN 现成可用 ONNX 权重的具体下载地址与文件体积——本次仅核实了许可证,导出/获取环节留给 architect 阶段具体验证
- HAT 的具体参数量、显存占用、在 onnxruntime 上的实测可行性——检索未能给出可靠数字
- Windows ML(WinML)相对 DirectML EP 的迁移成本与 API 差异细节——仅确认微软官方建议方向,未深入迁移指南
- macOS/iOS 端 CoreML EP 在 fp16/tiling 组合下的实测表现——未查
- QNN EP 在骁龙芯片上相对 XNNPACK 的实测加速比——未查
- Adobe Denoise/Super Resolution 底层是否用扩散模型还是 CNN(有传闻是基于扩散,未在本次检索中核实)——标"未核实"

## 设计建议速览(供 architect 直接消费)

- **候选模型短名单(商用安全优先)**:超分主力 Real-ESRGAN x4plus/anime-6B(BSD-3-Clause);备选高质量 SwinIR/HAT/SPAN(Apache-2.0,权重需逐个核实 LICENSE 文件);降噪主力 SCUNet/DRUNet(KAIR, MIT)+ Restormer(MIT,已核实非 NC,推翻"注意许可"的预警);JPEG 伪影用 FBCNN(Apache-2.0)。
- **人脸修复暂无安全开源选项**:GFPGAN(捆绑 StyleGAN2/DFDNet 均 NC)、CodeFormer(S-Lab License 1.0 非商用)均不可直接商用;RestoreFormer 表面 Apache-2.0 但依赖链未核实,列为"待专项调查"而非"可用"。
- **红线三项禁止商用打包**:MPRNet、CodeFormer、GFPGAN(整体链路)。
- **运行时建议**:统一走 onnxruntime + 多 EP(Windows DirectML 短期可用但已是维护态,需预留切 WinML;macOS/iOS CoreML 成熟;Android NNAPI 已被 Google 平台层废弃,长期应以 XNNPACK 兜底+QNN 高端加速,不押注 NNAPI)。
- **导出与 tiling 联动**:onnx 导出时锁定静态 shape(动态 shape 在多个 EP 上明显更慢或不支持),tile 尺寸设计应与该静态 shape 对齐,tile_pad ≈10px 起步,VRAM 充裕可放大 tile 减少融合缝,VRAM 紧张降到 200–400px。
- **精度档位**:fp16 作为默认高速档风险可控;int8 量化需项目自行做视觉盲测对拍后才能上线为"更快但略降质"选项,不建议无对拍直接给用户。
- **CPU 兜底体验**:预期比 GPU 慢 4–10 倍,产品上必须给耗时预估/进度提示,不能静默长时间阻塞。
- **UX 范式**:采用"自动 Autopilot 式一键 + 手动模型/强度精调兜底"的 Topaz 式范式,优于 DxO 式零选择或 Upscayl 式纯手动平铺。
- **RAW vs 已编码图边界**:降噪对已编码图(JPEG)伪影风险更高,可参考 Lightroom 做法(降噪限定 RAW 家族,超分放开到 JPEG/TIFF),而非照搬 ON1/Topaz 的"全格式通吃"。
- **ncnn-vulkan vs onnxruntime**:若要与现有 Rust/Tauri IPC 及 ai-worker 架构统一,onnxruntime 路线整合成本更低;ncnn-vulkan(如 Upscayl 同款)性能更极致但生态独立、需要额外维护 Vulkan 依赖矩阵,仅在onnxruntime 某平台 EP 表现不达标时作为局部备选。
- **OpenModelDB/Spandrel 定位**:仅作为模型发现渠道,不作为许可证安全的代理指标;每个候选权重必须逐一核实其 LICENSE 声明。

rounds: ≈9
