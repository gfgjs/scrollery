---
id: 2026-07-23-model-assets
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# PP-OCRv5 ONNX 模型资产清单(registry 钉版)

> 调研日期 2026-07-23。方法:WebSearch + WebFetch 交叉核对 RapidOCR 官方 `default_models.yaml`(经 ModelScope 分发)、
> PaddlePaddle 官方 HuggingFace repo(Apache-2.0 license badge 源)、GreatV/oar-ocr GitHub Release(独立 Rust 生态镜像,ONNX 实测字节数)。
> **重要限制**:WebFetch 工具经小模型摘要网页,sha256/文件大小均为"网页转述",非本机 HEAD/sha256sum 直接验证;施工时下载后必须自算 sha256 核对本文档数值,不可盲信直接落 registry。

## 资产表

主源统一为 RapidAI/RapidOCR ModelScope v3.9.2 tag(唯一带官方 sha256 的 ONNX 分发点);备用镜像为 GreatV/oar-ocr GitHub Release v0.3.0(全球 CDN,免 ModelScope 访问依赖,但发布页未附 sha256)。

| 档位 | 模型 | 主 URL(ModelScope resolve) | 备用 URL(GitHub Release) | 大小(来源) | sha256(来源) | License |
|---|---|---|---|---|---|---|
| mobile | det | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_mobile_det.onnx | 4.6 MiB(oar-ocr release 页标注,ONNX 实体)/ 参照 Paddle 权重 4.69MB(HF `PaddlePaddle/PP-OCRv5_mobile_det` 文件列表) | `4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae`(RapidOCR `default_models.yaml`,GitHub raw 渲染转述) | Apache-2.0(HF `PaddlePaddle/PP-OCRv5_mobile_det` model card badge) |
| mobile | cls | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/cls/ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx | (oar-ocr models.md 未列出 cls 条目,未验证是否随 release 一并打包) | Paddle 权重 986 KB(HF `PaddlePaddle/PP-LCNet_x0_25_textline_ori`);ONNX 实际字节数未测,预期同量级 | `54379ae5174d026780215fc748a7f31910dee36818e63d49e17dc598ecc82df7`(同上来源,转述) | Apache-2.0(HF card) |
| mobile | rec | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile.onnx | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_mobile_rec.onnx | 15.8 MiB(oar-ocr release)/ Paddle 权重 16.5MB(HF `PaddlePaddle/PP-OCRv5_mobile_rec`) | `5825fc7ebf84ae7a412be049820b4d86d77620f204a041697b0494669b1742c5`(转述) | Apache-2.0(HF card) |
| mobile | 字典 ppocrv5_dict.txt | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/paddle/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile/ppocrv5_dict.txt | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/ppocrv5_dict.txt | **实测 72.3 KB**(WebFetch 实际下载得到,非转述) | 源未提供(yaml 只标 onnx/paddle 权重 sha256,无 dict sha256),施工时下载后自算 | 归属 PaddleOCR 仓库同许可 Apache-2.0(数据文件,非独立声明) |
| server | det | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/det/ch_PP-OCRv5_det_server.onnx | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_server_det.onnx | 84.0 MiB(oar-ocr release)/ Paddle 权重 87.9MB(HF `PaddlePaddle/PP-OCRv5_server_det`) | `0f8846b1d4bba223a2a2f9d9b44022fbc22cc019051a602b41a7fda9667e4cad`(转述) | Apache-2.0(HF card,已核实) |
| server | rec | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_server.onnx | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_server_rec.onnx | 80.6 MiB(oar-ocr release)/ Paddle 权重 84.4MB(HF `PaddlePaddle/PP-OCRv5_server_rec`) | `e09385400eaaaef34ceff54aeb7c4f0f1fe014c27fa8b9905d4709b65746562a`(转述) | Apache-2.0(HF card) |
| server | cls | https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/cls/ch_PP-LCNet_x1_0_textline_ori_cls_server.onnx | (oar-ocr models.md 未列 cls) | Paddle 权重 6.74MB(HF `PaddlePaddle/PP-LCNet_x1_0_textline_ori`) | `7d3c02ef6c7da8ae08b4347cc7695b2081aae68c325d64375724ecf39c99e743`(转述) | Apache-2.0(HF card) |
| server | 字典 | 同 mobile,目录变为 .../rec/ch_PP-OCRv5_rec_server/ppocrv5_dict.txt | 同 mobile 镜像 | 同上,未逐字节比对两目录下 dict.txt 是否完全一致(推断一致,**未验证**) | 同上 | 同上 |

## 逐项确认

