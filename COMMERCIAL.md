# Scrollery Commercial Licensing

<!-- 商业授权说明。本文件为路线与边界说明,不是 EULA,不构成许可授予、报价或要约。 -->

Scrollery's first-party source code is released under the **GNU Affero General
Public License, version 3 only (AGPL-3.0-only)**. The complete license text is
in [LICENSE](LICENSE). A [limited additional permission](ADDITIONAL-PERMISSION.md)
covers the bundled Graphviz 2.40.1 / Viz.js 2.1.2 combination; it does not grant
general closed-source integration rights or change the commercial offerings.

The project plans two separate commercial offerings. They cover different
scopes and are not interchangeable:

- **Official end-user distribution.** The official stable release is planned as
  a paid, one-time purchase under an end-user commercial license. Individuals
  and organizations buy through this channel when they want the official signed
  build, its updates, and its support rather than building from source. One
  perpetual official-edition activation covers advanced image editing, OCR, image
  enhancement, and the PSD engine. Each feature still requires supported platforms
  and its runtime resources; image enhancement is not yet ready because release
  model assets have not been delivered. RAW decoding and video format extensions
  remain free.
- **Enterprise / OEM licensing.** For organizations that need to ship
  first-party Scrollery code under terms the AGPL cannot support — closed-source
  integration, white-label, or embedded distribution — the alternative is a
  negotiated license.

How each offering is packaged and priced is settled when it is offered. This
document describes boundaries only: it is not an offer, a quotation, or a
license agreement, and it grants no rights by itself.

## What the AGPL grants

For source or builds you obtain under the AGPL — including ones you compile
yourself and ones you lawfully obtain from a third party — the license grants
these rights directly, with no purchase and no separate agreement:

- use the software commercially, at work or in a paid engagement;
- charge for services of your own that use the software;
- install and run it on your own machines;
- modify it for your own use;
- redistribute the original or your modified version under the AGPL.

These are AGPL rights and they carry AGPL obligations. A modified version that
other people interact with remotely over a network — including over a company
LAN or an internal service — may trigger the section 13 obligation to offer
those users the Corresponding Source, and modifications kept inside your
organization still have to comply with every applicable term of the license.
The list above summarizes what the license grants; it is not an exemption from
what the license requires.

## How a commercial license takes effect

A commercial license exists only when it is separately agreed, by either:

- accepting the end-user commercial license terms presented with the official
  purchase; or
- signing a negotiated agreement covering the enterprise / OEM scope.

Either route is sufficient; nothing here requires a bilateral signed paper
contract for each end-user purchase. Until the relevant terms are accepted or
signed, no commercial license is in effect.

A negotiated agreement must state at least which first-party components are
covered, whether the scope is the official end-user channel or enterprise / OEM
integration, the term and territory, and the fees and payment terms.

## What a commercial license does not do

- It cannot relicense **third-party components**. Each dependency keeps its own
  license; those notices are listed in [NOTICE.md](NOTICE.md).
- It does not narrow anyone else's AGPL rights, or restrict rights you already
  hold under the AGPL for source you obtained publicly.
- It does not grant trademark rights. See [TRADEMARK.md](TRADEMARK.md).
- It does not withdraw the AGPL release of code that has been published under
  the AGPL.

## Availability today

There is no self-serve checkout, no published price list, and no live online
end-user license agreement or payment flow. Commercial terms are settled
individually when a concrete need arises. The application currently displays
"Purchasing is not yet available". Activation uses one official-edition token;
uninstalling a plugin preserves that license, and removing the local license
disables the included paid features without deleting plugins, models, or files.

## Scope of the AGPL release

The first-party source published in this repository is licensed under
AGPL-3.0-only. As AGPL-3.0 section 8 provides, the rights granted under the
license are not revocable by the project while you comply with the license, and
the project will not withdraw the AGPL release of code published under it as a
licensing decision; the license does terminate if you violate its terms. Releases
published earlier under the Mozilla Public License 2.0 remain available under
the terms they were released with. Until a commercial license is agreed, the
AGPL, together with that limited additional permission, is the licensing basis
for the first-party code published under it here; this document does not state
the license of future versions of the software.

