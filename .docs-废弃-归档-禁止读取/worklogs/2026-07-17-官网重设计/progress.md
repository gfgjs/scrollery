---
status: snapshot
type: working-memory
line: 官网重设计
created: 2026-07-16
---

# 进度日志:官网重设计

## 会话:2026-07-16 → 07-17(跨零点连续施工)
- 做了:全线一次交付——调研(两仓+Explore 特性/数字/敏感清单)、设计拍板(手卷裱装 IA+纯静态 h5+不上后端)、施工(index/faq/privacy/404 四页整页双语 + site.css/js/boot.js + og/图标自绘 + 8 张真实截图 webp + robots/sitemap/_headers/manifest + 站 README 重写)、验证(本地 8756 预览截图×6 配置 + iframe 布局探针 + 修 `.dot` 撞名)。
- 验证证据:
  - 截图:scratchpad/preview/*.png(index 亮zh/暗en/移动端390、faq、404、mobile-header、probe)。
  - 布局探针:iframe 390px 下 documentElement.scrollWidth=390、零越界元素。
  - 资产重量:webp 8 张共 ~168KB(hero 56KB);全站无外源请求(CSP `default-src 'none'` 兜底)。
- 工具环境三坑(已记错误账/候选):PS 脚本降权 ConstrainedLanguage、headless Edge 锚点截图不可靠、accesslint 两模式 CDP 注入坏。
- 遗留:①site+主仓两 commit(见阶段5);②push 待批(**push=上线**);③真机走查:展卷手感/暗色滚轴/读屏/Lighthouse;④本地预览服务还在 8756 端口跑着。

## 会话:2026-07-17(验收修订 + 收口)
- 做了:用户验收基本通过;按拍板收窄免费版格式口径(index 格式带重构:免费四组+「高级」行;FAQ 格式答同步;站 README 固化口径)→ 站仓 commit `09796d5`。随后走收口仪式:候选 F-001..F-004/D-001/D-002 逐项处置(experience §20/§21 + decisions 官网拍板件),closeout.md 入库,迁 worklogs。
- 验证:被砍格式全文检索无残留(阅读器卡 txt/md/epub/pdf 均在免费列);check_docs 门禁见收口 commit。
- 遗留:两仓 push(用户侧);真机走查项见阶段 4。

## 回顾(收口时填)
- 亮点:签名段「滚动驱动展卷」+印章系统把品牌意象做进了结构;真实截图(标注示例图库)比 CSS 假 mockup 更有说服力;boot.js 的 `?theme=/&lang=` 参数覆写同时服务了 headless 验证与英文深链。
- 教训:`.dot` 撞名=skill 警告过的选择器互斗,组件级类名必须带前缀/作用域;headless 截图证据链要用 DOM 测量兜底,像素图会说谎(按钮缺失伪影)。
- 意外:audit_live/audit_html 双双 CDP 坏;340vh 段在超高视口截图里产生视觉巨洞(vh 语义),差点误判为布局 bug;免费版格式口径超出仓内证据范畴,最终由用户按商业分层拍板收窄——「已登记 ≠ 该宣传」。
