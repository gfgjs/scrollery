---
id: 2026-07-23-research-web
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# OCR 文字提取技术方案调研(Web)

调研日期:2026-07-23
调研范围:图片/视频当前帧文字提取,中英混排为主,CJK 质量关键,本地优先/离线优先,云 API 仅作对照。
目标平台:Windows 11(主)、macOS/iOS/Android(规划)。技术栈:Tauri v2(Rust 后端 + Vue 3 前端)。

---

## 1. OS 原生 OCR

### 1.1 Windows — Windows.Media.Ocr(WinRT,legacy)

- API 概况:`Windows.Media.Ocr.OcrEngine`,`TryCreateFromUserProfileLanguages()` / `TryCreateFromLanguage()`,按用户资料语言解析可用识别语言;文档明确"必须在设备上安装对应语言包,用户可通过 Windows 设置安装新的 OCR 语言包"。
  来源:[MS Learn OcrEngine.TryCreateFromUserProfileLanguages](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine.trycreatefromuserprofilelanguages?view=winrt-26100)、[MS Learn OcrEngine Class](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine?view=winrt-26100)
- 该引擎是 Windows 10 起随系统内置、"Windows Search / OneNote 文字提取"同款引擎,零额外依赖、零成本,但语言包需用户主动安装。
  来源:[Windows Developer Blog 2016-02-08](https://blogs.windows.com/windowsdeveloper/2016/02/08/optical-character-recognition-ocr-for-windows-10/)
- 中文/CJK 准确率口碑:搜索未找到量化的中文准确率数据或用户投诉专帖,只查到泛泛评价"Windows OCR Engine 语言能力相对其他方案有限"。**标注未验证**(缺乏具体数据支撑)。
  来源:[IronSoftware: Windows OCR Engine vs Tesseract](https://ironsoftware.com/csharp/ocr/blog/ocr-tools/windows-ocr-engine-vs-tesseract/)
- Rust 接入:`windows` crate(microsoft/windows-rs)提供 `windows::Media::Ocr` 绑定(`OcrEngine` 等),文档存在但未找到公开的完整 Tauri/Rust 接入示例项目。社区侧有非官方封装 crate `win_ocr`(crates.io,未能取到详细版本/更新时间,判定为小众社区项目,**未验证**其维护活跃度)。
  来源:[windows::Media::Ocr docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Media/Ocr/)、[win_ocr crates.io](https://crates.io/crates/win_ocr)

### 1.2 Windows 11 新一代 — OneOCR(Snipping Tool / Windows 11 新增)

- OneOCR 是 Windows 11 新版"截图工具(Snipping Tool)"内置的 AI 增强 OCR 引擎,比旧版 Windows.Media.Ocr 模型更新;2025 年起 PowerToys Text Extractor 的能力被并入系统级 Snipping Tool 全屏文字提取。
  来源:[windowslatest 2025-04-16](https://www.windowslatest.com/2025/04/16/windows-11-now-lets-you-extract-texts-ocr-from-your-screen-without-screenshots/)、[BetaNews 2025-02-21](https://betanews.com/2025/02/21/microsoft-is-giving-snipping-tool-a-major-ocr-upgrade-in-windows-11/)
- **关键限制:OneOCR 目前没有公开官方 API/SDK**。现有 Rust/Python 接入方案均为社区逆向:需从 Snipping Tool 安装目录手动提取 `oneocr.dll`、`oneocr.onemodel`、`onnxruntime.dll` 三个文件放到自身可执行文件旁,属于非官方逆向使用,**存在合规/长期支持性风险**(微软可随时更新格式导致失效,亦不排除违反许可条款)。
  来源:[AuroraWright/oneocr (GitHub)](https://github.com/AuroraWright/oneocr)、[wangfu91/oneocr-rs (GitHub,MIT,58 commits/6 releases)](https://github.com/wangfu91/oneocr-rs)、[MattyMroz/oneocr ONNX 复刻 (HuggingFace)](https://huggingface.co/MattyMroz/oneocr)
- `oneocr-rs`(wangfu91)README 明确注明"仅在 Windows 11 测试,不保证 Windows 10 可用"。CJK 支持程度未在 README 中说明,**未验证**。
- PowerToys 官方 Text Extractor 模块本身走的是 `Windows.Media.Ocr` API(而非 OneOCR),这与"Snipping Tool 已升级到 OneOCR"是两条并行事实,注意不要混淆。
  来源:[MS Learn PowerToys Text Extractor](https://learn.microsoft.com/en-us/windows/powertoys/text-extractor)

### 1.3 macOS/iOS — Apple Vision(VNRecognizeTextRequest)

- API:`VNRecognizeTextRequest`,`recognitionLevel` 分 `.fast` / `.accurate` 两档。**中文识别必须使用 `.accurate` 档**——开发者论坛明确反馈 `.fast` 档完全无法识别中文字符。
  来源:[Apple Developer: recognitionLevel](https://developer.apple.com/documentation/vision/vnrecognizetextrequest/recognitionlevel)、[Apple Developer Forums #691708](https://developer.apple.com/forums/thread/691708)
- 已知限制:横排文字识别良好,**竖排中文文本无法识别**("nothing is recognized" 当字符竖排时);且不支持中文单字级别的独立识别。
  来源:[Apple Developer Forums #749234](https://developer.apple.com/forums/thread/749234)
- 未找到官方公布的中文准确率量化数据,**未验证**。
- Rust 接入:`objc2-vision` crate(objc2 生态,madsmtm 维护),提供 Vision 框架绑定,含 `VNRecognizedTextObservation` 等类型;支持 macOS 10.13+ / iOS 11.0+ / tvOS 11.0+ / visionOS 1.0+。成熟度:objc2 是 Apple 框架 Rust 绑定中维护最活跃、生态最完整的项目之一,但 objc2-vision 子 crate 相对较新,未找到大规模生产落地案例佐证,**接入成熟度判定为"可用但需自行摸索"**。
  来源:[objc2-vision crates.io](https://crates.io/crates/objc2-vision)、[objc2-vision docs.rs](https://docs.rs/objc2-vision)、[madsmtm/objc2 GitHub](https://github.com/madsmtm/objc2)

### 1.4 Android — ML Kit Text Recognition v2

- 支持拉丁、中文、日文、韩文、天城文(Devanagari)五种文字体系,**每种文字体系是独立模型**,需要在依赖中按需引入,例如识别中文需单独引入 `play-services-mlkit-text-recognition-chinese:16.0.1`;没有统一的多文种模型。
  来源:[Google ML Kit Text Recognition v2 languages](https://developers.google.com/ml-kit/vision/text-recognition/v2/languages)、[Google ML Kit Text Recognition v2 概览](https://developers.google.com/ml-kit/vision/text-recognition/v2)
- 完全端上运行,不联网、不产生云端账单,无隐私上传;但依赖 **Google Play Services** 分发模型(应用安装后自动下载)。**这对无 GMS 环境(如部分国行安卓机型/定制 ROM)是可用性风险**——此点为基于 Play Services 依赖机制的推断,**未直接检索验证国行场景实测数据**。
- Rust/Tauri 接入路径:Tauri v2 移动插件通过 Kotlin/Java 编写原生逻辑,再用 `jni-rs` 做 JNI 桥接;Tauri 官方文档提到团队评估过 `jni-rs` 但认为直接手写 JNI 样板代码繁琐,倾向让插件的 Kotlin/Swift 层直接实现业务逻辑、Rust 侧只做薄封装调用。**成熟度:路径可行,但没有找到现成的"Tauri + ML Kit Text Recognition"开源插件**,需要自行封装(工作量属中等)。
  来源:[Tauri v2 Mobile Plugin Development](https://v2.tauri.app/develop/plugins/develop-mobile/)

---

## 2. 跨平台引擎

### 2.1 Tesseract 5(+ Rust 绑定)

- License:Apache License 2.0(引擎本体)。
- Rust 绑定现状:
  - `leptess`(houqp 维护):MIT 许可,GitHub 111 commits/286 stars,含 CI workflow,判定"仍在维护"但**未取得具体最近提交日期**,标"活跃度中等,未验证近期活跃度精确值"。
    来源:[houqp/leptess GitHub](https://github.com/houqp/leptess)
  - `tesseract-rs`:近期有 0.1.20(2025-07-27)与 0.2.0(2026-03-23)两个版本发布记录,内置编译 Tesseract/Leptonica,判定**维护活跃**。
    来源:[tesseract-rs docs.rs](https://docs.rs/crate/tesseract-rs/latest)
- traineddata 体积(简体中文 `chi_sim`):`tessdata_fast` 版约 2.35 MB,`tessdata_best` 版约 42.3 MB;繁体中文(`chi_tra`)`tessdata_best` 版约 12.4 MB。
  来源:[tesseract-ocr/tessdata_fast (GitHub)](https://github.com/tesseract-ocr/tessdata_fast)、[tesseract-ocr/tessdata_best (GitHub)](https://github.com/tesseract-ocr/tessdata_best)
- 中英/CJK 准确率口碑:对高质量扫描文档,Tesseract 可达 95–99%;但对复杂真实场景图片(倾斜、艺术字、混排),口碑上**明显弱于 PaddleOCR**,尤其倾斜/旋转文本检测能力差(Tesseract 无专门文本检测网络,依赖版面分析假设)。
  来源:[IronSoftware: PaddleOCR vs Tesseract](https://ironsoftware.com/csharp/ocr/blog/compare-to-other-components/paddle-ocr-vs-tesseract/)、[PaddleOCR GitHub Discussion #8349](https://github.com/PaddlePaddle/PaddleOCR/discussions/8349)
- 离线:是。移动端:Tesseract 可交叉编译到 iOS/Android(社区有先例),但官方无一等公民支持,**移动端可行性判定为"可行但集成成本高"**,未见 Rust 生态下的现成移动端绑定。

### 2.2 PaddleOCR 生态(PP-OCRv4/v5 + RapidOCR/ONNX 化 + Rust crate)

- License:PaddleOCR 本体 Apache 2.0。
  来源:[PP-OCRv5 HuggingFace blog](https://huggingface.co/blog/baidu/ppocrv5)
- 模型体积(ONNX 导出,来自 oar-ocr 文档整理的模型清单):
  - 检测(det):PP-OCRv4 mobile ≈4.6MB / server ≈108.2MB;PP-OCRv5 mobile ≈4.6MB / server ≈84.0MB
  - 识别(rec):PP-OCRv4 mobile ≈10.4MB / server ≈86.3MB;PP-OCRv5 mobile ≈15.8MB / server ≈80.6MB
  - 即 mobile 档 det+cls+rec 三模型合计约 20–30MB 量级,server 档合计约 150–190MB 量级。
  来源:[PaddlePaddle/PP-OCRv4_mobile_rec README (accuracy 83.28%,体积 11M)](https://huggingface.co/PaddlePaddle/PP-OCRv4_mobile_rec/blob/8356d0681255f2313b393c00c5903b0eeccdb447/README.md)、[PP-OCRv5 官方文档](https://paddlepaddle.github.io/PaddleOCR/main/en/version3.x/algorithm/PP-OCRv5/PP-OCRv5.html)
- CJK/多语言:PP-OCRv5 支持简中、繁中、英文、日文、拼音五类文种,较 v4 在手写体、古籍、日文检测上有显著提升;PP-OCRv6(2026 新发布)进一步从 1.5M 参数做到 34.5M 参数档位,论文声称超越部分十亿参数级 VLM 的 OCR 任务表现(**这是论文自述数据,未经第三方复核,标"未验证/待独立复现"**)。
  来源:[arxiv 2606.13108 PP-OCRv6](https://arxiv.org/html/2606.13108v1)
- Rust crate 现状(均社区维护、非官方):
  - `oar-ocr`(GreatV):Apache-2.0,v0.8.0(2026-07-08 发布),支持 PP-OCRv3/v4/v5/v6 检测+识别管线,另有版面分析、表格、公式识别扩展;是目前功能最全的 Rust 原生 OCR 工具箱。
    来源:[GreatV/oar-ocr GitHub](https://github.com/GreatV/oar-ocr)、[oar-ocr models.md](https://github.com/GreatV/oar-ocr/blob/main/docs/models.md)
  - `paddle-ocr-rs`(mg-chao):Apache-2.0,基于 ort 调用 Paddle OCR ONNX 模型(含 `ch_PP-OCRv5_mobile_det.onnx` 等),参考实现来自 RapidAI/RapidOCR;85 stars/20 forks,近期仍有维护(2025-06-24 有模型相关 release)。
    来源:[mg-chao/paddle-ocr-rs GitHub](https://github.com/mg-chao/paddle-ocr-rs)
  - `rust-paddle-ocr`(zibo-chen):声称支持 PP-OCR v4/v5/v6,MNN 后端(非 ONNX Runtime),支持 10+ 语言,提供 Rust crate + C API + CLI;体积/性能数据**未逐一核实**。
    来源:[zibo-chen/rust-paddle-ocr GitHub](https://github.com/zibo-chen/rust-paddle-ocr)
  - 上述三者均为个人/小团队维护的社区项目,**没有一个是 PaddlePaddle 官方或大型商业公司背书的 Rust 绑定**,需评估长期维护风险。
- 参照实现(非 Rust,供设计参考):`OnnxOCR`(jingsongliujing),基于 PaddleOCR 重构、完全脱离 PaddlePaddle 训练框架、纯 ONNX 推理,证明 PP-OCR 模型可以脱离 Paddle 框架独立运行,思路与 Rust+ort 方案一致。
  来源:[jingsongliujing/OnnxOCR GitHub](https://github.com/jingsongliujing/OnnxOCR)

### 2.3 ocrs(robertknight,纯 Rust + RTen)

- License:MIT / Apache-2.0 双许可,纯 Rust 实现(模型用 PyTorch 训练导出 ONNX,由 RTen 引擎推理,无需 C/C++ 依赖或 ONNX Runtime),对静态编译/移动端跨平台友好。
- **关键限制:官方 `robertknight/ocrs` 只支持拉丁字母,不支持 CJK**。
  来源:[robertknight/ocrs GitHub](https://github.com/robertknight/ocrs)、[ocrs CHANGELOG](https://github.com/robertknight/ocrs/blob/main/CHANGELOG.md)
- 社区 fork `ocrs-cjk`(kent-tokyo)扩展了 CJK 支持,基于 PaddleOCR 模型做 CJK 感知分词、支持可搜索 PDF 输出:
  - License:Apache-2.0 / MIT 双许可
  - 最新版本 v0.1.0(2026-06-26 发布),**明确自称"early preview,预期比商用 OCR 引擎有更多错误"**
  - CJK 准确率自述:简体中文印刷体识别率约 90%,日文约 74%;合成图像测试中平假名/片假名/汉字/混排字符错误率 0%,但**繁体中文生僻字错误率高达约 67%**
  - 已知限制:无内置 CJK 检测模型(检测模型仍是拉丁体系训练)、字母表必须与模型训练字典严格匹配、PDF 支持仅限逐页单图扫描件
  来源:[kent-tokyo/ocrs-cjk GitHub](https://github.com/kent-tokyo/ocrs-cjk)
- **结论:ocrs/ocrs-cjk 目前不适合作为 Scrollery CJK 场景的主力方案**,可作为长期观察项(纯 Rust、零 C 依赖对跨平台打包有吸引力,但准确率和成熟度不足)。

### 2.4 其他(仅对照,不展开)

- EasyOCR(PyTorch,Python 生态,非 Rust 友好,GPL 相关依赖需核实,此处不展开,**未验证**)。
- PP-OCR 之外的商业 SDK(如 ABBYY FineReader Engine)非开源、授权费用高,不契合"本地优先+成本可控"定位,不展开。

---

## 3. ONNX Runtime(ort crate)路线细节

- `ort` crate(pykeio 维护):双许可 Apache-2.0 / MIT,最新 release **v2.0.0-rc.12(2026-03-05)**,主分支 978 commits/33 releases,判定**活跃维护**。
  来源:[pykeio/ort GitHub](https://github.com/pykeio/ort)
- 平台支持:ONNX Runtime 官方支持 Linux/Windows/macOS/iOS/Android/浏览器(WASM);iOS 走 CoreML + XNNPACK 执行提供程序,Android 走 NNAPI + XNNPACK。ort 作为绑定层理论上继承这些平台能力,但**未找到 ort 在 iOS/Android 上端到端跑通 PP-OCR 的公开生产案例**,交叉编译移动端 ONNX Runtime 本身有一定工程成本(需按官方 mobile 构建文档单独编译精简版 runtime)。
  来源:[ONNX Runtime Mobile 文档](https://onnxruntime.ai/docs/get-started/with-mobile.html)、[ONNX Runtime Deploy on mobile](https://onnxruntime.ai/docs/tutorials/mobile/)
- PP-OCR/RapidOCR 在 ort 上的三模型(det+cls+rec)管线是社区公认的标准做法(见 §2.2 的 `oar-ocr`/`paddle-ocr-rs`/`rust-paddle-ocr` 均采用此结构)。
- 速度量级(官方 benchmark,非 Rust/ort 环境实测,Intel Xeon 8350C CPU,单图端到端 det+cls+rec):PP-OCRv6_tiny ≈0.22s/图,PP-OCRv6_small ≈0.61s/图,PP-OCRv6_medium ≈3.31s/图,PP-OCRv5_server ≈6.36s/图。**注意:这是官方服务器级 CPU 上的数字,不同档位差异巨大(tiny 到 server 相差近 30 倍),不能直接当作 Scrollery 桌面端预期速度,需要用目标档位模型自行实测**。
  来源:[arxiv 2606.13108 PP-OCRv6](https://arxiv.org/pdf/2606.13108)
- **未取得 Rust/ort 环境下的第一手实测速度数据**,§3 速度结论标"部分未验证——需自行基准测试确认"。

---

## 4. 业界产品做法

| 产品 | OCR 方案 | 来源/置信度 |
|---|---|---|
| PowerToys Text Extractor | 官方文档确认走 `Windows.Media.Ocr` API | [MS Learn](https://learn.microsoft.com/en-us/windows/powertoys/text-extractor) 高置信 |
| Windows 11 Snipping Tool(2025+ 全屏文字提取) | OneOCR(AI 增强,非公开 API,社区逆向可用) | [windowslatest](https://www.windowslatest.com/2025/04/16/windows-11-now-lets-you-extract-texts-ocr-from-your-screen-without-screenshots/) 中等置信(引擎细节靠社区逆向确认) |
| macOS Live Text | 官方未公开底层实现细节,合理推断复用 Vision 框架同源模型 | **未验证**,推断 |
| PixPin(国内截图工具) | 博客/CSDN 来源称集成 "PearOCR" 离线识别引擎 | [CSDN 博客](https://blog.csdn.net/goodfat/article/details/149686751) **低置信度**(非官方一手来源) |
| Snipaste | 本轮搜索**未查得**其 OCR 方案细节(Snipaste 官方本身长期不带 OCR,需插件/第三方) | no-source,已查:WebSearch 中英文关键词组合 |
| uTools | 搜索结果仅提及用户从 uTools 截图功能迁移到 PixPin,**未查得** uTools OCR 引擎具体实现 | no-source |
| Eagle | 官方无内置 OCR;社区插件 "Copy Image Text" **调用 Google OCR API(需联网,非本地)**;另有 "AI Autotagger" 插件含 Extract Text(OCR)能力,底层引擎未注明 | [Eagle 插件页 Copy Image Text](https://community-en.eagle.cool/plugin/eagle-plugin-copy-image-text-google)、[Eagle AI Autotagger 插件](https://community-en.eagle.cool/plugin/4B56113D-EB3E-4020-A82C-6214FA08CB14)、[Eagle 官方支持文章](https://en.eagle.cool/support/article/does-eagle-support-exif-facial-recognition-gps-or-image-text-ocr-in-search) 中等置信 |
| Billfish | 知乎第三方文章提及"语义搜索含 OCR(支持艺术字)",**未在 Billfish 官方文档/更新日志中直接核实到 OCR 功能条目** | [知乎文章](https://zhuanlan.zhihu.com/p/151061991) **低置信度** |
| Adobe Bridge | 本轮搜索**未查得**官方 OCR 功能说明,推断 Bridge 定位为 DAM/预览工具、不含原生 OCR | no-source,推断 |

---

## 5. 方案对照总表

| 方案 | 中英/CJK 准确率口碑 | 体积 | License | Rust 集成成熟度 | 离线性 | 移动端可行性 | 速度量级 |
|---|---|---|---|---|---|---|---|
| Windows.Media.Ocr | 未验证(无量化数据),泛评"能力有限" | 0(系统自带,需语言包) | 系统 API,无需额外许可 | `windows` crate 有绑定但缺实战范例;社区 `win_ocr` 成熟度未验证 | 是(语言包需预装) | 不适用(Windows only) | 未验证 |
| OneOCR(逆向) | 未验证 | 需从系统提取 DLL,体积未知 | 非官方逆向,**无官方许可**,合规风险 | `oneocr-rs`(MIT)已有社区绑定,判定"可用但脆弱" | 是 | 不适用(Windows 11 only) | 未验证 |
| Apple Vision | 中文需 `.accurate` 档,竖排不支持;无量化数据 | 0(系统自带) | 系统 API | `objc2-vision` 绑定存在,较新 | 是 | 是(iOS 原生支持) | 未验证 |
| Android ML Kit v2 | 官方称支持中日韩,无第三方量化对比数据 | 按需下载(Play Services 分发) | Google 专有(免费使用,闭源) | 需自行封装 Tauri 插件(Kotlin+JNI),无现成开源插件 | 是(端上推理,依赖 GMS 分发模型) | 是(Android 原生) | 未验证 |
| Tesseract 5 | 高质量扫描件 95–99%,真实复杂场景明显弱于 PaddleOCR | chi_sim fast 2.35MB / best 42.3MB;chi_tra best 12.4MB | **Apache-2.0(非 GPL,无红线问题)** | `tesseract-rs`(0.2.0,2026-03-23)活跃;`leptess`(MIT)活跃度中等 | 是 | 可行但需自行交叉编译,成本高 | 未验证(口碑上比 PP-OCR 慢) |
| PP-OCRv4/v5(+ort) | 口碑上优于 Tesseract,复杂/倾斜场景优势明显 | mobile 档 det+rec 合计约 20–30MB;server 档约 150–190MB | **Apache-2.0** | `oar-ocr`(v0.8.0,2026-07-08)功能最全;`paddle-ocr-rs`/`rust-paddle-ocr` 均社区维护,无官方背书 | 是 | 理论可行(需自编译 mobile ONNX Runtime),**未见生产案例** | 官方 CPU benchmark 0.22–6.36s/图(档位差异大,非 ort 实测) |
| ocrs / ocrs-cjk | 官方版不支持 CJK;fork 版简中约 90%、日文约 74%,繁体生僻字错误率约 67% | 未详细披露 | MIT/Apache-2.0 双许可 | ocrs-cjk 早期预览(v0.1.0,2026-06-26),不建议现在用于生产 | 是(纯 Rust,零 C 依赖) | 理论最佳(纯 Rust 静态编译),但准确率不足 | 未验证 |
| 云 API(Google Vision / Azure Read,仅对照) | 东亚语言口碑最佳;Azure 印刷体准确率 99.8%(第三方博客数据,**低置信度**) | 0(远程) | 按调用计费 | HTTP 客户端即可,集成简单 | **否,与离线优先定位冲突** | 是,但违背隐私敏感前提 | 网络往返延迟,非本地推理速度 |

来源合并:[Google Cloud Vision vs AWS vs Azure 2026 对比](https://imagetotable.ai/blog/google-vs-aws-vs-azure-ocr-2026)、[Google Vision 定价](https://www.buildmvpfast.com/alternatives/google-vision)

---

## 6. 候选组合方案(供立项决策)

**方案 A:全原生路线**——Windows(Windows.Media.Ocr 或 OneOCR 逆向)+ macOS/iOS(Apple Vision)+ Android(ML Kit)。
优点:零模型体积、零推理成本、隐私最佳、速度依赖系统级优化通常较快。
风险:四端四套代码路径、准确率/行为不可能完全一致,**OneOCR 无官方 API 存在合规和长期可用性风险**,ML Kit 依赖 Google Play Services(无 GMS 设备不可用),Windows.Media.Ocr 依赖用户自行安装语言包、且缺少量化质量证据,长期 QA 成本高。

**方案 B:全平台统一 ONNX PP-OCR**——`ort` + PP-OCRv5(或 v6)三模型管线,走 `oar-ocr` 或自研 det+cls+rec pipeline,四端共用同一套 Rust 代码与模型资产。
优点:CJK 准确率口碑最佳、代码路径统一、Apache-2.0 无 license 风险、离线、结果可预测(同一模型同一逻辑)。
风险:桌面端模型体积约 20–30MB(mobile 档)需要打包/首启下载,iOS/Android 需自行编译移动端 ONNX Runtime、**没有验证过的移动端生产案例**,所依赖的 Rust crate(oar-ocr/paddle-ocr-rs/rust-paddle-ocr)均为个人/小团队维护、无官方背书,存在长期维护风险,速度需自行实测(官方 benchmark 档位差异达 30 倍,不能直接套用)。

**方案 C:混合路线**——桌面(Windows/macOS)优先用系统原生 OCR,移动端(iOS/Android)用系统原生 OCR;仅在系统 OCR 不可用(语言包缺失/系统版本过旧)或需要统一批量处理行为时,fallback 到 ONNX PP-OCR 本地引擎。
优点:兼顾体积与准确率,主路径复用系统免费能力,fallback 保底质量与跨端一致性,可分阶段演进(先原生打通体验,PP-OCR fallback 后置为二期)。
风险:两套代码路径长期并存维护成本翻倍,fallback 触发条件与降级 UX 设计复杂,QA 矩阵(系统版本 × 语言包状态 × fallback 触发)显著增大。

---

## 7. 低置信度结论(逐条标注原因)

1. **Windows.Media.Ocr 中文识别质量的量化数据**——搜索未命中具体准确率数字或用户投诉专帖,只有"能力相对有限"的泛化评价,原因:检索到的均为二手评测博客,未找到微软官方或第三方量化基准。
2. **OneOCR 的 CJK 支持程度**——`oneocr-rs` README 未说明,原因:项目文档信息量有限,且 OneOCR 本身无官方文档。
3. **Apple Vision 中文识别准确率数字**——未找到官方或第三方量化数据,原因:检索结果集中在开发者论坛的功能性抱怨(竖排/单字问题),没有准确率基准帖。
4. **PixPin 使用 "PearOCR" 引擎**——来源为 CSDN 博客,非 PixPin 官方文档,原因:PixPin 官方网站未在本轮检索中直接核实此细节。
5. **Billfish 的 OCR 语义搜索能力**——来源为知乎第三方文章,原因:Billfish 官方更新日志页面本轮未能核实到明确的 OCR 功能条目(页面访问受限或描述过简)。
6. **Adobe Bridge 是否有 OCR**——本轮检索无命中,判定为推断性结论(基于产品定位),原因:no-source,已查:WebSearch 中英文关键词组合均未命中直接证据。
7. **Azure OCR 99.8% 印刷体准确率**——来源为第三方聚合博客(imagetotable.ai),非 Azure 官方公布数据,原因:未直接访问 Azure 官方文档核实此数字。
8. **PP-OCR 在 ort(Rust)环境下的实测速度**——本轮只取得 PaddlePaddle 官方在 Xeon 8350C 上的 Python/ONNX Runtime benchmark,原因:未找到 Rust/ort 环境下的第一手速度实测报告,需要自行基准测试才能确认 Scrollery 场景下的真实延迟。
9. **PP-OCRv6 论文"超越十亿参数级 VLM"的表现声称**——来源为论文自述(arxiv 2606.13108),原因:未经第三方独立复现验证,存在自我评测偏差风险。
10. **ML Kit 在无 GMS(如国行安卓定制 ROM)环境下不可用**——基于 Play Services 依赖机制的技术推断,原因:未直接检索到国行场景的实测报告或 Google 官方声明。
11. **Snipaste / uTools 的 OCR 引擎具体实现**——no-source,已查:WebSearch 中英文关键词组合("Snipaste PixPin uTools OCR 引擎 百度 腾讯 离线"),均未命中明确一手信息,只查到 PixPin 相关的间接提及。
12. **`win_ocr` / `rust-paddle-ocr` 两个社区 crate 的具体维护活跃度(最近提交日期、下载量)**——原因:crates.io/lib.rs 页面 WebFetch 均遇到渲染/403 限制,未能取得精确元数据,仅能确认项目存在及 README 描述的功能范围。

---

落盘路径:`C:\workspace\scrollery\docs\planning\2026-07-23-OCR文字提取\research-web.md`
