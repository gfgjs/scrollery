---
id: 2026-09-16-打包体积调查-findings
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-16
---

# 调查发现（2026-09-16现有产物快照）

以下统一使用MiB（1048576字节）。

- target/release/scrollery.exe：55316480字节，52.75 MiB。PE .text=33.56 MiB、.rdata=17.35 MiB、.pdata=1.62 MiB；不能把只读数据全算成前端，也未按Rust crate归因。
- MSI：56580701字节，53.96 MiB；NSIS：43034172字节，41.04 MiB，后者小23.94%。仅为现有产物比较，未安装验证功能一致。
- src-tauri/binaries四个AI DLL：63378184字节，60.44 MiB；加ai-worker为66.63 MiB。raw-worker=8.85 MiB，video-worker=1.47 MiB。
- NSIS与WiX生成清单均含mock_data.exe（1954816字节，1.86 MiB），源码src-tauri/src/bin/mock_data.rs为测试数据生成工具。排除它是明确候选；安装包实际减少量需重打包测量。另两个测试EXE虽在release目录，不能据此宣称已进安装包。
- NSIS已使用solid LZMA，WebView2为downloadBootstrapper，没有捆绑完整浏览器运行时。
- Cargo.toml:98 release使用opt-level=3、thin LTO、strip；106行另有z/fat/单codegen-unit的lite profile，但package.json:17的tauri:build:lite仅调用tauri build。Lite feature与lite profile不同。AI/RAW/video构建脚本固定release，主程序换profile不会同步改变worker。
- dist共489文件、21959424字节（20.94 MiB）；vditor=15136072字节（14.43 MiB），assets=6806225字节（6.49 MiB）。逐文件gzip模拟合计6405660字节（6.11 MiB），仅用来说明文本压缩效果，不等于Tauri嵌入体积或安装包贡献。
- VditorDocumentModule.vue:22以cdn=/vditor启用编辑器。可选图表渲染资源约7.9 MiB原始体积，直接删除会损失功能；字体格式精简候选须检查CSS及跨平台回退。
- vite.config.ts已接bundleBudget自定义预算，不能因关闭默认chunk告警就判定没有体积检查。

## 优化顺序

1. 下载分发优先现有NSIS；清除发布清单中的测试工具。
2. 保持图片链性能优先，评估fat LTO/单codegen-unit；对比s/z需要实测速度和体积。保留unwind，项目依赖catch_unwind处理坏图。
3. 若追求大幅缩减基础安装包，评估AI运行时首次使用下载/独立组件；66.63 MiB是未压缩上限，不是安装器可减少量，且不缩小主EXE。不可直接删DLL破坏AI/人脸/OCR。
4. 前端按功能需求裁剪，不以懒加载等同不打包；当前无足够证据承诺53MB降到某个目标。

## 外部资料（仅作数据）

- https://v2.tauri.app/concept/size/ ：LTO与codegen单元的体积选项；其中panic=abort示例不适用本项目。
- https://doc.rust-lang.org/cargo/reference/profiles.html ：s/z不保证更小，需测量；体积与速度的取舍不可直接推断。

## 耐久提升候选

| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 现有产物基线、发布测试工具与优化优先级 | 仓库架构与流水线全面梳理滚动状态 |
