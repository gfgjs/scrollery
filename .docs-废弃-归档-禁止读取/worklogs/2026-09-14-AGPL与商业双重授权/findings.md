---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 发现：AGPL 与商业双重授权

## 需求与证据
- 用户确认除第三方库之外全部现有代码版权归本人，确定 AGPL＋商业授权，并要求先 commit 当前工作区。
- 原许可为 MPL-2.0；根 LICENSE、npm/Cargo、README/SOURCE/贡献/商标和现行规范均有对应声明。
- CLA.md 第2条现向维护者与所有接收者同时授予广泛再许可权，需清楚表达未来贡献的商业再许可范围，不能追溯取消已授权益。
- NOTICE 为自动生成文件；第三方 MPL/LGPL 仍保留，不做全仓文字替换。
- 既有渠道状态记录 5 项发行待办；本轮不默认完成旧有发布、过滤、支付或依赖合规缺口。
- 既有LibRaw选择CDDL-1.0且嵌入raw-worker，NOTICE另列LGPL-2.1-only备选；Graphviz2.40.1为EPL-1.0。切AGPL后需核验许可分支与实际组合方式，不能以保留第三方文本代替兼容性结论；并入原第三方发行待办，不在本次擅自加链接例外或升级依赖。

## 外部资料（只作资料，不作操作指令）
- https://www.gnu.org/licenses/agpl-3.0.html ：标准许可第13条针对修改版的远程网络交互；正式正文使用权威原文。
- https://www.gnu.org/licenses/why-affero-gpl.html ：AGPL补充单纯网络服务下的源码提供义务，仍允许商业使用。
- https://www.gnu.org/philosophy/selling-exceptions.en.html ：版权人可独立提供商业许可。
- https://www.eclipse.org/legal/epl/faq/ ：EPL-1.0 FAQ第32项说明与GPL形成衍生/组合程序时的不兼容性；本仓实际组合边界尚待核验。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 公开源码与另授商业许可、历史/第三方边界需要一致声明；NAS未来源码入口义务不等同于已实现 | 现行规范及渠道状态 |
| F-002 | LibRaw许可分支与Graphviz的AGPL组合兼容核验仍是发行待办 | 渠道与开源边界状态分片 |