1. **URL**:主链来自 RapidAI/RapidOCR 官方 `python/rapidocr/default_models.yaml`(GitHub 源码 + ModelScope resolve 直链),两次独立 WebFetch(raw.githubusercontent 与 github.com/blob 页面渲染)取得的 URL 与 sha256 完全一致,交叉核对通过。备用镜像 GreatV/oar-ocr GitHub Release v0.3.0 提供相同权重的全球 CDN 直链(其发布页文件命名为 `pp-ocrv5_*` 小写下划线风格,内容应为同一份 PaddleOCR→ONNX 转换产物,但未逐字节比对两镜像是否 byte-identical,**未验证**)。
2. **文件字节大小**:仅 `ppocrv5_dict.txt` 为 WebFetch 实际下载后报告的真实字节数(72.3 KB);其余 ONNX 文件大小均为"网页标注转述"(oar-ocr release 页面文本 + HF Paddle 权重文件列表),非本机 HEAD 请求实测。两个独立来源(oar-ocr ONNX 实测标注 vs HF Paddle 权重字节数)量级互相印证(ONNX 略小于 Paddle 权重,符合预期),可信度中等,**施工时仍须 HEAD/下载后核对**。
3. **sha256**:仅 RapidOCR `default_models.yaml` 一个来源提供,两次独立 WebFetch 转述数值完全一致(降低 LLM 转录随机错误的担忧,但仍非本机计算),标记为"较高置信但未本机验证"。
4. **License**:PaddlePaddle 官方 HuggingFace 全部 6 个模型 repo(mobile/server 的 det/rec + 两档 cls)model card 均显示 Apache-2.0 badge,与 Apache-2.0 兼容要求一致。dict.txt 本身未见独立 LICENSE 声明,视为随源仓库同许可的数据文件。
5. **字典匹配**:`ch_PP-OCRv5_rec_mobile` 与 `ch_PP-OCRv5_rec_server` 在 RapidOCR yaml 中的 `dict_url` 均指向各自目录下同名 `ppocrv5_dict.txt`(路径不同、文件名相同),内容按惯例应为同一份 PP-OCRv5 通用字典,但**未逐字节比对两份文件是否完全一致**,标"未验证"。实测下载的 dict.txt 内容为单行单字符格式,UTF-8 编码确认(含 CJK 汉字/日文假名/拉丁字母数字/希腊字母/西里尔字母/emoji/数学符号等),编码层面无问题;精确行数未核实(WebFetch 摘要给出"约 3400+ 行"的粗略采样计数,该数字对大文件不可靠,施工时须本地 `wc -l` 或读取后 `len(lines)` 精确核对再与 rec 模型输出维度对拍)。
6. **cls 结论**:PP-OCRv5 **有专属 cls 模型**,mobile/server 两档**各自独立**、不共用:
   - mobile 用 `ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx`
   - server 用 `ch_PP-LCNet_x1_0_textline_ori_cls_server.onnx`
   两者均为"文本行方向分类"(textline orientation),命名和架构(PP-LCNet_x0_25 / x1_0)已不同于旧版 `ch_ppocr_mobile_v2.0_cls`(PP-OCRv2 时代的整图方向分类器)。**任务预设的"cls 若两档共用注明"这一前提与实际不符** —— 实际应各钉各的 cls,不可用同一个文件覆盖两档。来源:RapidOCR `default_models.yaml`(两条独立条目)+ HuggingFace `PaddlePaddle/PP-LCNet_x0_25_textline_ori` 与 `PaddlePaddle/PP-LCNet_x1_0_textline_ori` 两个独立 repo(均 Apache-2.0)。

## 兼容性附带确认

- **opset**:社区转述(WebSearch 摘要,未直接读到 paddle2onnx 一手转换命令)称 PaddleOCR 官方 paddle2onnx 导出 PP-OCRv5 常用 **opset 17**。ONNX Runtime 官方兼容性文档(onnxruntime.ai/docs/reference/compatibility.html)称任意 ORT 版本向下兼容 ONNX opset 7 至该版本发布时的最新 opset;ORT 1.20+ 系列已支持到 opset 21/22,故 opset 17 应在支持范围内。**未搜到明确的"PP-OCRv5 onnx 在 onnxruntime 1.2x 上 opset 不兼容"的具体 issue 报告**,该结论为推断,非一手确证。
- **ort 2.0.0-rc.12 绑定的具体 onnxruntime 版本号**:本轮未直接核实(需查 `ort` crate 的 Cargo.toml/release notes),此处仅采用任务描述"1.2x"的既有说法,**未验证**。
- **RapidOCR/oar-ocr 踩坑记录**:
  - GitHub `RapidAI/RapidOCR` issue #514:PP-OCRv4 server det 在 RapidOCR(CPU 与 DirectML 均含)上比原生 PaddleOCR 慢 2–3 倍(101–210ms vs 30–59ms),是**性能**问题而非正确性/算子缺失问题,与"DirectML 算子覆盖缺失"不是同一类坑,可作为 DirectML 路径需要性能实测的信号。
  - `docling-project/docling` discussion #2249:RapidOCR 3.x 起配置层强制要求 `dict_url` 字段(即便本地已提供字典路径也报 `ConfigKeyError`),属于 RapidOCR **自身封装层**的配置架构问题,不是 onnx 模型本身或 onnxruntime 层面的坑;Scrollery 若不复用 RapidOCR 的 Python 封装、自行走 ort crate 直接加载 onnx,则不会遇到这个特定坑。
  - **动态输入尺寸**:WebSearch 找到 Paddle2ONNX 历史 issue 提及"PaddleOCR 转 ONNX 后跑在含 dynamic shape 的模型上,concat 算子报 'Non concat axis dimensions must match'",官方给出的对策是用 ONNX Runtime 自带的 `make_dynamic_shape_fixed` 工具把动态维度定死为固定尺寸。此为已知的一类真实坑,但未确认是否命中 PP-OCRv5(该 issue 未标注具体版本),标"未验证是否影响本清单模型"。
  - 未搜到 PP-OCRv5 专属的、明确标注版本号的 opset/DirectML 算子缺失 bug 报告。

