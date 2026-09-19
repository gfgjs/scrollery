---
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# 进度日志:OCR文字提取

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:阶段 4 全波次施工闭环(T1-T11 全提交)+ 资产源已钉定(本机实测,见下段);全量门禁已跑;余=golden/bench 定案+GUI 手测六项+push 待批,见 task_plan 阶段 4 尾注
- 未解错误:无
- 关键指针:construction-plan.md(任务卡+D-OCR-1..7)/ model-assets.md(含 2026-07-23 追记段)/ findings 并发警报行
- 基线:开工时工作树已有他线未提交改动(2026-07-23-自定义ICC与色域切换 等),本线不得触碰;本线 commit 只 stage 本线显式路径

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-23
- 做了:两代理回执齐(researcher 14 轮、摸底 12 轮);主线亲读两落盘件,出三案对比+推荐组合+4 裁决点交用户
- 验证:无涉码;commit 只含本线路径,`git log -1 --stat` 末行 = `5 files changed, 617 insertions(+)`
- 拍板:4 裁决点全过(B′ / 付费插件 / 可选档位下载 / 前端 canvas),登 D-418..421
- 波次1+T4:T1(8r+复核4低修)/T3(8r+复核5修 A-E)/T4(8r+快扫清)全闭环;T2 落地 22r 22测绿待深审
- 提交:commit 1=T1 协议、commit 2=T3 门控、commit 3=立项文档批(哈希见 git log)
- 波次2:T2 深审 7 项修复闭环(box_score 掩膜/零尺寸护栏等,commit 见下)、T4 闭环;T5(21测)/T6(42测)落地待深审;T7 在飞
- 提交:批2=T2 管线、T4 registry(哈希见 git log);T5 侧 exotic-protocol lib.rs re-export 留待 T5 批
- 波次2收口:T5 深审 4 修(像素设防/逃逸锁测)+T6 深审 3 修(未知档回退)均增量核验闭环并提交;T7 落地待深审;T9/T10/T11 在飞(T10 已落地待快扫)
- 波次3+4:T7(深审2存疑闭环)/T9(深审2警告3低闭环)/T10/T11(快扫各1闭环)全提交(C1-C4 哈希见 git log);全量门禁:①`cargo test --workspace --quiet` 退出101,被基线红阻断(psd-worker E0004,非本批引入,见下)②`cargo clippy --workspace --all-targets -- -D warnings` 同一基线红阻断,退出101③`npm run lint` 退出0,零输出④`npx vue-tsc --noEmit` 退出0,零输出⑤`npx vitest run` 首跑1 failed(useExoticStore.spec.ts 缺 list_exotic_format_resolutions mock,C4 loadBuiltin 新增 IPC 调用触发,机械修复已提交)复跑 116 files/1405 tests 全绿,退出0
- 基线红(非本批引入,未修):crates/exotic-workers/psd-worker/src/main.rs:166 match RequestBody 非穷尽,缺 OcrSessionInit/OcrSessionClose/OcrBatch 三臂;源头=commit 156f016(T5/T6,先于本会话)未同步更新 psd-worker 兜底分支;阻断 cargo test/clippy 全仓;修法机械(仿现有 unsupported_variant 兜底加一臂)但跨 crate 越出本批四文件清单,留主线裁决
- 未覆盖(对照 ci.yml job 清单):rust-linux(Linux 编译面,本机 Windows 环境无法本地覆盖)、smoke(tauri build --no-bundle + 开机冒烟,未跑)、rust job 内 cargo fmt --check/渠道依赖树断言/NOTICE 新鲜度(未跑)、frontend job 内 sync-version/rename-gate/path-hygiene/exotic-protocol-sync/theme-contrast/vite build/verify-channel-bundle(未跑,超出给定五命令范围)
- 遗留:阶段 4 施工(批次 B0-B6 见 task_plan);阶段 5 见 task_plan
- 门禁复绿:psd-worker 穷尽臂补齐(新增红正名:系本线 156f016 连带非基线红)+ exotic-protocol-sync/fmt --check 补跑结果:`cargo test --workspace --quiet` 954 passed(0 failed,1 次偶发 flaky 隔离重跑绿)、`cargo clippy --workspace --all-targets -- -D warnings` 清、`node scripts/check-exotic-protocol-sync.mjs --selftest`+无参两跑均通过、`cargo fmt --all -- --check` 本线文件(psd-worker/main.rs)无 diff(仓内既有红均属他线文件未动)

