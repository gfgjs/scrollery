---
id: 2026-07-17-status-DOM-膨胀治理
status: active
type: rolling-status
line: DOM-膨胀治理
created: 2026-07-17
---

# DOM-膨胀治理 · 滚动状态

> 状态板 2026-08-24 自 docs/todo.md「文件树与时间轴 DOM 膨胀治理」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## 权威文档
> 独立复核报告 + 六路竞争式评审裁决见 [2026-07-09-文件树与时间轴DOM膨胀-独立复核报告v2.md](../designs/2026-07-09-文件树与时间轴DOM膨胀-独立复核报告v2.md) §9(**当前权威**)。旧案已废弃(可参考不可信),2026-07-09 移入 [archive/…治理方案.md](../archive/2026-07-09-文件树与时间轴DOM膨胀治理方案.md)。
> 权威顺序:**T0(廉价先手)→ 量化网格节点 → T1+T2(文件树)→ [二期,实测驱动] canvas / loupe / 键盘 slider**。
> **2026-07-13 Canvas 热路径优化**(`a5d8e99`):可见行/指针命中线性扫描改两级二分 + 缩略图裁剪异常路径释放中间 `ImageBitmap` + 右侧日期/文件夹轴离屏静态层缓存(O(1) 绘制 active/hover/指示线)+ 拖拽 pointermove 合帧;验证:Vitest 843 测全绿 + browser harness 双 Canvas/切分组/滚动/hover 通过(交付史详见 **▸ 详注 O-Canvas热路径**)。**仍保留**下表真机大库 perfProbe 与 DOM 并排基准待办,未以 18 项 harness 代替性能定案。
> **2026-07-13 全局低扰动性能面板**(`81cef21`):App 内 Ctrl+Shift+P 或设置→开发者工具可全局打开;静默录制,支持帧预算/P95/P99/错 VSync/LoAF 指标/Canvas 指标/JSON 导出+固定速度三趟画廊往返基准;Vitest 855 全绿(交付史详见 **▸ 详注 O-性能面板**);应用内浏览器自动验收因本机 ACL 沙箱故障未取得证据,仍需用户真机终证。

