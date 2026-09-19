---
status: snapshot
type: working-memory
line: 全仓代码消融
created: 2026-09-15
---

# 发现

- 开工工作树干净。根 Cargo workspace 包含主程序、协议、插件契约、信任、AI 核心和多个 worker；RAW worker/probe 为独立 workspace。
- 默认构建 feature 为 custom-protocol/lite/channel-direct；另有 perf/netfs/ffmpeg、非商用人脸、渠道桩等审查面，必须核实实际调用后分类。
- 工程文档治理于 2026-09-15 退出 CI；旧文档结论需对照当前工作流与代码。
- 基线：264655第一方LOC/927文件；独立测试34431行/180文件，Rust内嵌测试仍在原文件分组。32主机顶层模块、261命令、79设置键、19状态键。
- 重点：ReaderLocator/旧settings scroll-spy/休眠VectorStore、旧IPC与去重链、H-Lab、空交付工厂和未来AES字段；细目已写入报告，避免此处维护第二份清单。
- 终审纠正：GpuToken在AI/face/enhance真实获取；logDir被设置页读写；generate_thumbnail在logging bench有真实消费者。
- 保留真实边界：用户文件/DB一致性、签名/授权、像素/路径/EPUB不可信输入、GPU并发及generation/source_revision；未证明双滚动/DOM-Canvas可等价删除。
- 字节收益单列：MathJax46文件/6634702字节；不计入第一方LOC。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 第一方代码基线、真实调用和26项候选/7批实施方案 | 已落 docs/reviews/2026-09-15-全仓代码消融/README.md；后续在本线status续作 |
