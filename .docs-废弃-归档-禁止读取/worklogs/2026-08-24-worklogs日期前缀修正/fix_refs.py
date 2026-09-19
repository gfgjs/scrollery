#!/usr/bin/env python3
"""按 rename-map.tsv 将文件内 '2026-08-14-<任务名>' 引用替换为 '<创建日>-<任务名>'。
二进制字节级替换,不动换行与编码;长名优先,避免「全仓深度review」误吃「全仓深度review与直修」。"""
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
pairs = []
for line in (HERE / "rename-map.tsv").read_text(encoding="utf-8").splitlines():
    line = line.rstrip("\n")
    if not line.strip() or line.startswith("#"):
        continue
    old, new = line.split("\t")
    pairs.append((old.strip(), new.strip()))
pairs.sort(key=lambda p: -len(p[0]))

for arg in sys.argv[1:]:
    p = Path(arg)
    data = p.read_bytes()
    total, hits = 0, []
    for old, new in pairs:
        ob, nb = old.encode("utf-8"), new.encode("utf-8")
        n = data.count(ob)
        if n:
            data = data.replace(ob, nb)
            total += n
            hits.append(f"{old}->{new} x{n}")
    if total:
        p.write_bytes(data)
        print(f"[updated] {arg}: {total} 处({'; '.join(hits)})")
    else:
        print(f"[skip] {arg}: 0")
