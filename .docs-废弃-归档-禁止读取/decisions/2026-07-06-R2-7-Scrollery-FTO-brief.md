---
id: 2026-07-06-R2-7-Scrollery-FTO-brief
status: active
type: decision
line: 产品命名 R2-7(FTO 在途)
created: 2026-07-06
---

# R2-7 终选拍板与 Scrollery FTO 律师 brief(终稿)

> 拍板日期:**2026-07-06,用户终选 = 英文 Scrollery + 中文 画卷**(注册主体与传播名合一,接受一页纸 §2 列明的三笔获客税)。
> 本文档 = 委托三辖区(US/EU/CN)商标清权检索(FTO)的完整律师输入包;预筛证据链见
> [初筛与改名面盘点](2026-07-05-R2-7-产品名初筛与改名面盘点.md) §7.1/§7.4,决策对比见
> [终选一页纸](2026-07-05-R2-7-终选对比一页纸.md)(已盖拍板横幅)。
> ⚠ 预筛全部为 knockout 检索而非法律意见;本 brief 的价值 = 让律师不重复我们已做的粗筛,把预算花在官库深检与意见书上。

## 0. 拍板当天动作清单(用户侧,今天做)

| # | 动作 | 状态/证据(2026-07-06 实测复核) | 成本 |
|---|---|---|---|
| 1 | **注册 scrollery.app** | **仍未注册**(rdap.org 404 + Google DoH NXDOMAIN 双证)——scrolio(-11 天)/scrollary(-5 天)教训在前,今天就注 | ~$14/年 |
| 2 | 建 GitHub org **scrollery-app**(或 scrolleryhq) | 两变体均 404 可用;裸名 Scrollery 被东方 Project 闲置粉丝号占(2020 建号/2022 停更/9 repo,bio="Forbidden Scrollery"),拿到商标注册后可走 GitHub trademark policy 尝试收回裸名 | 免费 |
| 3 | 注册 npm org scope **@scrollery** | 裸包名 scrollery 被 2022 年 JS 库占(最后发版 2022-11),但 npm org scope 是独立命名空间,未来包一律发 `@scrollery/*` | 免费 |
| 4 | 可选:注册商面板顺手查并注册 scrollery.dev / scrollery.io / getscrollery.com | .dev/.io 未程序化确认(端点不可达),注册面板一看便知 | ~$12-35/年·个 |
| 5 | 可选:注册 scrolleria.com + .app 作 **FTO 红灯 fallback 保险** | 次选组合 Scrolleria/卷廊 双域 2026-07-05 实测皆空;FTO 数周窗口内 ~$30 买断退路 | ~$30/年,可弃 |
| 6 | crates.io **不预占**(政策禁 squatting) | 实测 404 空闲(带 UA 复核)——好消息:Rust 生态裸名可得,首个真实 crate 发布时自然取名 | — |

域名到手后挂邮箱转发/停放即可,FTO 期间不必建站。

## 1. 委托范围

- **辖区**:US(USPTO)/ EU(EUIPO)/ CN(CNIPA),三路并行。
- **标的一(三辖区)**:英文文字标 **SCROLLERY**(standard character / 纯文字)。
- **标的二(仅 CN)**:中文文字标 **画卷**;附「单独注册 vs 组合标」策略咨询(见 §4-CN)。
- **类别**:Nice **9 类**(可下载软件:照片/数字资产管理软件)+ **42 类**(SaaS/软件服务);CN 侧请细化到 0901/0907 与 4220 类似群组。
- **交付物**:clearance search + 注册可行性/侵权风险书面意见(green/yellow/red + 附条件)。
- **预算/周期预期**:$1,500-4,000,数周。

## 2. 产品事实(律师需要的最小上下文)

跨平台(Windows/macOS/iOS/Android)**本地优先照片与数字资产管理软件**;签名交互 = 手卷式(handscroll)横向长卷浏览。商业模式 = 免费 + 付费高级版(桌面直销 + 移动商店上架)。名称构词:scroll(卷轴/滚动)+ -ery(场所后缀,同 gallery/bakery)——单一熔铸造词,我方定性为 **suggestive(暗示性)而非 descriptive**,请律师确认。

