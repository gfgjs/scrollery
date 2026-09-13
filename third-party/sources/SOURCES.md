# Third-party source packages

本目录归档随安装包分发的第三方编译件所对应的**上游源码包**，用于履行 `ADDITIONAL-PERMISSION.md`
中 Graphviz / Viz.js 源码与构建脚本的提供义务，以及 Expat 的义务。构建流程不读取也不解压这些
文件；包内版本已核实，无需联网重取。SHA256 为文件级固定值。

| 文件 | 上游 | 版本 | SHA256 |
| --- | --- | --- | --- |
| graphviz-2.40.1.tar.gz | Graphviz 官方历史源码目录 https://www2.graphviz.org/Archive/stable/SOURCES/ | 2.40.1 | ca5218fade0204d59947126c38439f432853543b0818d9d728c589dfe7f3a421 |
| vizjs-2.1.2.tar.gz | mdaines/viz.js 标签 v2.1.2 源码归档（解包目录 viz-js-2.1.2） | 2.1.2 | 4ecde8e5243ec0e8f4b694a922c9f531d3144f44399686e1cc6da721d9757462 |
| expat-2.2.5.tar.bz2 | https://github.com/libexpat/libexpat/releases/download/R_2_2_5/expat-2.2.5.tar.bz2 | 2.2.5 | d9dc32efba7e74f788fcc4f212a43216fc37cf5f23f4c2339664d473353aedf6 |

## Viz.js 构建入口（Graphviz 2.40.1 的 Emscripten 单文件产物）

Viz.js 2.1.2 的 Makefile 内钉定 `GRAPHVIZ_VERSION = 2.40.1`、`EXPAT_VERSION = 2.2.5`、
`EMSCRIPTEN_VERSION = 1.37.36`；入口目标为 `make full.render.js`，其依赖
`deps-full → expat-full + graphviz-full`（Graphviz 取自 Makefile 的 `GRAPHVIZ_SOURCE_URL`，
即上游官方 SOURCES 目录）。本仓 `public/vditor/dist/js/graphviz/full.render.js`
由 Vditor 3.11.3 分发，其头部标明上述上游版本；此处记录上游构建入口，不宣称已复现相同字节的构建。

本仓未执行上述编译：产物字节、尺寸与版本标记由 `scripts/lib/vendored-artifacts.mjs` 固定并校验。
