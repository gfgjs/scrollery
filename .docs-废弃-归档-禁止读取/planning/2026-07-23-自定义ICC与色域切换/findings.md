---
status: active
type: working-memory
line: 自定义ICC与色域切换
created: 2026-07-23
---

# 发现与决策:自定义ICC与色域切换

## 需求
- 用户:评估自定义 ICC / 自定义切换色域(sRGB、DCI-P3 等)可行性 → 裁定「做 A+B,起三件套开始施工」。
- A=缩略图 ICC→sRGB 投影(修既有色偏)+ DecodedImage.icc 地基;B=查看器渲染色域(sRGB/Display P3/DCI-P3/自定义 ICC),Rust 派生渲染嵌 target ICC;C(显示器校准/HDR)不做。

## 发现

### 评估期现状盘点(2026-07-23,主线已核实代码)
- 编辑链已有完整 moxcms CMS:`src-tauri/src/editing/color.rs` `to_srgb_rgba8`(pub),intent 钉 RelativeColorimetric(P0-CM);CMYK bypass、(Gray,Luma*) 布局匹配、失败落 `edit_decode_failed` 稳定码。
- moxcms 0.8.1 内置构造器齐全:`new_srgb/new_adobe_rgb/new_display_p3/new_dci_p3/new_pro_photo_rgb/new_bt2020(_pq/_hlg)/new_aces_*/new_lab` + `new_from_slice` 任意 ICC(defaults.rs:222-543)。B 档零新依赖。
- 大图显示 = 原文件直接 `<img :src="absPath">`(ContentViewer.vue:51),色彩管理归 WebView2/Chromium(source ICC→display profile),今天已正确。
- 缩略图链零 ICC:`DecodedImage`(crates/scrollery-ai-core/src/decoded.rs:11)只有裸 RGBA;image-rs / WIC 两引擎均丢 profile;WebP/JPEG 编码不嵌 → 广色域源缩略图今天按 sRGB 解读 = 色偏(既有缺陷)。direct-display 小文件走原图不受影响(generator.rs:264-309)。
- WIC 引擎(engine/gpu/wic_engine.rs)无 IWICColorContext/IWICColorTransform。
- tauri 配置无 additionalBrowserArgs;`--force-color-profile` 只收内置名(srgb/display-p3/hdr10 等),整窗生效需重启,只配当诊断开关。
- canvas `colorSpace` 只支持 `'srgb'|'display-p3'`,任意 ICC 前端走不通 → B 只能 Rust 侧。

### 需验证(未测,施工/验收中补)
- Chromium/WebView2 Windows 对系统显示 profile 实际采用行为(真机)。
- moxcms 对 v4 LUT 型 ICC(A2B/mAB)覆盖度——自定义 ICC 导入须有解析失败稳定错误码兜底。
- 100MP 全图 CMS 耗时(估数百 ms 档)→ B 必须 spawn_blocking + 派生缓存。

### 摸底回执索引(阶段 1)
- attachments/recon-A-缩略图链.md(S2 产出)✓consumed→0d677d3
- attachments/recon-B-渲染与前端.md(S3 产出)✓consumed→0d677d3

### A 批复核发现(2026-07-23)
- S2 称「image-rs ICC 不可得」不准确——便捷路径丢弃,`into_decoder()` 可得(editing/metadata.rs:30 先例),已按此施工。

### B 批门禁与混线排雷(2026-07-23,阶段 7)
- 同工作树检出并行 OCR 线的在途改动(crates/scrollery-ai-core ocr 模块/Cargo.toml、src-tauri formats/exotic/scanner/db/router、docs/planning/2026-07-23-OCR文字提取)。B 批提交 0e1aa8b 已排雷:router.rs `builtin:false` 行/formats/exotic/ai-core 等 OCR 相关路径全排除,commit 只收本线显式路径(viewer_color/editing/thumbnail/engine/ipc/error/state/schema/前端)。门禁红(clippy exotic/validate.rs、ai-worker cargo check)经核实均为外线 OCR 符号缺失引入,与本批无关,已原样归属主线,未修。

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-033 | 缩略图链丢 ICC 是既有色偏缺陷:引擎输出裸 RGBA、编码不嵌 profile,广色域源被当 sRGB | experience 或 docs 色彩管理章 |
| F-034 | image crate 从 `.decode()` 便捷路径换 `into_decoder()` 时,内建 512MB limits 守卫会静默丢失,须手动 reserve 复刻(A 批复核抓到) | docs/experience.md |
| F-035 | architect 对复杂新模块的施工轮次预估失真 ×2-3(预估 12-16,实际 33+34),复杂接线直接派高档 | docs/experience.md 或 orchestrate 记忆 |
| F-036 | 后端拉模式读 config 的 IPC,前端 setter 须先持久化后改本地 state,否则 watch 触发的请求与 set_config 竞速(色域切换偶发失效实证);同模式设置项通用陷阱 | docs/experience.md |
