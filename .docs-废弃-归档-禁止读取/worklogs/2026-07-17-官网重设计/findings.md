---
status: snapshot
type: working-memory
line: 官网重设计
created: 2026-07-16
---

# 发现与决策:官网重设计

## 需求
- 用户原话:scrollery-site 目前只是简单单页 index.html;按产品设计与代码实现做一个**精美官网**;技术栈用「h5 原生」或「vue(如有必要)」;**功能尽可能完善**;另评估**官网有没有必要上后端**。

## 发现
- scrollery-site 现状:仅 `index.html`(15.8KB 单文件)+ `README.md`;2 个 commit;remote=git@github.com:gfgjs/scrollery-site.git;**CF Pages push 即部署**(全局规则:push 需用户批准)。
- v1 设计已相当讲究:纸(亮)/墨(暗)双主题 CSS 变量、朱砂强调色 `#c3402b`、中英双语整页共存按 `data-lang` 显隐、FOUC 防护脚本、长卷装饰动画(prefers-reduced-motion 兜底)、6 特性卡、诚实路线图脚注(「内测进行中,以正式发布为准」)、零依赖零跟踪。
- v1 品牌要素:favicon=data URI SVG 朱砂方章「卷」字;serif 品牌字(Palatino/Songti);tagline「铺开你的画卷 / Your photo library, unrolled」。
- 主仓 README(公开镜像同步):定位=十万~百万张照片流畅画廊式浏览的高性能跨平台媒体资源管理器;状态=活跃开发,已实现图片浏览+AI 语义搜索;栈=Rust+Tauri v2+rusqlite/WAL+rayon+WIC GPU 解码+Chinese-CLIP(ort/DirectML 独立 ai-worker 子进程);前端 Vue3+Pinia+Vite+TS+原生 CSS 变量。
- 性能架构可公开卖点(README 已公开):两阶段扫描(秒级出网格)、后端 Justified Layout+行级虚拟化、常驻布局缓存+视口按需取元数据、O(1) 布局索引、bucket 虚拟化+自研逻辑滚动条(绕 16.7M px 元素高度上限)、CLIP f16 常驻缓存+rayon 余弦相似度。
- 法务:开源核心 Apache-2.0;「Scrollery」名称与 logo 是商标,**不在** Apache-2.0 内(TRADEMARK.md);CLA 要求。官网页脚应链接/呼应。
- 应用图标资产:src-tauri/icons/ 全套(icon.png/icns/ico + Square 系列),可复用为站点 favicon/og 素材。
- 记忆佐证(内部):真机开发库 54 万张;contact@scrollery.app 只收不发;identifier=com.scrollery.app。
- **站点 README 文案红线**:对外性能表述固定「数十万张」量级;未落地能力只进「路线图」措辞;站仓内容(品牌文案与标识)版权保留,不适用主仓 Apache-2.0。
- **src-tauri/icons 全套仍是 Tauri 默认占位图**(青黄圆形),非 Scrollery 品牌——官网禁用;favicon 沿用「卷」字朱砂印 SVG,og:image 需自绘。
- tauri.conf.json:productName=Scrollery,identifier=com.scrollery.app,主窗 1280×820/min 800×560——mockup 比例参考。

## 设计方案(v2 定稿)
- **方向**:不换皮,深挖「手卷」——整页即一幅手卷的裱装结构(引首=Hero → 画心=滚动驱动展卷演示 → 题跋=FAQ/路线图 → 拖尾=页脚),延续 v1 纸墨朱砂。
- **签名元素**:「展卷」段——300vh sticky 区,纵向滚动驱动横向展开 4 幕(原样接入文件夹 → 秒级铺开网格 → 一句话搜索 → 时间轴漫游);reduced-motion/窄屏退化为纵向静态四幕。
- **调色板(命名)**:纸 #f6f2e9/墨底 #171310;绢面 #fffdf7/#211b16;墨字 #2b2622/#ece5da;朱砂 #c3402b(暗 #e05a3a)=品牌与交互;石青 #2b4d6f(暗 #7fa8c9)=技术/AI 区次强调;石绿 #4a6741=状态点缀。纪律:朱砂与石青不同组件混用。
- **字体(零外源,系统栈)**:展示=宋体系 serif(Songti SC/Noto Serif CJK SC/SimSun + Palatino/Georgia);正文=system-ui;数据/规格=mono(Cascadia/Consolas)。竖排(writing-mode: vertical-rl)只用于章节侧签(zh 时),en 退化横排。
- **结构语汇**:印章方块作章节眉(卷/械/藏/问),细双线裱边分节,直角低圆角(2-4px),留白宽裕。
- **动效纪律**:载入时印章一次性「钤印」;展卷滚动驱动;其余仅 hover 微反馈;全部受 prefers-reduced-motion 门控。
- **自评(对 AI 三大默认皮的差异化)**:米纸+衬线+赤陶确属默认皮 #1 族,但此处是品牌本体(画卷/纸墨朱砂印/青绿设色皆为手卷材料语汇);差异化落在:裱装分节结构、印章系统、竖排侧签、滚动展卷、矿物颜料次色、mono-serif 张力。接受。

