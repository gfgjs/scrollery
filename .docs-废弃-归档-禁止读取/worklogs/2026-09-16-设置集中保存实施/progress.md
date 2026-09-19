---
status: snapshot
type: working-memory
line: 应用配置重构-外置配置文件与热应用
created: 2026-09-16
---

# 进度日志

## 2026-09-16
- 已核对 git status、现有设置入口、方案、项目规则。
- 基线 HEAD 9f75e1bb；演示打码已提交，按当前代码接入。
- 四个白名单 High 代理已委派；契约与写集记录 interfaces.md。
- 尚未运行测试，未改真实设置。
- 消费者首批已接入布局、播放、阅读、查看器等；中央模块和窗口模块已出现，主会话进行关键边界早审。
- 已回传必须修复项：中央保存队列的采集时generation、确认快照与预览分离、flush错误传播、reset互斥；窗口flush必须drain全部、失败可重试、最大化normal bounds保存、尺寸坐标口径与共享锁内合并。
- 已要求初始启动门控保护演示打码首帧；阅读器外部快照竖排变化需capture-first remount回归。
- Planck 首批完成：消费者相关14个测试文件149项通过、相关文件ESLint退出0（代理更正：原22文件计数有误，源批实际21，后续补查其他文件）；主会话已审查关键播放/阅读/布局路径，不重复整套测试。全局typecheck尚未运行。
- 已追加同一代理处理开发harness的启动/设置契约适配，避免旧fixture失效；日志窗口独立入口已增加设置初始化。
- Harness追加完成：ipcFixtures新契约11项测试，连同searchStore/theme-snapshot共73项通过；格式修复影响的4个测试文件30项复核通过（与首批有重叠，不累加成独立总数）。
- 已发现日志窗口不经过App.vue，窗口代理接手main.ts logs分支挂退出flush桥；主窗口仍由中央代理挂桥。
- 后端代理获唯一Cargo局部验证时间窗：cargo test --offline -p scrollery --lib config::；窗口代理不并发跑Cargo。

- 消费者最终补查：阅读器整数归一化与 harness 默认值/旧 generation 拒绝用例，2 文件 29 项通过（与前批重叠）。窗口前端桥、locale、日志 store 共 46 项通过；相关 ESLint 与 Rust rustfmt parse 检查通过，Rust 尚待编译。
- 独立只读审查发现两条退出竞态：exit 原生关闭提前销毁 webview；重复 ExitRequested 在 prevent 前放行。已交窗口代理修复并补回归，显式 force 也须避免二次确认。
- 主审追加：监听注册必须完成后才取初始快照；reset 迟到 ACK 不能覆盖更高 revision；内部 patch 合外部磁盘修改后必须整份刷新内存；窗口单 label 合并须基于当前磁盘；派生热应用由后端每批一次且重置保留停止意图。

- 中央前端收尾：30项保存服务、5项配置store、22项UI store及其他相关用例共60项通过；最终全局typecheck退出0，相关ESLint通过。代理前端全套1927通过、1项theme-contract失败；主会话将CloseConfirmDialog未定义颜色改为既有--color-error，对应主题测试19项通过。首次过滤路径拼错未找到测试，已更正路径，不将该次记通过。
- 窗口桥最终9项通过；两条退出守卫修复经独立只读复核确认；窗口失败值保留到显式flush的Rust测试已补，待后端统一执行。已移除新建的App源码字符串测试，实际首帧渲染仍属未验证。

- 后端检查点：cargo check --offline -p scrollery --lib 曾exit0（36.48s，无warning）；随后三处生产直接写配置已改统一入口，尚待复验。config::测试先编译失败，修后遇0xC0000139测试进程启动失败；最后窗口测试restore_pending少attempts参数与unused json仍待修。主会话将唯一Cargo窗口移交窗口代理，原后端停止改动。
- Windows测试缺CommonControls v6 manifest，TaskDialogIndirect导入缺失；新增原生失败提示可能引入该测试链接依赖，尚不能称与改动无关。已限制解决方案为本crate测试链接/现有SDK工具，最多两个证据方案，不用全局RUSTFLAGS触发第三方全量重编。原后端测试排查在target/TEMP创建文件，待精确清单后安全清理。

- 用户指出本轮消耗约10%周限额，主会话承认拆分/边界设计及反复审查成本过高，停止扩大工作。最终收取结果：Rust config:: 79通过/2失败（value.rs新增JSON/TOML样例解析失败，尚未修复复验）；lifecycle::13通过；库clippy -D warnings退出0。build.rs本crate Windows manifest链接解决测试启动。代码仍未提交，不能记为全部完成。

- 用户要求继续做完。范围限定：只修value.rs两项失败并复跑config::value::tests，整理文档与本轮提交；不新增审查、不重复全套测试、不扩大构建。原后端作者承担该单文件任务。

- 最终修复：value.rs字符串/枚举字段原先输出裸串，导致日志预设结构无法作为合法JSON再次解析；改为JSON字符串编码。另一失败为测试把JSON常量拼入TOML，已改为原生TOML样例。config::value::tests 16通过/0失败，覆盖此前2项失败；该文件rustfmt --check通过。未重复此前通过的测试。
- 最终证据：前端typecheck通过；全套1927通过/1失败后修复颜色变量，对应19项主题测试通过。Rust配置原79通过/2失败，失败所在值转换组最终16项通过；生命周期13项通过；库clippy -D warnings通过。代码与局部验证完成；真实窗口、DPI/副屏、首帧及macOS/移动端交互未验证，保留本线待办。
- 原后端代理续作遇提供商400错误，切换现有白名单代理完成单文件修复；未增加代理数量、依赖或完整构建。

## 回顾
- 应先收敛最小实现，再按实际失败修复；多路实现叠加重复审查与环境排查导致本轮成本偏高。
- 最后一轮只处理两项失败、复验所属测试组后立即收口，没有再次运行整套测试。
