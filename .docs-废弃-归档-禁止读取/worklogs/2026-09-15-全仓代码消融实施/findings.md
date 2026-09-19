---
status: snapshot
type: working-memory
line: 全仓代码消融
created: 2026-09-15
---

# 剪枝记录

基线：e7db489b，264655 LOC/927第一方文件、261 IPC、79设置/19声明状态键。源码终态31cab793：254057 LOC/906文件、232 IPC、77设置/36声明状态及UI偏好键（17个原隐式阅读器键纳入）。

| 分类 | 实际内容 |
|---|---|
| 已删除 | 无消费者定位/导航/状态包装与VectorStore；29个旧IPC及孤立去重/搜索闭包；历史升级链/旧配置兼容/两死列；空渠道、空授权字段与第三方闲置资源 |
| 已合并 | direct唯一分发与catalog显式worker来源；画廊统一bucket；单份当前DDL；配置与状态清单封闭路由 |
| 已简化 | StorageBackend降为连接测试函数；布局来源直接装配；单选择模式直接导入；基准直接调用现行缩略图底层入口 |
| 已保留 | P12 H-Lab、DOM/Canvas、选择语义解耦、当前安全/并发/持久化保护；P25因解码/色彩语义不同且再包装无收益保留 |
| 待确认 | 真机跨平台GUI、原生编辑器装配等动态证据；不等待历史数据许可，不恢复已删兼容 |

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-101 | 分批实际删减、验证与量化结果 | reviews实施报告与本线status |

- 已删除（批1b）：休眠VectorStore簇、恒None激活payload/enc_seed、旧AI后端日志路径、Float空类型及重复restart_required元数据。
- 已简化（批1b后续终态）：三个交付源包装及InstallSource枚举均已删除；唯一direct交付，实际签名与凭据边界保留。
- P21已验证资源层：删除162文件/7862271字节；锁定Vditor同版本UMD浏览器fixture触发27 URL、37请求，无意外404/外部回落/页面错误；公式、高亮、Mermaid、Graphviz及method.min.js打印同构链渲染成功。未测试Vite装配和Tauri工具栏端到端；不把此证据当原生GUI验收。

- P25等价性限制：generator::maybe_write_ai_cache消费已解码pixels并在缩放后显式project_rgba8_to_srgb；derive::image::write_short_edge_webp从原图解码后直接编码，输入与色彩投影不同。不能为了剪枝统一解码/像素处理或顺带做ICC修复；只允许实际等价且有净收益的编码/发布尾段，否则明确保留。
- P23当前边界复核：db::boot在恢复失败时drop写连接→回滚old→重开是Windows句柄和数据一致性真实要求，删除迁移不能连带删除；restore暂存库可改为仅校验当前格式，去掉needs_migration和暂存升级。DDL现按early/mid/late重导出，最终要直接当前DDL，不能把33段合并文件冒充消融。
