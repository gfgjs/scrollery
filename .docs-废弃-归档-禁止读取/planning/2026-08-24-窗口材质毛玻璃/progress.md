---
status: 施工中
type: 工作记忆
line: 窗口材质毛玻璃
created: 2026-08-24
---

# 进度日志:窗口材质毛玻璃

## 会话:2026-08-24
- 做了:读方案全文,建三件套;派 Rust/前端两个子代理按方案 §4 并行施工,两侧均完工且无实质偏差(前端三处合理裁决:白名单含 none 三值——排除 none 会导致用户持久化「不透明」重启回玻璃;isWindows 复用 utils/platform;平台分支测试走 doMock+resetModules)。
- 终审:diff 对照方案 §4 全部吻合;主会话补一处子代理遗漏——`src/harness/ipcFixtures.ts` 的 StartupConfig 构造字面量缺 `windowMaterial`(vue-tsc 抓到,vitest/build 不做类型检查),补 `windowMaterial: null`。
- 验证(全绿,退出码均落盘复核非管道):cargo fmt --check 0 / clippy -D warnings 0 / test 1327 过 25 套件;npm lint / typecheck / vitest 1564 过 / build ✓;NOTICE --check 零 churn(rust 442 + npm 127 + vendored 1)。
- 遗留:双 commit(门禁债 → 功能)已执行;后续为真机手动验收(方案 §7 手动清单)与浓度微调(72/82 初值)。

## 会话:2026-08-24 真机反馈审查
- 做了:按 AGENTS“善用子代理”并行派原生层、前端/CSS、测试契约三路只读审查;主会话对照截图、实现提交与本地 `window-vibrancy 0.6.0` 源码交叉终审。确认红圈为 `.app-content`+`.settings-view` 实色截断,侧栏红箭头为 opaque sticky header,右上三键区为嵌套同材质重复合成;另登记 Acrylic 失焦未重挂与 Win11 22000–22522 切换状态互斥风险。
- 验证:`npm test -- src/stores/uiStore.spec.ts src/themes/theme-contract.spec.ts` → 2 文件/29 测试全绿;这些测试只钉配置与 token/属性,不渲染真实背景。`cargo test -p scrollery window_material --lib` → 编译通过但 0 个匹配测试(1100 filtered out),证实原生材质分支无单测覆盖。
- 遗留:本轮按用户“Review”只审不修;待授权后按“先修表面层级与原生互斥/失焦,再六主题真机调 alpha,最后补视觉/调用序列测试”的顺序施工并复验三模式。

## 会话:2026-08-25 反馈修复施工
- 做了:按授权实施表面层级修复与原生生命周期修复。`glass.css` 取消 `.app-content`/`.settings-view` 的整块实色、清除窗口三键区重复材质、为侧栏 sticky 头栈加高不透明玻璃遮罩;Rust 切换前清三种旧背板,`Focused(true)` 按配置重挂;新增 `glass.spec.ts` 三条静态契约。
- 验证:`cargo fmt --all -- --check` 0;`cargo test -p scrollery lifecycle --lib` 5/5;`cargo check --workspace --locked` 0;`cargo clippy --workspace --locked -- -D warnings` 0;`npm run lint` 0;`npm run typecheck` 0;`npm run test` 138 文件/1567 测试全绿;`npm run build` 0(入口 bundle 680.77 kB,预算余 27.23 kB);定向 glass/uiStore/theme 32/32。
- 遗留:仍需 Win11 真机/全新 profile 手动验收三值切换、外部 config 热切换、托盘/F11/窗口操作、六主题明暗壁纸可读性、Win10 blur 与 macOS 构建;本环境未自动化这些 GUI/跨平台项。

## 会话:2026-08-25 红圈表面层级微调
- 做了:按用户确认将 `glass.css` 的高不透明 sticky 遮罩替换为 Mica/Acrylic 分别定义的 `--glass-*` token；侧栏标题/粘性路径行低 alpha + 12px blur，普通树行透明，工具卡/紧凑 select/搜索框/侧栏开关改用半透明表面。搜索框同时覆盖独立 `.app-toolbar` 与合并 `.window-chrome` 两条布局路径。
- 验证:本地 UI harness 视觉检查了 Porcelain Mica、Ink Mica；computed style 确认 Mica sticky/card/search 为 42%/24%/38%，Acrylic sticky/card 为 36%/20%，搜索焦点态提升至 48% 并有品牌色 focus ring。`npm run test -- src/assets/styles/glass.spec.ts src/themes/theme-contract.spec.ts` 19/19；`npm run check:contrast` 六主题硬门槛全绿；`npm run build` 通过（入口 680.95 kB，预算余 27.05 kB）。
- 遗留:浏览器 harness 不含 Tauri/DWM 原生背板，仍需既有 Win11 真机与跨平台手动验收清单。