## 3. 预筛已知事实(证据在案,勿重复粗筛)

1. **THE SCROLLERY**:US Reg. No. 3,407,995,**41 类**,死海古卷(Dead Sea Scrolls)学者团体持有,表观休眠;scrollery.com 即该学术站(DomainPeople 注册,在线)。这是全部索引里唯一的 scrollery 族注册。
2. **ZeniMax 标准字 SCROLLS**:ZeniMax Media(Microsoft 旗下)2024 年官方商标清单单列在权条目 "SCROLLS"(独立于 THE ELDER SCROLLS 家族);v. Mojang 案 2012 和解 = "Scrolls" 商标所有权全部转归 ZeniMax;执法史含对粉丝项目 Fortress Fallout 发 C&D、逼独立游戏 Prey for the Gods 改拼写。MyScrolls 候选即因此被我方 🔴 否决(初筛文档 §7.6);SCROLLERY 为熔铸造词、SCROLLS 不独立成词素,我方判断暴露面量级更低,请律师给正式距离评估。
3. **npm 同名开源库 ×2**:JS 滚动库 "scrollery"(最后发版 2022-11)及另一同名库——开发者生态拥挤是已接受的获客税;法律面请评估 US common-law 权利暴露(预期低:开源库非商业标识使用)。
4. **Scrollary 近似邻位**:scrollary.com + .app 于 2026-06-30 被同一注册人同秒注册(.com 购 3 年);GitHub 2026-03 出现同名新闻 PWA 项目在建。搜索引擎已把 scrollary 当 scrollery 误拼。
5. **Forbidden Scrollery**:日本东方 Project 官方漫画英文标题(2012-2017,KADOKAWA 出版)——完整性记录;JP 不在本次委托辖区,但 GitHub 裸名即被其粉丝号占用,文化圈词汇存在感需知悉。
6. **TMview 直查(2026-07-05)**:SCROLLERY 在 9/42 类零命中,全索引仅 THE SCROLLERY(41 类,US)一条。中等置信,须官库复核。
7. **CN「画卷」**:App Store 中国区在营精确同名同类目 app **「画卷 - 跨屏无缝拼图」**(Photo&Video 类,bundleId `com.picscroll.lazyva`,2026-06 仍在更新,体量小);裸词为高频双字通名,百科/素材库语料淹没级。

## 4. 律师问题清单(按辖区)

### US
1. THE SCROLLERY(Reg. 3407995,41 类)当前维持状态(Section 8/9 是否按期提交,是否已死/可撤销)?41 类学术服务 ↔ 我方 9/42 软件的混淆可能性评估;若存活,是否需要且可行取得 consent/coexistence?
2. ZeniMax 标准字 SCROLLS ↔ SCROLLERY(照片管理软件)的正式距离评估:熔铸造词是否足以出打击面?opposition 概率?有无可参照的 TTAB 先例?
3. npm 两个同名开源库的 common-law 权利暴露(我方预期低,请确认)。
4. Scrollary(他人 2026-06-30 起新启用,疑似建站中)的近似风险与申请日竞争——是否建议尽快提交 **intent-to-use** 申请抢占优先权?是否值得挂 watch service?
5. 整体 registrability:SCROLLERY 对「滚动/卷轴式浏览」软件有无 descriptiveness(2(e)(1))拒绝风险?我方定性 suggestive,请确认。

### EU
1. EUTM + 成员国库检索 SCROLLERY 及 SCROLL 词根近似族(9/42)。
2. 显著性:-ery 构词对英语区成员国消费者是否落 descriptive 边界(Art. 7(1)(b)/(c))?
3. ZeniMax SCROLLS 在 EU 的注册面与异议(opposition)风险。