## Future professional components

Independent professional components are planned to be licensed separately. They
are not part of the AGPL-licensed source tree and carry no open-source
commitment in advance. Whether and when such components ship is a roadmap
question, not a licensing promise.

---

# Scrollery 商业授权说明

<!-- 中文对照;与上文章节内容一致,语义以中英文对照为准。 -->

Scrollery 的第一方源码以 **GNU Affero General Public License 第 3 版(仅限第 3
版,AGPL-3.0-only)** 发布,完整正文见 [LICENSE](LICENSE)。另附[限定附加许可](ADDITIONAL-PERMISSION.md),仅处理随附 Graphviz 2.40.1 / Viz.js 2.1.2 的组合,不授予一般闭源集成权,不改变商业产品方案。

项目计划两类彼此独立的商业产品,范围不同、不可互相替代:

- **官方终端用户发行**:官方稳定版计划以**一次性买断**方式,按终端用户商业许可提供。个人与组织如需官方签名构建、更新与支持,可选择该渠道,而非自行构建。
- **企业／OEM 授权**:组织若需在 AGPL 无法支持的方式下分发第一方 Scrollery 代码——闭源集成、贴牌或嵌入发行——则以协商授权作为替代方案。

两类产品如何打包与定价,均在实际提供时确定。本文件只界定边界:不是要约、报价或授权协议,本身不授予任何权利。

**AGPL 已授予的权利。** 对于你依 AGPL 取得的源码或构建——包括自行编译的,以及合法从第三方取得的——许可直接授予以下权利,无需购买、无需另行签署协议:在工作或付费项目中将软件用于商业用途;就使用本软件的自有服务收费;在你自己的设备上安装运行;为自己的用途修改;依照 AGPL 再分发原始版本或你的修改版。

这些是 AGPL 的权利,同时附带 AGPL 的义务。若修改版由他人通过网络远程交互——包括经公司局域网或内部服务交互——可能触发第 13 条向这些用户提供对应源码的义务;即便修改仅在组织内部使用,也仍须遵守许可的全部适用条款。上述列表只是对许可授予内容的概括,不是对许可要求的豁免。

**商业授权如何生效。** 商业授权仅在另行达成约定时成立,以下任一方式即可:在官方购买时接受随附的终端用户商业许可条款;或签署覆盖企业／OEM 范围的协商协议。两种方式均可,并不要求每个终端用户买断都签署双边纸质合同。在相应条款被接受或签署之前,不产生商业授权。

协商协议至少应写明:覆盖哪些第一方组件;范围属官方终端用户渠道还是企业／OEM 集成;期限与地域;费用与付款条件。

**商业授权不能做的事:** 不能改变第三方组件的许可,各依赖保持自身许可,声明见 [NOTICE.md](NOTICE.md);不缩小他人的 AGPL 权利,也不限制你通过公开渠道已依 AGPL 取得的权利;不授予商标权,见 [TRADEMARK.md](TRADEMARK.md);不撤回已按 AGPL 发布的代码在 AGPL 下的发布。

**当前可用性:** 目前没有自助购买流程、没有公开价目表,也没有在线的终端用户许可协议或支付闭环。商业条款在出现具体需求时逐案确定。

**AGPL 发布的范围:** 本仓库当前发布的第一方源码以 AGPL-3.0-only 授权。如 AGPL-3.0 第 8 条所定,只要你遵守许可,依本许可取得的权利不因项目改变许可决策而被撤销,项目也不会撤回已按 AGPL 发布的代码在 AGPL 下的发布;若你违反许可条款,许可将终止。此前以 Mozilla Public License 2.0 发布过的版本仍按其发布时的条款提供。在商业授权达成之前,对这里以 AGPL 发布的第一方代码而言,AGPL 及上述限定附加许可构成生效的授权基础;本文件不代表软件未来版本的许可。

**未来专业组件:** 计划单独授权,不属于 AGPL 源码树,不预先承诺开源;其是否发布、何时发布属于路线图问题,不是授权承诺。
