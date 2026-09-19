---
status: 快照
type: working-memory
line: 全仓注释清理精简
created: 2026-07-25
---

# 任务计划:全仓注释清理精简

## 目标
在不模糊原意、不损失准确性的前提下,清理全仓源码注释:删冗余/复述型注释、删注释掉的死代码、中英双语并存处删英文留中文;附带把发现的简单问题直接修掉,拿不准的列清单待用户裁决。代码语义零变更。

## 当前阶段
全线收口:待裁 5 项已全部处置(2026-07-25 用户裁「开工」);新发现 F-h(残留 515 行 / 84 文件)同日裁**暂不做、留账**,重启须新决策。**无未决项**

## 范围
- **在范围**:`src-tauri/src/**`、`crates/**`(`.rs`);`src/**`(`.ts` `.vue`,**排除 `src/vendor/`**);`scripts/**`
- **不在范围**:`docs/**`(Markdown 文档)、`src/vendor/**`(vendored 第三方)、生成文件、`*.json`/配置

## 清理规则集(施工代理逐条照做,禁自行发挥)

### 删
1. 同一处中英双语重复表达 → 删英文,留中文
2. 纯英文散文注释 → 有信息价值的改写成中文一行;纯复述代码的直接删
3. 复述代码的注释(`// 增加计数` 紧跟 `count += 1`)
4. 注释掉的旧代码,除非同处写明保留原因
5. 装饰性 banner 分隔线、空注释、编辑痕迹(`// 新增`、`// 已修改`、日期戳)
6. 同一事实在文件头/函数头/行内重复多遍 → 只留最靠近代码的一处
7. 明显与代码不符的过时注释 → 改对;拿不准的进待裁清单,不擅改

### 留(红线,删了即事故)
- 抑制/指令类:`// SAFETY:`、`eslint-disable*`、`@ts-expect-error`、`@ts-ignore`、`prettier-ignore`、`biome-ignore`、`@vite-ignore`、`#[allow]` 的理由说明、clippy 抑制说明
- license / copyright 头;`src/vendor/**`;标注 generated 的文件
- 决策锚点(D-xxx / F-xxx / R-x)、红线注释、测试钉定说明(「改必跑 xxx」「勿回退」)
- rustdoc `///` `//!` 与 TSDoc `/** */` 公开 API 文档:可精简措辞,不整段删
- 解释「为什么」的注释、边界条件/踩坑说明、外部 issue/PR/URL 引用
- 注释里的英文标识符、类型名、参数名、外部 API 名与术语(不属「英文散文」)

### 不动
代码语义、格式(交给 rustfmt/prettier)、字符串字面量与用户可见文案、Markdown 文档。

## 阶段

### 阶段 1:摸底
- [x] 注释体量统计 + Top 文件清单 + vendored/generated 排除名单
- [x] 英文注释 / 中英双语重复定位清单
- [x] 门禁基线红名单(改前状态)
- **状态:** done

### 阶段 2:分批施工
- [x] 按文件域不相交分批并发清理,每批落地即复核;15 代理 workflow 分域严审 1078 hunk,50 条发现经对抗核验确认 11 条并修复;另一轮改写型注释复核 42 条过压缩已全部补回;取证式审计对 1016+ 条被删非 CJK 注释行逐条对拍
- **状态:** complete

### 阶段 3:门禁收口
- [x] npm run lint / vue-tsc / vitest(120 文件 1454 测试)/ cargo clippy / cargo test --workspace 全绿;`cargo fmt --all --check` 仍红,文件集与基线相同(enhance-worker 的 main/run/session.rs + src-tauri/src/enhance/service.rs + src-tauri/src/ipc/enhance_commands.rs),属 dev 既有债,本线未代修
- [x] 待裁清单交付用户(见 progress.md 末尾)
- **状态:** complete

### 阶段 4:待裁 5 项处置
- [x] 第 1 项 fmt 5 文件既有债 → 清掉(`cargo fmt --all` 只动那 5 文件,纯排版)
- [x] 第 2 项 state.rs 中英混杂残留 → 落地(顺带清同文件 15 处同类英文块 + 2 条纯英文孤行)
- [x] 第 3 项 fast_scan.rs 重复行 → 落地(顺带清同文件 5 处英文块)
- [x] 第 4 项 en-US.ts `U-2` → 核实为「复制 zh-CN.ts 既有注释」,非新编,无需改动
- [x] 第 5 项 ReaderSettingsSection.vue → 核实无可删项,跳过正确
- [x] 门禁复跑:fmt / clippy -D warnings / test --workspace 全 🟢(fmt 基线红已消)
- **状态:** complete;新待裁 F-h(第二轮清残留)未纳入本阶段

## 落地提交
- `6f1cd57 fix(gate)`:eslint 排除 `.claude/**` + clippy err_expect×3
- `d9b337f refactor(comments)`:283 文件,+744/−3068,净减 2324 行(含三件套本身),已落 dev(未推)
- `762d1e2 style(fmt)`:待裁第 1 项,enhance 子系统 5 文件排版归正,零语义改动
- `ba3be1c refactor(comments)`:待裁第 2/3 项,state.rs + fast_scan.rs,2 文件 +7/−70

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 中英双语处删英文留中文 | 项目规则:注释散文用中文 | |
| `src/vendor/` 与 generated 文件不动 | 第三方/生成物,改动会被覆盖或触碰 license | |
| 抑制指令类注释(eslint-disable/SAFETY 等)一律不删 | 删了会破坏编译/lint 语义,属代码而非注释 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
