---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-09-12
---

# 进度

## 2026-09-12
- 已接受执行与分批commit授权；三路子代理开始A/B/C1实现，禁止代理独自commit或全量测试。
- 代理：01a093fc-0f5e-7bf1-b4f8-17fd2c913b9a（A）；01a093fc-103b-73c0-94d9-4d7fb89ad4b9（B）；01a093fc-1104-7330-b564-9a9587d65297（C1）。
- 主会话准备验证环境与文档，等待独立批次回传后即审查提交。
- 用户追加授权在本机真实库测试，已通知基准代理01a093fd-df71-79b0-afb2-806f1d14cbb7改用真实普通app实例与WebView2，结束恢复配置。

## 验证
- C1：代理集中验证 thumbnail::serve 8、layout::items_cache 19、layout_swap_during_offloaded_io_is_rejected 1均通过；本文件rustfmt --check通过；主审复核惰性候选、stat去重/截止与零IO回写，diff --check通过。无需重复运行同一组。
- C1已独立提交bf7806cd。
- B：共享预取因子与两引擎Canvas窗口完成；主审收敛为单函数+一行余量，MediaGrid使用同一canvasActive判据。代理集中测试181项通过、vue-tsc通过、相关ESLint通过，主审diff --check通过。
- A：主体修复与指标已完成，相关319项集中测试通过；主审要求积分按gauge变化时间结算，离屏首绘跟踪及时退出、失活清零；仅重测受影响指标组。
- A终审修正完成，追加perf/pipeline集中35项通过；全局vue-tsc、相关ESLint与前端生产构建通过（既有混合静态/动态导入提示，不影响构建）。B已提交df82ab62。
- A已提交ae2b5171。Rust集中静态检查cargo clippy -p scrollery --lib --locked -- -D warnings一次通过。C2因当前小档供给已存在且后台让路/换根删除竞态尚需独立设计，本轮暂不落地。
- 真机初始51格记录仅为环境探针，已排除出4K/64px目标基线。代理继续定位真实密集区域并记录实际源档位，不能用HEAD标签代替被服务代码版本证明。
- 初轮ab-baseline-1录制前曾在首部停留2.5s，污染冷首入，已排除正式比较；改为远处DOM→Canvas重挂清空LRU，再校准后冷进入。另发现仪器未排除DB终态thumbStatus=2，主会话按media.ts状态定义补上该排除，两臂共同使用；相关ESLint通过。

## 回顾
- 有价值的做法：先核对实际格数、DPR/backing与LRU归零，再比较相同轨迹；每批及时提交，回退由真实数据纠正。
- 教训：HEAD不能证明Vite实际服务源码；录制前停留目标区会污染冷首入；永久失败占位不能算加载等待，空样本p95不能当零延迟。
- 意外：本机早已有大量64档且实际被服务；初版调度反而取消回滚会复用的请求，局部raw绘制对暖路径改善更明确。冷进入未达到50%建议目标，已作为后续独立事项登记。

## 真机首轮裁决
- 8b103163公共仪器排除终态失败后，两对真实密集短轨迹均显示A+B初版回退：冷格时间比例69.24/69.84%→73.39/75.33%，候选取消372/676次；暖0.94/1.01%→1.54/1.92%，draw P95仍约25ms。该数据不作优化成功宣称。
- 已委派A将近期请求保护补齐后向0.5屏（原仅前向），避免短往返取消马上复用的请求，集中补1条回归再定向复测。
- 后向保护修正完成，前后各0.5屏且每侧最多512项，方向反转对称；新回归覆盖近带保留/远处腾槽，pipeline集中15项通过。主审通过，准备提交与定向验证。
- B只读确认Canvas内局部drawRows=props.rows.map(toRaw)可避免热循环深代理；命中检测/悬停仍消费代理props.rows，保留hover响应。等待CPU profile证明热点后才实施D，不改父层canvasRows（那样会丢hover响应）。
- CPU profile已取得（baseline-overlay，4.2s）：isRef2自耗200.6ms、Vue get自耗94.7ms/含子调用410.4ms，支持局部raw视图小实验；嵌套inclusive不能相加或直接称53%全是代理开销，原生Canvas也占显著部分。
- 3d12de7b两侧保护实测一轮：cold72.21%、warm1.36%，取消0；消除了初版372/676次取消，但尚不宣称整体优于旧基线。
- D仅修改MediaGridCanvas.vue新增drawRows computed，绘制/pipeline/selAnim读raw，命中/hover仍代理；单SFC ESLint通过。未提交实验SFC SHA256=D6EF473546F0F222A5FF2F0F88E12E2016016597360AC0CDC43A086743D311B0，等待同轨迹实测和真实悬停结构表征。
- D两轮结果确认保留：暖冷格比例0.08/0.06%（旧0.94/1.01%），暖draw P95=19.64/19.40ms（旧25.93/25.46）；冷进入67.24/68.47%，与旧69.24/69.84%差异有限，不宣称消除冷加载。冷首绘尾延迟波动大，暖第二轮无首绘样本，不能把空样本p95=0当零延迟。
- 真机表征verify-d-behavior.json确认：hover卡可见，hoverItemReactive/hoverSameAsRowsItem=true，rawRowItemReactive=false，与代理底层原对象相同；临时探针写入可从代理读回且已删除，未改真实媒体字段。DOM↔Canvas切换通过。
- D终态再次全局vue-tsc、相关ESLint、前端生产构建通过；不重复全量单测。下一步仅一次native重建+真实C1首屏短滚动smoke，恢复现场并归档。
- D已提交33f54140；新native二进制2026-09-12 14:09:34，真实库首屏/480px短滚动466个asset响应全部200，零页面/控制台/网络错误。少量空占位仍在，未宣称全部无占位。
- 已恢复原1280×849/DPR1.5/Canvas984×783、scrollTop0与localStorage，config完整哈希与基线一致，自有app/Vite关闭且9223/1420无监听。证据native-smoke.json / restore-final.json；未入库真实媒体截图。C1代理接续完成恢复，避免因另一提供商429留下实验进程。
