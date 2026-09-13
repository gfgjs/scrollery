# foliate-js(vendored)

Scrollery 阅读器的统一渲染引擎(EPUB / txt / md 共用),源自开源库 **foliate-js**,
以**钉定 commit 全量 vendored** 方式引入(上游无 npm 发行、且自认 API 不稳定,vendoring 是
唯一可控方式)。本文件是该 vendoring 的权威登记与升级手册,承接
`docs/designs/2026-07-07-阅读器完善方案.md` §4.2 / §10。

## 来源与版本

| 项 | 值 |
|---|---|
| 上游仓库 | https://github.com/johnfactotum/foliate-js |
| 钉定 commit | `78914aef4466eb960965702401634c2cb348e9b1`(2026-07-07 时的 `main` HEAD) |
| 许可 | MIT(见本目录 `LICENSE`,Copyright © 2022 John Factotum) |
| 引入期 | 阅读器方案 R2-1 |

MIT 是宽松许可,允许 vendored 分发(须保留 `LICENSE` 与版权声明,本目录已随附)。
**与禁抄红线的区别**:方案 §10 禁抄的是 Readest app 层 / Koodo·kookit / legado /
Foliate **GTK app** / Vivliostyle / KOReader —— 它们是 (A)GPL 族;而 **foliate-js 本身是
独立的 MIT 库**(与 Foliate GTK 桌面应用不是同一许可主体),可 vendored。引入时已逐文件
核对文件头无异种许可声明。

## 保留的文件(view.js 传递闭包 + 库核心)

`view.js` 是主自定义元素 `<foliate-view>`(import 即 `customElements.define`)。它静态依赖
`epubcfi.js` / `progress.js` / `overlayer.js` / `text-walker.js`,其余格式 loader 与渲染器
均为**按需动态 import**。保留集 = view.js 的完整传递闭包,使所有 `await import(...)` 在构建期
可解析:

- 核心:`view.js` `paginator.js` `fixed-layout.js` `epubcfi.js` `progress.js`
  `overlayer.js` `text-walker.js` `search.js` `tts.js`
- 格式 loader:`epub.js`(**我们实际使用**)、`comic-book.js` `fb2.js` `mobi.js`
  (view.js 动态 import 的构建期目标;仅依赖小体积 zip.js/fflate.js,保留零成本,且为 R6 的
  mobi/azw3 插件轴预留后端)
- `pdf.js`:**已替换为 stub**(见下「剥离项」)
- `uri-template.js`(闭包内被引用)
- `vendor/zip.js`(36K,epub/comic zip 解压)、`vendor/fflate.js`(4K,mobi 解压)

## 剥离项(有意不引入)及理由

| 剥离 | 大小 | 理由 |
|---|---|---|
| `vendor/pdfjs/`(整目录) | ~13M | Scrollery 的 PDF 走独立 `PdfReader.vue`(pdfjs-dist),不经 foliate;删除该 cmaps blob |
| `pdf.js`(换 stub) | — | 上游 `pdf.js` 依赖 `vendor/pdfjs/pdf.mjs`;换成只导出 `makePDF`(抛错)的 stub,满足 view.js 动态 import 的构建期解析,运行期永不触达(PDF 不路由到 <foliate-view>) |
| `reader.html` `reader.js` `ui/`(tree.js/menu.js) | — | 上游 demo 阅读器应用;Scrollery 用自建 Vue UI(BookReader.vue + 侧栏),不用 demo。`ui/` 仅被 `reader.js` 引用,一并弃后闭包仍自洽 |
| `dict.js` `opds.js` `footnotes.js` `quote-image.js` | — | 独立特性,**view.js 闭包不引用**(经反向依赖核对确认);需要时再单独引入 |
| `tests/` `rollup/` `eslint.config.js` `rollup.config.js` `package.json` `package-lock.json` `README.md` | — | 上游 dev 工具链,非库运行代码 |

**本地补丁清单(升级时须重做)**:仅 `pdf.js` 一处——整体替换为 stub。其余文件与上游 commit
逐字节一致(升级只需重拉对应 commit 覆盖 + 重做此 stub)。

## 构建集成

- **ESLint**:`eslint.config.js` 的 `app/files-to-ignore` 已加 `src/vendor/**` —— vendored 代码
  风格(私有 class 字段 `#x`、生成器)非本仓约定,且须保持与上游零 diff,不参与 lint。
- **Prettier**:`.prettierignore` 已加 `src/vendor/` —— 不重排,保持升级可 diff。
- **类型**:vendored `.js` 不进 vue-tsc(tsconfig `include` 不含 `.js`);消费侧类型由
  sibling `view.d.ts`(仅声明 Scrollery 消费面)提供。升级若改动 view.js 公开面,同步更新
  `view.d.ts`。
- **许可登记**:vendored 源不在 `package-lock.json`,`scripts/generate-notice.mjs` 收录不到 →
  该脚本已内置 `VENDORED_FRONTEND` 手工登记表(含本条目),NOTICE.md 的「Vendored source」
  小节由此生成,`--check` 门控覆盖。**升级改版本/commit 时须同步该表**。

## 升级流程

1. `git clone https://github.com/johnfactotum/foliate-js && git checkout <新commit>`
2. 用「保留集」覆盖本目录对应文件(勿动 `pdf.js` stub、`view.d.ts`、本 `VENDOR.md`)。
3. 重做 `pdf.js` stub(整体替换)。
4. 若上游改了 `view.js` / `epub.js` 公开面,同步 `view.d.ts` 与 BookReader 消费点。
5. 更新本文件的「钉定 commit」+ `scripts/generate-notice.mjs` 的 `VENDORED_FRONTEND` 版本;
   跑 `node scripts/generate-notice.mjs` 重生成 NOTICE.md。
6. `npm run build` + `npm run typecheck` 过;GUI 真机跑方案 §9 epub 清单(升级回归硬门)。
