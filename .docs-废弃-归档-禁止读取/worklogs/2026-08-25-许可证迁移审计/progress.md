---
status: 已完成
type: 工作记忆
line: 许可证迁移审计
created: 2026-08-25
---

# 进度日志:许可证迁移审计

## 会话:2026-08-25
- 做了:读取项目级约束和 planning skill；检查工作树、根目录、docs 目录及关键构建/许可证文件索引。
- 验证:工作树为 `dev...origin/dev [ahead 16]`，存在大量已修改、删除和未跟踪文件；当前未执行迁移专项测试。
- 遗留:读取实际迁移 diff、许可证正文/归属材料、依赖元数据和发布脚本，随后运行可行验证。
- 做了:核对根许可证、README/CONTRIBUTING/CLA、Cargo/npm 元数据、Copybara 边界、Tauri resources、NOTICE 生成器、sidecar 构建脚本和当前 NSIS 产物。
- 发现:实际发货闭包包含被 NOTICE 排除的 ORT DLL 与独立 raw-worker/LibRaw；完整第三方文本未进入安装包；法律资源断言与实际 payload 断言脱节；私有 pro 构建复用 Community 法律资源；历史重许可权利无法由当前仓库独立证明。
- 追加发现:按需 FFmpeg 解压丢弃上游文档/许可材料；ONNX Runtime 二级 notices 未随 DLL 抽取；`SOURCE.md` 未跟踪且 verifier 不查源文件存在；release workflow 未直接跑 NOTICE freshness/SBOM 门。
- 验证:当前 NSIS 列表含 `LICENSE`/`NOTICE.md`/`SOURCE.md`；`generate-notice.mjs --check` 与 bundle verifier 通过，但 verifier 的覆盖不足已确认；前端测试、类型检查、Lint、Rust check 已通过。
- 验证:Rust `cargo test --workspace --locked` 完成，1094 passed、6 ignored，0 failed；官方 Mozilla/FFmpeg/BtbN/ONNX Runtime 资料用于核对许可文本与分发闭包语义。
- 遗留:补充最终证据和严重性排序，形成只读审计结论。

## 回顾(收口时填)
- 亮点:用实际构建脚本、独立 worker workspace、本地依赖源码和 NSIS 条目交叉核对，识别出 NOTICE 绿色检查没有覆盖真实发货闭包。
- 教训:根 LICENSE、lockfile 和现有状态文档都不能代表安装包合规；必须把“构建输入 → 实际条目 → 法律文本”作为一条可执行证据链。
- 意外:独立 `rsraw-sys` 的包级 MIT 元数据掩盖了内嵌 LibRaw 的 CDDL/LGPL；`onnxruntime-node` 的 devDependency 标记也与产品运行时 DLL 的实际发货用途相冲突。
