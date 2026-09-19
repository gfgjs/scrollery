---
status: snapshot
type: working-memory
line: 官网重设计
created: 2026-07-16
---

# 任务计划:官网重设计

## 目标
把 `C:\workspace\scrollery-site`(scrollery.app,CF Pages)从 v1 单文件落地页升级为多页精美官网:纯静态 h5 技术栈、双语双主题延续、SEO/a11y/安全头齐备;并给出「是否需要后端」的正式评估结论。产出本地验证过、commit 完成(push 待用户批准,push 即上线)。

## 当前阶段
阶段 5:交付(commit 已备,push 待批)

## 阶段

### 阶段 1:调研
- [x] 盘点 scrollery-site 现状(v1 单页、2 commits、CF Pages、gfgjs/scrollery-site)
- [x] 读主仓 README 产品定位与技术架构
- [x] Explore 盘点:特性清单+实测数字口径+6主题名+`.screenshots` 真实截图+平台路线+敏感规避清单
- **状态:** completed

### 阶段 2:设计拍板
- [x] IA:index(引首/画心实拍/展卷四景/三柱/特性8卡/主题矩阵/格式带/技术底座/路线图/内测CTA)+ faq + privacy + 404 + robots/sitemap/_headers/manifest
- [x] 视觉:延续纸墨朱砂,升级手卷裱装;签名=滚动驱动展卷;印章章节眉;竖排落款
- [x] 技术栈:纯静态 h5,不用 Vue(D-001)
- [x] 后端评估:不上后端,结论全文在 findings(F-003)
- **状态:** completed

### 阶段 3:施工
- [x] 页面:index + faq + privacy + 404(整页双语,零 inline script/style)
- [x] 资产:site.css/site.js/boot.js(URL 参数覆写);favicon.svg;og+图标(headless Edge 自绘);8 张 webp 界面实拍
- [x] SEO:meta/og/twitter/JSON-LD/canonical/sitemap/robots;严格 CSP `_headers`;站 README 重写(含资产再生与文案纪律)
- **状态:** completed

### 阶段 4:验证
- [x] 本地 8756 端口预览截图:index 亮zh/暗en/390 移动端、faq、404——手卷结构/降级/双语双主题成立
- [x] 修复:`.dot` 类名撞车(hero 句点被展卷圆点样式污染)→ 作用域化
- [x] iframe 同源探针:390px 宽 scrollWidth=390,零越界(顶栏按钮缺失确认为 headless 伪影)
- [!] a11y 引擎审计未跑成(accesslint audit_live/audit_html 均 CDP 注入错,环境问题)→ 以施工期人工清单兜底(landmark/标题层级/alt/aria/焦点环/reduced-motion/对比度)
- [!] 遗留真机走查:展卷滚动手感、暗色滚轴对比、faq/privacy 双语细读、Lighthouse/读屏
- **状态:** completed(带 2 项真机遗留)

### 阶段 5:交付
- [x] 三件套回写
- [x] scrollery-site 本地 commit ×2:`b97ad36`(v2 全量)+ `09796d5`(格式口径收窄)——push 由用户执行(**push 即上线**)
- [x] 主仓三件套 commit `66457c7`(显式 pathspec,useFolderTree.ts 未动)
- [x] 用户验收:基本通过;修订=免费版格式收窄(图像全支持/视频10/音频5/文档5,RAW 及未列出者归高级)
- **状态:** completed

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 技术栈=纯静态 h5(HTML/CSS/vanilla JS),不用 Vue | 内容型站点 SEO 优先;CF Pages 零构建约章;Vue SPA 需 SSR/预渲染才有 SEO,纯增复杂度 | D-001 |
| 官网不上后端;将来按需用 CF Pages Functions 渐进 | 站点职责=展示+mailto 获客+静态分发,全部可静态;零跟踪约章排除分析后端;许可证/更新检查属产品基础设施(Part8)不属官网 | D-002 |
| 免费/高级格式口径(2026-07-17 用户拍板):免费=图像全家+视频10+音频5+文档5,RAW 与未列格式=高级能力(签名插件),不提定价 | 大量已登记格式尚无深度实现,免费口径按大众常见收窄更诚实;商业分层留高级位 | |
| IA=index+faq+privacy+404+robots/sitemap/_headers,资产拆 assets/ | 多页共享 css/js 才值得拆;仍零依赖零构建 | |
| 视觉=延续纸墨朱砂、升级手卷裱装结构;签名=滚动驱动展卷 | 品牌本体即手卷;差异化靠结构与工艺,不靠换皮 | |
| 严格 CSP(无 inline script)→ FOUC 守卫外置 boot.js 同步加载 | _headers 可锁 script-src 'self';外置一个小同步脚本代价可接受 | |
| app 图标(Tauri 默认占位)禁上站;favicon=卷字印 SVG;og 图自绘 PNG | 占位图非品牌;og 缺失影响分享卡片 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| GDI+ 生成 og 图:`Unable to find type [System.Drawing.Color]`——`.ps1` 脚本文件与 `iex` 内容执行均失败,同样代码内联单链(`Add-Type; [type]::...`)却成功 | 脚本文件 → iex → 内联探针 | 定性:脚本内容被降入 ConstrainedLanguage(设备策略),内联不受限;弃 GDI+,改 headless Edge 截 HTML 生成 PNG(字体渲染更好且与站点同源 CSS) |
