---
id: 2026-07-13-realtest-round8-2026-07-13-视图路由化与URL同步
title: 第八轮真机验收清单（S2-c 视图路由化 + S2-b2 view-pref/search URL 同步）
status: active
type: acceptance
created: 2026-07-13
acceptance: 待真机验收
snapshot_date: 2026-07-13
scope: S2-c 视图路由化（smart-album + folder 筛选，会话续23）· S2-b2 view-pref + search URL 同步（Stage 3a/3b）
commits: afa10e1(S2-c), 724dcf2(3a view-pref), 3b(search)
---

# 第八轮真机验收清单（视图路由化 + URL 同步）

> 承 S1 阶段7(UiToolbar 决策收口=不建组件,SelectionActions 加 role/可访问名)。本轮两大块:
> **① S2-c 视图路由化**——smart-album（5 档）与 folder 筛选态各占独立路径,深链/刷新可恢复;folder 滚动
> 锚点态(模式A)不进 URL。**② S2-b2 URL 同步**——group/sort/order/layout(view-pref)+ q/scope/mode(search)
> 进 URL,深链恢复、URL 覆盖 persist、语义搜索不污染。
> 逻辑已过本地门禁(typecheck/lint/vitest 839/build)。以下为**门禁盲区**——路由导航/深链恢复/KeepAlive/
> 语义 IPC 恢复/persist 竞态均 SSR/单测不可验,须真机。起 `npm run tauri dev`。

## A. S2-c 视图路由化（smart-album + folder 筛选）

- [ ] **smart-album 各占独立路径**:侧栏点「收藏/实况/最近/回收站/全部照片」→ 地址栏分别变
      `/favorites` `/live-photos` `/recent` `/trash` `/`(hash 路由下形如 `#/favorites`);画廊内容随之切换。
- [ ] **深链恢复 smart-album**:直接把地址改成 `#/favorites` 回车(或刷新页面)→ 进入收藏视图,侧栏高亮收藏。
      `#/live-photos`、`#/recent`、`#/trash` 同理。
- [ ] **folder 筛选态(模式B,非 folder 分组)→ /folder/:id**:分组设为「日期」或「无」时,侧栏点某文件夹 →
      地址栏变 `#/folder/<数字id>`,画廊筛成仅该目录;刷新后仍在该目录(深链恢复)。
- [ ] **folder 滚动锚点态(模式A,folder 分组)不进 URL**:分组设为「文件夹」时,侧栏点不同文件夹 → 地址栏
      **保持 `#/`**(不变成 /folder/...),画廊平滑滚动到该文件夹段(锚点滚动,非筛选)。这是有意的——滚动位置
      不该进 history。
- [ ] **新增扫描根 / 移动文件夹后自动选中**:新增一个扫描根目录,或移动一个文件夹 → 自动选中该目录时地址栏
      应变 `#/folder/<id>`(而非停在 `#/` 被回填成「全部」而丢选中)。**这是本轮修的 clobber 点,重点验。**
- [ ] **前进/后退**:在 全部→收藏→某文件夹 间点几次,浏览器/应用的后退能逐步回到上一个视图,视图与地址一致。
- [ ] **KeepAlive/滚动不回归**:全部照片滚到中间 → 切收藏 → 切回全部 → 滚动位置恢复、不重新加载闪白
      (多路径复用同一 MediaGrid 实例,应无重挂)。
- [ ] **切视图不误清选区之外的**:在某视图多选几张 → 切到另一视图 → 选区清空(既有契约,正常);但**同一视图内
      改筛选/排序不应清选区**(见 C 节 App.vue 只监听 path 段)。

## B. S2-b2 · view-pref（分组/排序/方向/布局）URL 同步

- [ ] **改 view-pref 即写 URL**:工具栏改「分组=文件夹 / 组内排序=文件名 / 排序方向 asc / 布局=宫格」→ 地址栏
      query 出现 `?group=folder` `sort=filename` `order=asc` `layout=grid`(仅非默认值出现;全默认时 URL 干净无这些键)。
- [ ] **深链恢复 view-pref**:把地址改成 `#/?group=folder&sort=filename&layout=grid` 回车/刷新 → 画廊按该分组/排序/
      布局呈现。
- [ ] **URL 覆盖 persist**:先在设置里把默认分组设为「日期」(持久化)→ 用带 `?group=none` 的 URL 深链进入 →
      画廊显示「无分组」(URL 权威覆盖了持久的「日期」);去掉 URL 的 group 键刷新 → 回落持久的「日期」。