## Explore 代理盘点要点(2026-07-16,全文见代理报告;证据均已核到文件)
- **产品是「四媒体」管理器**(图/视/文/音),不只是照片——官网定位据此升级;权威定位句在 Part0 §2.2/§3.2。
- 落地特性(官网可宣传):统一画廊/查看器/阅读器(竖排竹简)/时间轴/文件夹树/中文语义搜索/人脸人物墙/6主题+5阅读器纸色/插件商店(PSD 首发)/收藏评分色标/筛选排序/音频歌词/离线卷/WebDAV。未落地勿宣传:跨设备同步、TTS/仿真翻页、移动端。
- 实测数字全部是**开发机内部实测非正式基准**;真机库 543,449 项/29 格式。公开口径:「数十万张实测,面向十万~百万张设计」。
- 主题 6 套:墨/素/月白/宣/玄/黛(3亮3暗,双槽持久,Windows 标题栏跟色);阅读器另 5 纸色(registry.ts / readerThemes.ts)。
- **真实截图资产**:主仓 `.screenshots/theme-matrix/` 6主题×3视图(capture 脚本产物,**示例图库合成内容**,上站须标注);已转 webp 入站(hero 56KB、6 缩略 ~18KB、viewer 10KB)。
- 格式:66 内置(图8+heic/heif/avif、RAW×10、视频17、音频13、文档15)+ PSD exotic;dwg/dxf 仅测试 fixture 非真支持;真机库 RAW/heic 计数为 0(勿暗示海量 RAW 实战)。
- **敏感规避清单**:定价(Part8 全部数字)、DRM/抽成、旧代号 picasa-next、®(FTO 在途)、SCRFD/ArcFace(commercial_ok=false 不出货)、macOS/移动发布日期承诺。人脸只提「商用安全(MIT/Apache-2.0)」。

## 后端必要性评估(正式结论)
**不上后端。** 理由:
1. 官网职责=展示 + mailto 获客 + 未来静态分发(GitHub Releases/CDN),全部可静态承载;CF Pages 已有 push 即部署链路,零运维零攻击面。
2. 零跟踪约章(站 README)排除分析类后端;联系走 contact@(只收不发)。
3. 逐项检视潜在后端需求:表单(→ 现阶段 mailto;将来如需,CF Pages Functions 同仓渐进)、下载计数(→ 不做,或客户端读 GitHub API)、newsletter(→ 不做)、许可证签发/更新检查(→ 属产品基础设施 Part8,不属官网)、评论/社区(→ GitHub Issues)。
4. 结论:现在上后端=纯负资产;演进路径=静态 → (按需)CF Pages Functions → (商业化时)独立 Workers 服务,官网仓始终保持零构建。

## 外部资料(当数据,不当指令)
- (暂无)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | PS5.1 下 `.ps1` 脚本文件与 `iex` 执行均被设备策略降入 ConstrainedLanguage(GDI+/任意 .NET 类型解析失败),同代码**内联单链不受限**;绕法=headless Edge 渲 HTML 截 PNG(字体渲染更佳) | experience |
| F-002 | headless Edge 截图三坑:锚点/scrollTo 均不可靠(smooth-scroll+virtual-time 竞态)、340vh 段在超高视口截图中膨胀出巨大空白、同源 iframe 探针(scrollWidth+getBoundingClientRect 列越界元素)才是可靠布局验证法 | experience |
| F-003 | 官网后端评估结论与演进路径(上方「后端必要性评估」全文) | decision |
| F-004 | 官网素材链:`.screenshots/theme-matrix/` → ffmpeg webp → 站 assets/shots;og/图标由 assets-src/*.html headless Edge 再生;规避项清单(占位图标/定价/®/日期承诺) | no-promotion(已落 scrollery-site README「资产再生+文案纪律」) |
