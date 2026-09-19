---
id: 2026-09-14-graphviz-permission-draft
status: snapshot
type: review
line: 渠道与开源边界
created: 2026-09-14
---

# Graphviz 限定附加许可草案（2026-09-14 用户已批准）

> 用户明确选择“批准限定附加许可，保留功能”。下方草案为审批时文本；正式授权已落根目录 ADDITIONAL-PERMISSION.md，当前状态以正式文件为准。

## 需要裁定的原因

现有前端分发 Viz.js 2.1.2 编译的 Graphviz 2.40.1（EPL-1.0）。单独文件、按需加载或 EPL 允许目标码分发，不足以直接证明整个应用属于 AGPL 的独立聚合例外。Eclipse 官方 FAQ 第32项明确描述 EPL/GPL 组合及链接不兼容边界：https://www.eclipse.org/legal/epl/faq/ 。主会话不接受仅补 NOTICE 即宣布组合兼容的结论。

用户已选择标准 AGPL-3.0-only + 商业授权；以下附加许可会增加一项限定权限，须版权人明确决定。根 LICENSE 标准正文保持不变；若批准，另置公开附加许可文件并同步声明与元数据。当前文件仅为可审阅提案，不授予权限，不作为发布放行证据。

## 建议条款

Additional permission under GNU AGPL version 3, section 7

The copyright holders of Scrollery grant you additional permission to combine the Scrollery code they license under the GNU Affero General Public License, version 3, with Graphviz version 2.40.1 as included in Viz.js version 2.1.2, and to convey the resulting combination. Graphviz remains subject to the Eclipse Public License, version 1.0; this permission does not relicense Graphviz or any other third-party component.

You must comply with the GNU Affero General Public License, version 3, for the Scrollery code and with the applicable licenses of all third-party components. When conveying this combination in object code form, the Corresponding Source you provide must also include the source code and build scripts for the Graphviz and Viz.js components used in the combination.

This additional permission applies only to code whose copyright holders have granted it. It does not grant permission on behalf of other copyright holders. Modifiers may extend this permission to their own modifications, but are not required to do so; recipients may remove this additional permission as provided by section 7 of the GNU Affero General Public License, version 3.

## 效果与实施条件

- 保留现有 Graphviz 渲染功能，第一方其余 AGPL 源码及网络服务义务不变，商业授权方案不变。
- 仅覆盖上述指定 Graphviz/Viz.js 组合；不授予任意闭源链接或任意 EPL 组件的组合权。
- 批准后才写入公开文件；仍须核实参与组合的第三方许可证、补对应源码/构建配方及法律材料并验证。
- 替代方案：移除 Graphviz 载荷和功能，或另做隔离/替换方案；后者涉及实现调整与额外验证，不在本草案中自动执行。