- [ ] **order 跨刷新恢复**:切换排序方向 asc/desc(工具栏方向钮)→ URL 出现/移除 `order=asc` → 刷新后方向保持
      (sortOrder 无持久化,只有 URL 能跨刷新恢复它)。
- [ ] **语义搜索不污染 view-pref URL**:进入语义搜索(见 C)→ 此时 groupBy 被临时改「无分组」,但**地址栏不应出现
      `group=none`**(临时态不写 URL);退出语义搜索 → 分组复位、URL 恢复真实值。**这是防 previousGroupBy 污染的
      关键,重点验:语义搜索开→关→刷新,分组应回到语义前的值,不卡在「无分组」。**

## C. S2-b2 · search（查询/范围/模式）URL 同步 ⚠ 高风险重点

- [ ] **提交搜索即写 URL**:普通/文件名搜索输入并提交 → 地址栏出现 `?q=<词>`(及非默认 `scope=`/`mode=`);
      清空搜索 → 相关键从 URL 移除。
- [ ] **深链恢复普通搜索**:`#/?q=IMG&mode=normal` 刷新/深链 → 恢复为文件名搜索「IMG」,结果正确。
- [ ] **深链恢复语义搜索(触发 IPC)**:`#/?q=猫&mode=semantic` 深链/刷新 → **恢复时真实执行语义搜索**(不是只改
      地址不查询),画廊出语义结果。**这是恢复触发 IPC 的路径,重点验:AI worker 未就绪时不崩、有 loading、
      最终出结果或明确错误(不静默失败)。**
- [ ] **scope 恢复**:文件名搜索把范围切到「目录/日期/设备/地理位置/全局」→ URL 出现 `scope=folder` 等 → 刷新恢复该范围。
- [ ] **mode 恢复的副作用正确**:`#/?q=x&mode=semantic` 深链进入(语义会把分组临时改「无」)→ 退出语义 →
      分组复位到深链时 URL 里的值(或持久默认),**不卡在「无分组」**(previousGroupBy 捕获正确)。
- [ ] **恢复顺序**:带 `?group=folder&q=y&mode=normal` 的复合 URL → 恢复后既是文件名搜索「y」又按文件夹分组,
      两者共存不互相冲掉。

## D. 组合 / 回归

- [ ] **filter + view-pref + search 三者共存**:同一 URL 带 `?favorite=1&group=folder&q=z&mode=normal` → 收藏筛选
      + 文件夹分组 + 文件名搜索「z」全部生效,刷新后全部恢复,互不冲掉。
- [ ] **URL 干净性**:回到默认(无筛选/默认分组排序布局/无搜索)→ 地址栏应为干净的 `#/`(或 `#/favorites` 等纯路径),
      **无残留空 query 键**。
- [ ] **collection/person 详情不受影响**:进 `/collections/:id`、`/persons/:id` → view-pref/search 仍可用,
      但语义搜索面板(SemanticSearchPanel)行为与拆分前一致(只在主库血统视图显示;collection/person 详情此前即不显示)。
- [ ] **无控制台报错/无死循环**:全程 Console 无 vue-router 重复导航告警、无 URL 抖动(反复 replace)、无 Duplicate keys。

## E. 【需你判定】view-pref 语义:全局携带 vs 纯 per-view

> **本轮实现的是「全局携带 + URL 恢复/覆盖持久」**:group/sort/order/layout 像 filter 一样是全局态——在视图 A
> 改分组,切到视图 B 也是同一分组(全局),但每个视图的 URL 都带着当前值、深链可恢复、URL 覆盖 persist。
> **未实现「纯 per-view 逐视图独立」**(视图 A 记文件夹分组、视图 B 记日期分组、互不影响)——那需要一套
> read-on-navigation 模型 + 工具栏改动不落全局持久化(改 toolbar 持久语义),属 UX 契约变更。

- [ ] **体验后判定**:用下来觉得「全局携带」够用,还是想要「每个相册/文件夹各记各的分组排序」(纯 per-view)?
      若要纯 per-view,是一个单独的后续改动(会改工具栏是否持久化的语义),请在此记录你的裁决。

## 结论

- [ ] 全部通过 → 视图路由化 + URL 同步交互契约在真机确立。
- [ ] 若有问题:记录症状 + 复现步骤(尤其地址栏 query 实际值 + 分组是否卡在「无」),回写本清单对应项 + findings。