## 状态板
| 状态 | 待办 | 现状结论 / 阻塞源 |
|---|---|---|
| ✅ | **T0** 时间轴 folder 模式点轨吸附最近 separator + 放开 hover 分组名浮层 | **2026-07-09 交付(本会话)**,⏸GUI(folder 点轨落点吸附、hover 显分组名;张数未显 defer)→ **▸ 详注 O-T0** |
| ✅ | 量化「全屏 + 极小缩略图」MediaGrid 常驻节点(devtools 实数) | 实测 3556 cell→网格 7294 节点(合法地板)+ 文件夹轴 2042(最大可削废浪费);结论=canvas 化另立项 → **▸ 详注 O-量化** |
| 🔨 | **「另立项」= 画廊极密网格 canvas/优化专项**(承接上一行 §9.5「削不动除非 canvas=另立项」裁定) | **🔨 G-T0…G-T4 canvas 专项开题即交付 ⏸GUI(2026-07-09→10)**:方案 [极密网格渲染方案](../designs/2026-07-09-画廊极密网格渲染方案.md) + perfProbe 探针;G-T0 选择态零重渲(`caf9dd2`)/ G-T1+T2 useThumbLoader 单源 + 轻量卡片(`ea2da48`)/ **G-T4 canvas 转正**(`83a4e17` → 🔴 v2 mirror-removal 零逐格 DOM:hitTestCell 算术命中 + useSelection 注入解析器)。**✅ 滚动飞掠卡顿子线收口(真机通过)**:低保真加载闸门 useThumbLoadGate(`0a39e31`+`da43dfa`+`8078d7c`)。**🔨 canvas 2D 续挖**:alpha:false(`34c9be6`)+ 解码期预缩放 createImageBitmap(`f40251f`);**✅ 深审 F1-F4 回补**(`8d69257`/`bdd83b1`/`cee653b`/`ea9c8f6`)。vitest 525(仅本地)。**⬜ 余**:真机探针对比(canvas vs DOM 的 FPS+selectEnterMs)定 canvas 长期去留 + G-T3(decoding=async)。详注见 [completed.md](../completed.md) ▸ O-canvas |
| 🔨 | **画廊 Canvas 功能对齐 DOM 线 + 滚动加载策略线(2026-07-10 立项即交付)** | **🔨 Canvas 功能面对齐 DOM 四阶段 + 滚动加载策略交付 ⏸GUI(2026-07-10)**:悬停跟踪/选中动画(`0cb79b7`)+ 全格常显徽章信息浮窗(`35ba0e8`)+ **单例悬停卡**(`7f6fc25`,mgc-hover-layer 挂整只 MediaThumb,O(1) 节点)+ 33 测(`873ee30`);真机迭代移入闪烁三层修(`12b2373`)。滚动加载:视口外预取(`3a82a73`+`68e23e8`)+ 闸门阈值随行高缩放(`29614a5`)+ 拖拽关闸(`ae02b9d`)。**🔴 未解**:拖滑块「一闪一闪」根因不在本线四项(`d532140` 回退仍闪),嫌疑=生成期 media_enriched 2s 节流整页重排 / 55k 库生成风暴,**用户将 fork 更早代码分离变量,悬置待结论**。vitest 593(仅本地)。详注见 [completed.md](../completed.md) ▸ O-parity |
| ✅ | **60px Canvas 数千格加载驻留与任务风暴收口(2026-07-17)** | `384e10e`;真机终验:已加载区域接近无感,极速无顿挫;480px 冷区无改善转 N 线多档源 → **▸ 详注 O-60px** |
| ✅ | **Canvas 快速拖动防闪烁 + 稍快速换图波根治(2026-07-17)** | `ec2bbff` 四层演进;headless A/B 冷区帧 4911→~530(−89%);4K+全屏+64px 极速域残留(边际收益不高记档后期)由 N 线多档源行承接 → **▸ 详注 O-防闪烁** |
| ✅ | **T1** 文件树虚拟化(方案 B 共享滚动,a/b/c 全交付) | **✅ T1-a/b/c 全交付,T1-a GUI 验收通过(2026-07-09)**(逐项交付史详见 ▸ 详注);**⏸GUI**:万级目录 DOM 恒定 / 快滚不空白 / 钉顶 / 拖拽手感 → **▸ 详注 O-T1** |
| ✅ | **T2** 单目录文件分页 + `nodes` 改 shallowRef(**百万级必做**) | **✅ 2026-07-09 交付**(`93e9f13`):`nodes` 深 ref→shallowRef(修大数组红线,原地 mutate 补 triggerRef)+ 单目录文件分页(FILE_PAGE=200,后端 limit/offset + 前端 MoreRow「加载更多」,+3 测);cargo/vue-tsc/eslint/vitest 405/build 全绿(仅本地);大目录分页手感⏸GUI → **▸ 详注 O-T2** |
| ✅ | **缺陷修复:并发 `loadChildren` 造成重复目录行**(Vue "Duplicate keys found during update: dXXXX" 控制台刷屏,用户 2026-07-09 截图报症) | **✅ 2026-07-09 修复**(`6373f90`):root 因=groupBy=folder 时两 watch 并发 `expandToNode→loadChildren` 对同一未展开祖先各 splice 相同子节点(T1-c + T2 shallowRef 手动 mutate 放大竞态窗口);修法=loadChildren 双防线(在途去重 Map<parentId,Promise> 等待 + splice 前 id 集合过滤)+ 回归网 3 测;⏸GUI 待终证 → **▸ 详注 O-dup** |
| ✅ | **缺陷修复:大库拖动 slider 时文件树跳到靠后的同名目录** | `c2536e6`;⏸GUI:40 万图库拖动滑块终证 → **▸ 详注 O-slider** |
| 🔨 | **filename 排序卡顿根治 = B-file-iii 全局内存 rank(2026-07-15,三阶段 landed)** — 用户真机报「顶栏点筛选(视频/收藏/星级)、切分组 dev>5s/release>1s」;根因隔离实证=`NATURAL_CMP` collation 的 SQLite→Rust FFI(占 filename MISS 93% dev/65% release,非缺索引/非 SQL 结构) | **🔨 三阶段 landed ⏸GUI(2026-07-15)**:D-018 施工前核实反转(sort_datetime=EXIF 墙钟当 UTC 存 → UTC 桶正确、SQL `localtime` 是双重时区 bug)→ 用户改选 **A′ 全面 UTC 桶**。①`d928271` date+filename UTC 内存派生(`div_euclid` 日桶,零新列)根治换轴 thrash;②`55bd4a4` 全局 filter-invariant rank 基建(开机后台预建);③`aeed4e4` MISS 取数恒 canonical + 全局 rank 接线,filename 筛选 891ms→内存派生级。验证:`cargo test --workspace` 522/0/5 + rustfmt + clippy `--workspace -D warnings` 绿(本地非 CI)。三件套 [planning/2026-07-14-统一文件树与画廊目录排序/](../planning/2026-07-14-统一文件树与画廊目录排序/) B-file-iii 详细设计 v2.0。**⏸GUI**:真机点筛选/切分组/跨轴切换手感(阶段4);F-007 搜索谓词 localtime 待裁;B-file-ii(D-013)仍独立待 go |
| ✅ | **S1** 时间轴 folder/none 模式 DOM 降采样(桶聚合到条像素高,2042→~数百) | **2026-07-09 交付(本会话)**,⏸GUI(轴点数 ≤300、疏密可辨、点轨吸附/hover 不回归)→ **▸ 详注 O-S1** |
| ✅ | 时间轴 date 模式改逻辑 y 比例定位(与滚动条同步)+ 修缩略图遮挡 | `f67be18`,GUI 验收通过(2026-07-09)→ **▸ 详注 O-date** |
| ✅ | 文件树一键 展开全部/折叠全部(压测辅助 + 实用) | `4940a3b` → **▸ 详注 O-展开** |
| 🔨 复活 | **时间轴 canvas 密度带升级线(P1–P4)** — 推翻本表 2026-07-09「canvas 长期搁置」裁决 | **🔨 P1–P3 全交付 ⏸真机(2026-07-10,推翻 canvas 搁置裁决)**:方案 [时间轴密度带 canvas 升级](../designs/2026-07-10-时间轴密度带canvas升级方案.md);canvas 转正(`fa7ff96`)→ P1 三视觉切换(`3699a97`)→ P2 全轨渐变聚光(`7d824fb`)→ P3 time 日历坐标全链(后端 epoch_day,`d06a80d`)→ 深审修复批 R1-R8(`0efacdc`..`6988e53`,朝向自适应 / 日历 guard / 行色缓存,后端 epoch_day 链全对)。vitest 499·cargo 456(仅本地)。**⬜ P4**:真机 paint timing 与 DOM 并排实测定视觉/坐标留舍 + DOM 版去留(§12)。详注见 [completed.md](../completed.md) ▸ O-timeline |
| ✅ 复活 | ~~loupe 放大镜长期搁置~~ **已随 canvas scrubber 实现**(推翻本表搁置裁决) | canvas 版原型即内建放大镜(光标最近 K=9 分隔符富标签列表),观感⏸真机 → **▸ 详注 O-loupe** |
| ✅ | 键盘 slider(role=slider + ↑↓逐 separator)a11y | `6fd7c1f`;读屏实操⏸GUI → **▸ 详注 O-键盘** |
