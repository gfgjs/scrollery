---
status: active
type: working-memory
line: 自定义ICC与色域切换
created: 2026-07-23
---

# 任务计划:自定义ICC与色域切换

## 目标
A 档:缩略图链补 ICC→sRGB 投影,修广色域源缩略图色偏(既有缺陷),并给 `DecodedImage` 铺 `icc` 地基。
B 档:查看器「渲染色域」特性——sRGB / Display P3 / DCI-P3 / 自定义 ICC 导入,走 Rust 派生渲染(moxcms source→target,输出嵌 target ICC,浏览器做 target→display),桌面先行。
C 档(显示器校准链 / HDR tone-mapping)明确不做。

## 当前阶段
阶段 5:B 施工(rust ∥ 前端,在飞)

## 依赖 DAG
```
S1(DecodedImage 构造点)─┬→ 阶段2 A施工 → 阶段4 A复核+收尾 ─┐
S2(缩略图链/引擎路由)──┘                                  ├→ 阶段7 全量门禁 → 收口(待用户明示)
S3(B侧锚点)→ 阶段3 B设计(architect)→ 阶段5 B施工(rust∥fe)→ 阶段6 B复核+收尾 ─┘
```
A 线与 B 线无真依赖,全程并行;B施工 rust/前端按文件域不相交并发。

## 阶段

### 阶段 1:摸底(并发)✅ complete — S1/S2/S3 三路摸底齐(回执在 attachments/recon-A、recon-B + S1 构造点清单已消费进任务卡)

### 阶段 2:A 施工(依赖 S1+S2+主线裁决)✅ complete — A 施工落地,commit 0d677d3

### 阶段 3:B 设计(architect,依赖 S3)
- [x] IPC 派生渲染命令形态、派生缓存键与 tmp+rename、自定义 ICC 导入校验与存放、设置项与前端接线、平台门控
- [x] 计划落 findings,主线裁决后开阶段 5
- **状态:** ✅ complete — architect 计划已批入 attachments/plan-B.md(主线裁决头:文件回传/组R排A后)

### 阶段 4:A 复核+收尾 ✅ complete — reviewer(opus)1 警告 3 建议→警告+2 建议已修(limits 守卫/assert_ne/防御注释,主线补 import 修复)→增量核验三处全过,同入 0d677d3

### 阶段 5:B 施工(rust ∥ 前端,文件域不相交)
- **状态:** ✅ complete — commit 0e1aa8b;R1(sonnet)33 轮停机收束→R2(opus)34 轮接盘,F(sonnet)14 轮,F/R 文件域不相交并行

### 阶段 6:B 复核+收尾
- **状态:** ✅ complete — F 复核 opus(2警告2建议1存疑)+ R 复核 opus(3警告1建议1存疑),两修复批(sonnet)全落,增量核验双双通过,同入 0e1aa8b

### 阶段 7:批末全量门禁
- [x] DecodedImage 属跨 crate 共享契约 → 全量 cargo test + clippy + vitest + vue-tsc(quiet),一轮清账
- **状态:** ✅ complete(本线面全绿)— cargo test 946 passed/6 ignored/0 failed(退出0);cargo clippy --lib -D warnings 退出101,但失败点全在外线面 exotic/validate.rs(exotic_protocol::OcrItem/OcrItemResult/OcrLine 未解析导入),属并行 OCR 线在途改动,非本批引入,原样记录不修;vitest 115 files/1397 tests passed(退出0);vue-tsc --noEmit 退出0(空输出);ai-worker cargo check 退出101,同源 exotic_protocol OCR 符号缺失(main.rs OcrSessionReadyBody/handle_request 签名/RequestBody match 未穷尽),外线面,原样记录不修。本线面(viewer_color/editing/thumbnail/engine/ipc/error/state/schema/前端)零红。

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| A:CMS 在 resize 后小图上做;仅 RGB 型 profile 转换,Gray/CMYK/解析失败一律 passthrough,缩略图绝不因 CMS 失败而失败 | 成本最小;color.rs 的 match 只认 (Gray,Luma) 布局而引擎输出恒 RGBA,Gray profile 硬转必失败;缩略图可用性 > 色准 | D-410 |
| B:唯一路线 = Rust 派生渲染嵌 target ICC;canvas colorSpace(只有 srgb/display-p3)与 --force-color-profile(整窗/需重启/不收任意 ICC)不作特性面 | 任意 ICC 只有 CMS 侧能做;浏览器负责 target→display,单次转换语义正确 | D-411 |
| 铁律:像素编码域与嵌入 profile 必须一致,任何输出路径不得只转像素不换 tag(或反之) | 违反即双重转换/漏转换,肉眼可见色偏 | D-412 |
| 编辑链钉死 sRGB(P0-CM 不动);查看器色域独立,两者并排色差是口径不是 bug,文档说明 | P0-CM 勿翻案红线;避免编辑语义漂移 | D-413 |
| B 桌面先行;非桌面(Android WebView 嵌 ICC 色彩管理不可靠)默认锁 sRGB | 跨平台一致性:sRGB 派生是全平台可信基线 | D-414 |
| C 档不立项:显示器校准 ICC 归 OS+浏览器,HDR 另立线 | 自建校准链与浏览器 CMS 打架 | — |
| cache_key 不动,靠既有重新生成入口失效 | 无版本位,免全库无差别重生成 | D-415 |
| exif_thumb 快速路径不做 CMS | 内嵌预览是相机产物,不解主图拿不到可靠 profile | D-416 |
| WIC v1 不出 ICC,heic/avif(iPhone P3 主场)色偏留 A2 后续 | — | D-417 |
| target=custom 且 custom_id 空 → 前端零 IPC + 后端 Ok(None) 优雅回退,不落错误码 | 避免空态误报为错误;前端可静默短路 | D-418 |
| to_target_rgba8 对 None/CMYK 源假定 sRGB 恒投影(修灰度不可渲染+CMYK D-412 破口) | 短路语义留 to_srgb_rgba8 包装层,编辑链字节级不变 | D-419 |
| 16-bit/f32 无 ICC 源在宽色域 target 下「先量化后 CMS」精度损失,裁 defer | 属 B2 后续,非本批阻塞项 | D-420 |

待裁(摸底后,三项全裁):WIC 引擎 v1 是否出 ICC → D-417;缓存失效策略 → D-415;B 派生回传形态 → 文件(plan-B.md 头部主线裁决)。

## 遗留
- A2:WIC GetColorContexts 出 ICC(heic/avif P3)。
- B2:D-420 位深分派(16-bit/f32 无 ICC 源在宽色域 target 下的量化时序)。
- 真机 GUI 验收(见 progress.md GUI 手测清单)。
- WebView2 对嵌 P3 ICC 的 target→display 行为真机验证(plan-B §6-1,若不成立特性语义回炉)。
- 视频渲染色域不支持,已裁不做(派生渲染=全片转码不可行;WebGL 逐帧 LUT 属独立大线须另立项);如需请另立项,先做 WebGL 逐帧 LUT 可行性评估。

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 色域切换偶发不生效(GUI①) | 竞速链亲见定位(watch 先于持久化发 IPC) | setter 先持久化后改 state(92d8396) |