## 会话:2026-07-23(资产源钉定,用户裁「先做一个能用的,后期换自有仓库」)
- 做了:主源定 RapidAI/RapidOCR ModelScope v3.9.2(RapidOCR 官方 `default_models.yaml` 逐一核对);用 `curl` 对 7 件文件本机实测(Range 请求取 `Content-Range` 精确字节数、HEAD 取 `X-Linked-Etag` 核对 6 个 onnx sha256、下载 dict.txt 自算 sha256+确认 mobile/server 字节级一致);重写 `ocr_registry.rs`:`OCR_ASSET_BASE` 单常量+`asset_url` 拼接废弃(ModelScope 四类文件分属不同子路径,dict 甚至在 paddle/ 树,单 base 拼不出),改 `PINNED_ASSETS` 七件全量 URL 表(GitHub oar-ocr v0.3.0 作镜像,det/rec/dict 有、cls 两档无);同步改 `ocr_commands.rs` 两处过期 doc 注释;model-assets.md 追记本机验证结果段(升级多条「转述/未验证」为已验证,含 dict.txt 真实行数 18383 行,原「约 3400+」摘要证实不可靠)
- 验证:`cargo test -p scrollery --lib ai::ocr_registry` 4 passed;`cargo test -p scrollery --lib ipc::ocr_commands` 编译通过 0 failed;`cargo clippy -p scrollery --lib -- -D warnings` 清;`git diff --stat` 仅 2 文件(ocr_registry.rs/ocr_commands.rs),无越域改动
- 遗留:golden/bench 定案(swap_rb 对拍+ocr_bench 实测)+ GUI 手测六项 + push 待批,同阶段 4 尾注;资产源为过渡方案,用户后续切自有仓库时只需替换 `PINNED_ASSETS` 的 url/mirror_url

## 会话:2026-07-23(真机下载报 size mismatch,根因定位到下载引擎非资产表)
- 现象:用户真机点下载,报 `ch_PP-OCRv5_det_mobile.onnx 大小校验失败:期望 4819576 实得 4826518`;先怀疑上条会话钉的字节数错——重新 curl 全量下载+sha256 复核,确认 PINNED_ASSETS 数值本身完全正确(问题不在资产表)
- 排查:curl 对比空 UA vs 非空 UA 直接钉死根因——ModelScope 前置 WAF 空 User-Agent 必 403,非空 UA(哪怕字面量 "reqwest")即放行且内容正确;`reqwest::Client` 默认不发 UA 头,是 `download/mod.rs::secure_client`(exotic/AI/face/OCR 共用的下载引擎)此前从未在 ModelScope 类源暴露过的既有缺口,与本线资产表钉定无关(是设施性 bug,借这次真机测试出的头)
- 修:`secure_client` 加 `.user_agent("scrollery/版本号")` + `.cookie_store(true)`(后者顺手对齐浏览器语义,非本次故障必要条件);用真实 reqwest(非 curl 代测)对 ModelScope 实测验证修复(临时 `#[ignore]` 测试跑绿后即删,不留 flaky 网络测试)
- 验证:`cargo test -p scrollery --lib -- download:: ai::ocr_registry ipc::model_download` 12 passed;`cargo clippy -p scrollery --lib -- -D warnings` 清;commit 11d766e(Cargo.lock/Cargo.toml/download/mod.rs 三文件,新增 `cookies` cargo feature)
- 遗留:待用户真机重试确认下载全程可用(4 文件 mobile 档);其余遗留同上不变

## 会话:2026-07-23(下载确认成功;真机推理报 cls 维度不符,golden 定案第一子项落地)
- 下载已确认成功(用户回执);签发一枚内测 license token(`node scripts/exotic-issue-license.mjs --plugin exotic-ocr --sku ocr-engine-2026`)供插件商店激活,dev build 已自取 `.internal-signing/internal-keyset.json` 为信任根,不需额外配置
- 真机点提取报 ONNX Runtime 错:`Got invalid dimensions for input: x index:2 Got:48 Expected:80 index:3 Got:192 Expected:160`——`cls_input_hw` 猜值 (48,192)(旧版 `ch_ppocr_mobile_v2.0_cls` 惯例)与真实模型(PP-OCRv5 `textline_ori` 新架构)不符,真机报错即定案证据,回填 (80,160)(commit 1fdcd67);mobile/server 同架构一并回填,server 档待其真机复核
- swap_rb(通道序)仍未定案——待用户下一轮真机测试看识别文字是否正常(乱码/空则疑 swap_rb 或其他环节),这是 golden 对拍清单第二子项
- 验证:`cargo test -p scrollery-ai-core --features inference` 23 passed(det/cls/rec/dict/geometry 全绿),golden/bench 两 `#[ignore]` 测试仍需 `OCR_MODELS_DIR` 手跑未跑
- 遗留:待用户真机重试提取,看识别文字质量(定 swap_rb)+ 其余遗留同上不变