### CN
1. **「画卷」9/42 类全检**(0901/0907/4220 类似群):高频双字通名在先注册/申请概率不低;若已被占,给出共存/撤三(连续三年不使用撤销)/变体策略。
2. App Store 在营「画卷-跨屏无缝拼图」:对方表观未注册商标——我方先注 9 类的可行性,与被指恶意抢注的反向风险(我方有真实使用意图,非囤标);以及若对方后续抢注,对我方 App Store 上架名的投诉风险。
3. SCROLLERY 英文标 CN 注册 + 音译近似检索(无固定中文音译,预期干净,须查)。
4. **注册结构策略**:「画卷」单独注册 vs 「Scrollery画卷」组合标 vs 图形+文字组合?若「画卷」不可注册,其作为传播语/副标的描述性使用法律边界如何把握?

## 5. English brief(可直接转发 US/EU 律师)

> **Subject: Trademark clearance request — "SCROLLERY" (standard-character word mark), Intl. Classes 9 & 42**
>
> We are preparing to launch **Scrollery**, a cross-platform (Windows/macOS/iOS/Android), local-first photo and digital-asset management application whose signature interaction is handscroll-style horizontal browsing. Business model: free tier + paid premium (direct desktop sales + mobile app stores). The name is a unitary coined word (scroll + -ery, as in gallery/bakery), which we regard as suggestive rather than descriptive.
>
> We request a full clearance search and a written registrability/infringement-risk opinion in [the US / the EU], Classes 9 and 42.
>
> Findings from our preliminary knockout screening (not legal advice; please verify against official records):
> 1. **THE SCROLLERY** — US Reg. No. 3,407,995, Class 41, held by a Dead Sea Scrolls research group (scrollery.com); appears dormant. Please confirm maintenance status and assess likelihood of confusion across Class 41 vs. 9/42.
> 2. **ZeniMax Media (Microsoft)** lists the standard-character mark **"SCROLLS"** in its official trademark list (effective January 2024) and has a documented enforcement history (the 2012 ZeniMax v. Mojang settlement transferred the "Scrolls" mark to ZeniMax; cease-and-desists against small independent projects). Please assess the distance between the unitary coinage SCROLLERY (photo software) and SCROLLS (game software), and the realistic opposition risk.
> 3. Two open-source JavaScript libraries named "scrollery" exist on npm (latest activity Nov 2022). Please assess common-law exposure (US).
> 4. **"Scrollary"** (typo-adjacent): scrollary.com/.app were registered on 2026-06-30 by a single registrant, and a "Scrollary" news PWA appeared on GitHub in March 2026. Please advise on typo-proximity risk, filing urgency (intent-to-use), and whether a watch service is warranted.
> 5. "Forbidden Scrollery" is the English title of a Japanese Touhou Project manga (2012-2017) — noted for completeness; Japan is out of scope for this engagement.
> 6. Our TMview sweep (2026-07-05) found **zero SCROLLERY marks in Classes 9/42** across indexed registers; THE SCROLLERY (Class 41, US) was the only family hit. Medium confidence — please verify on official systems.
>
> Deliverable: clearance opinion (green/yellow/red with conditions). Timeline: weeks, running in parallel with EU/CN counsel.

## 6. 拍板后流程状态(接一页纸 §3)

| 步骤 | 状态 |
|---|---|
| 终选拍板:Scrollery / 画卷 | ✅ 2026-07-06 |
| §0 占位六项(域名/org/@scrollery) | ⏳ 用户侧,今天 |
| 持本 brief 委托三辖区 FTO | ⏳ 用户侧(渠道建议:US 找 boutique 商标所或信誉 flat-fee 服务;EU 找 EUIPO 执业代理;CN 找有软件类经验的商标代理机构) |
| FTO 期间:改名施工计划草案 + D10 门禁设计(我出,施工不动手) | ⏳ 可先行 |
| FTO 绿 → 商标申请(US intent-to-use / EUTM / CN 9+42 双类)→ 改名施工(identifier 锁定 `com.scrollery.app` + KEYRING_SERVICE 迁移 + D10 门禁 CI + 全仓替换,约一个工作日) | 待 FTO |
| FTO 红 → 退回一页纸选下一个(次选 Scrolleria/卷廊;§0-5 保险若已买,退路零摩擦) | 兜底 |