## 顺手发现

无(未发现与本次调研目标无关但需上报的问题)。

## 2026-07-23 追记:资产源已钉定(用户裁决「先做一个能用的,后期换自有仓库」)

主源单点定为 RapidAI/RapidOCR ModelScope v3.9.2(镜像 GreatV/oar-ocr GitHub Release v0.3.0,cls 两档无镜像),回填进 `src-tauri/src/ai/ocr_registry.rs::PINNED_ASSETS`。本次回填**用本机 HTTP 直连验证**,升级了上表多条「转述/未验证」结论:

- **字节数**:对全部 7 件文件发 `Range: bytes=0-0` 请求,读服务器 `Content-Range` 响应头取得精确总字节数(非网页标注估算)。det/cls/rec 六个 onnx 实测:mobile 4,819,576 / 1,018,508 / 16,631,306;server 88,118,768 / 6,776,876 / 84,577,022。
- **sha256**:对全部 6 个 onnx 文件发 HEAD 请求,读 `X-Linked-Etag` 响应头(ModelScope 服务器直给,非页面转述),与 `default_models.yaml` 载明值逐一比对**完全一致**——原表标注的「较高置信但未本机验证」现已本机验证通过。
- **dict.txt**:官方 yaml 无 sha256 字段(dict_url 单独一条,无校验值)。本机下载 mobile/server 两档 dict.txt 并 `cmp` 字节级比对,**确认完全一致**(此前「未验证」)。自算 sha256 = `d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b`,实测 74,012 字节、18,383 行(此前 WebFetch 摘要「约 3400+ 行」证实不可靠,已淘汰)。
- GreatV/oar-ocr 镜像:实测 det/rec(mobile+server 四件)+ dict 共 5 个 URL 均 200 可达;cls 两档确认无对应发布文件(此前调研判断成立)。是否与 ModelScope byte-identical 未逐字节比对,但 `ModelAsset.sha256` 校验以 ModelScope 主源值为准,镜像仅作为下载失败时的备用直链,不改变完整性判据。

`OCR_ASSET_BASE` 单常量+`asset_url(base, filename)` 拼接的旧设计已废弃——ModelScope 四类文件(det/cls/rec/dict)分属不同子路径(dict 甚至在 `paddle/` 树而非 `onnx/` 树),无法用单一 base+文件名拼出;`PINNED_ASSETS` 改为逐文件全量 URL 表。

临时性:用户已明示后续会切换到自有仓库托管,届时只需替换 `PINNED_ASSETS` 的 `url`/`mirror_url`。

## 低置信度结论(2026-07-23 前;上方追记已覆盖其中数条,不逐条回改,以追记为准)

- 全部 sha256 数值:来自 WebFetch 对网页的 LLM 转述,非本机 sha256sum 直接计算,两次独立抓取结果一致降低了随机转录错误风险,但不能排除源头页面本身的抓取偏差,施工时须重新计算核对。
- 除 `ppocrv5_dict.txt`(72.3 KB 为实测)外的全部 ONNX 文件大小:均为网页标注转述(oar-ocr release 页文本 / HF 文件列表页文本),未做 HTTP HEAD 直接测量。
- opset 17 的具体转换命令来源:仅为 WebSearch 摘要转述,未直接读取 paddle2onnx 官方转换脚本或 release notes 原文。
- `ort` crate 2.0.0-rc.12 绑定的确切 onnxruntime 版本号:本轮未核实,沿用任务描述的"1.2x"提法。
- mobile 与 server 两档 `ppocrv5_dict.txt` 内容是否 byte-identical:未逐字节比对,仅按 RapidOCR 目录结构惯例推断一致。
- dict.txt 精确总行数(需与 rec 模型输出维度对拍):WebFetch 摘要给出的"约 3400+ 行"是对大文件的粗略采样计数,不可靠,需本地精确核对。
- GreatV/oar-ocr GitHub Release 中的 ONNX 文件是否与 ModelScope/RapidAI 主源 byte-identical:未比对,仅认为同源转换产物。
- "动态输入尺寸导致 concat 算子报错"这条社区坑是否命中 PP-OCRv5(而非更早版本 PaddleOCR 模型):未确认版本范围。

rounds: ≈9(自数)