## 会话:2026-08-25 画廊玻璃底面
- 做了:核对后确认画廊不是玻璃——DOM `.media-grid-layout` 铺不透明 `--color-bg-canvas`，Canvas 路径固定 `alpha:false`+`canvasGap`。按用户指令改为 Windows Mica/Acrylic 专属透明底面：DOM 主容器 transparent；Canvas 以 `glassBackground` 选择 alpha context，玻璃路径 `clearRect` 留透明，切换材质时以 key 重建不可变 alpha context 并在 `nextTick` 后重绘；`none` 保留原不透明快路径。
- 验证:`npm run test -- src/components/media/MediaGridCanvas.glass.spec.ts src/assets/styles/glass.spec.ts src/components/media/mediaGridCanvas.helpers.spec.ts` 70/70；目标前端 ESLint 无 error、`npm run typecheck` 通过、`npm run build` 通过（入口 680.96 kB，预算余 27.04 kB）。本地 UI harness 载入 Mica，computed style 确认 `.media-grid-layout`、`.media-grid`、`.mgc-canvas` 背景皆 transparent；harness 无 Tauri/DWM，不能替代原生壁纸透出目检。
- 遗留:补入既有 Win11 清单，实测照片缝隙在 Mica/Acrylic 下露出原生背板、切 none 后恢复主题 canvas 底色，且 DOM/Canvas 实验渲染路径均无残帧。

## 会话:2026-08-25 毛玻璃透明度配置化
- 做了:按用户需求把玻璃浓度拆为窗口栏、分组/路径遮罩、卡片表面、控件表面 4 组 `glass_*_opacity`。Rust schema 默认值 100 且 hot，`StartupConfig` 一次下发；uiStore 设置器和 `config-file-changed` 热更新共用 20–120 收敛与 CSS scale 应用；设置页 general 区新增 4 个双语数字输入。
- 做了:glass.css 保留 Mica/Acrylic 各自基准百分比，只对对应 surface 乘 `--glass-*-scale`，并覆盖 toggle、border、highlight 等控件透明层；新增 CSS/static 与 uiStore 回归断言。
- 验证:`npm run test -- src/assets/styles/glass.spec.ts src/stores/uiStore.spec.ts src/components/media/MediaGridCanvas.glass.spec.ts src/components/media/mediaGridCanvas.helpers.spec.ts` 88/88；`npm run typecheck`、目标 ESLint、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml schema -- --nocapture`（8/8）均绿；本地 harness 设置页可见 4 个输入，CSS.supports 与 computed 玻璃表面均通过；`git diff --check` 通过。
- 遗留:Win11 真机确认 4 组外部 config.toml 热切换、材质切换与画廊/设置页透底的最终观感；当前 harness 的 Tauri IPC 不写真实 config.toml。

## 会话:2026-08-25 画廊底面透明度配置化
- 做了:补充独立 `[ui].glass_gallery_opacity`，默认 0%、范围 0–100；设置页新增“画廊底面不透明度”，前端水合/设置器/外置 config 热更新统一写入 `--glass-gallery-opacity`。
- 做了:玻璃模式下 `.media-grid-layout` 使用主题色 `color-mix` 遮罩，Canvas 的透明图片间隙会随之露出该底层；none/非毛玻璃路径继续使用原 `--color-bg-canvas` 与 opaque Canvas。
- 验证:前端全量 140 文件/1581 测试、typecheck、lint、build、Rust schema 8/8、Rust fmt、`git diff --check` 均通过；本地 harness 设置页显示第 5 个输入，Mica 下默认 `--glass-gallery-opacity=0%` 且画廊 computed background 透明。仍遗留 Win11 原生背板与外部 config.toml 热切换人工验收。

## 错误账(本会话实际踩坑)
| 错误 | 尝试 | 解法 |
|------|------|------|
| `cmd 2>&1 \| tail` 管道吃 exit code,npm 串与 clippy 两次假绿 | 以 tail 退出码与后台任务 exit 0 判绿 | 验证串一律 `> log 2>&1; ec=$?` 落盘取真码,读输出内容复核 |
| typecheck 报 ipcFixtures 缺 windowMaterial | — | 接口加键后须全仓搜构造字面量(harness 夹具是第二构造点);Rust 侧子代理搜了 StartupConfig 构造、前端侧漏搜,主会话终审补 |
| `cargo fmt --all --check` 红:mark_missing/enricher/metadata 5 处 + clippy 1.98 新 lint `chunks_exact_to_as_chunks`(embedding.rs) | 排查是否本次引入 | 均既有债(189af9cf scan 提交遗留 / clippy 1.98 新 lint,CI stable 同版本会红);修复合入独立门禁债 commit,不混入功能 commit |
| `npm test -- --run ...` 被 npm 解析为未知 config flag | 误把 vitest 的 `--run` 再传给已含 `vitest run` 的 npm script | 改用 `npm run test -- <spec...>`;脚本自身已固定 `vitest run` |
| 搜索框仍是纯白 | 只以 `.app-toolbar` 为祖先写覆写，合并标题栏下实际祖先为 `.window-chrome` | 通过本地 UI harness 查 computed style，补齐两条祖先路径并重载复验 |

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
