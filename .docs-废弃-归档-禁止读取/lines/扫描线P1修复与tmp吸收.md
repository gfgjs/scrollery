---
id: 2026-08-23-扫描线P1修复与tmp吸收
status: active
type: line
line: 扫描线P1修复与tmp吸收
created: 2026-08-23
---

# 扫描线P1修复与tmp吸收

使命:对 2026-08-21 入库扫描元数据性能线做双实现对照评审(scrollery-tmp-23a5679 同题镜像),修复其 P1 跨根并发误标缺陷(seen 表按根隔离+用后销毁),并吸收对照线优秀实现(富化队列表、walker 单次 stat、XMP 首字节定位);主体已收口,余项滚动见 [status](../status/扫描线P1修复与tmp吸收.md)。
