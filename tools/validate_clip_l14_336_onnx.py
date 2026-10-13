# -*- coding: utf-8 -*-
"""
验证导出的 ViT-L/14@336 ONNX（默认 fp16，可指定 fp32）是否正确可用。

三项检查：
  1. io 契约：输入名 image/text、输出名 unnorm_*、嵌入维度 768，与 profile 一致；
  2. 数值保真：ONNX(fp16) 输出 vs 原始 PyTorch(fp32) 模型输出，逐条余弦应 ≈ 1（≥0.99）；
  3. 语义区分：不同含义文本的余弦相似度应明显 < 0.95（相同文本应 = 1）。

用 onnxruntime（CPU，与 App 同款 ORT 1.26 加载 fp16 外部数据格式）跑推理。
"""

import argparse
import os
import numpy as np
import torch
import onnxruntime as ort

import cn_clip.clip as clip
from cn_clip.clip.utils import _MODEL_INFO, _MODELS, create_model

ROOT = os.path.dirname(os.path.abspath(__file__))
# 脚本位于 tools/ 子目录 → .models 在上一级（项目根）。
MODELS_DIR = os.path.join(os.path.dirname(ROOT), ".models")
CTX = 52

_ap = argparse.ArgumentParser(description="验证导出的 Chinese-CLIP ONNX")
_ap.add_argument("--arch", default="ViT-L-14-336",
                 choices=["ViT-B-16", "ViT-L-14", "ViT-L-14-336", "ViT-H-14", "RN50"])
_ap.add_argument("--suffix", default="fp16", help="精度后缀：fp16 / fp32")
_args = _ap.parse_args()

ARCH = _args.arch
RES = _MODEL_INFO[ARCH]["input_resolution"]
_ckpt_name = _MODELS[ARCH][1]                                       # clip_cn_vit-l-14.pt
_prefix = _ckpt_name.removeprefix("clip_cn_").removesuffix(".pt")   # vit-l-14
CKPT = os.path.join(MODELS_DIR, _ckpt_name)
IMG_ONNX = os.path.join(MODELS_DIR, f"{_prefix}.img.{_args.suffix}.onnx")
TXT_ONNX = os.path.join(MODELS_DIR, f"{_prefix}.txt.{_args.suffix}.onnx")


def cos(a, b):
    a = a / (np.linalg.norm(a) + 1e-8)
    b = b / (np.linalg.norm(b) + 1e-8)
    return float(a @ b)


def main():
    # ── 原始 PyTorch 模型（fp32，作为数值基准）──
    with open(CKPT, "rb") as f:
        ckpt = torch.load(f, map_location="cpu")
    model = create_model(_MODEL_INFO[ARCH]["struct"], ckpt).float().eval()

    sess_i = ort.InferenceSession(IMG_ONNX, providers=["CPUExecutionProvider"])
    sess_t = ort.InferenceSession(TXT_ONNX, providers=["CPUExecutionProvider"])
    in_i, out_i = sess_i.get_inputs()[0], sess_i.get_outputs()[0]
    in_t, out_t = sess_t.get_inputs()[0], sess_t.get_outputs()[0]

    print("== 1) io 契约 ==")
    print(f"  image: in='{in_i.name}'{in_i.shape}{in_i.type} -> out='{out_i.name}'{out_i.shape}")
    print(f"  text : in='{in_t.name}'{in_t.shape}{in_t.type} -> out='{out_t.name}'{out_t.shape}")

    texts = ["一只猫", "一辆红色的汽车", "雪山日落的风景", "a cute puppy dog"]
    toks = [clip.tokenize([t], context_length=CTX) for t in texts]

    with torch.no_grad():
        torch_txt = [model(None, tk).numpy()[0] for tk in toks]
        # 固定随机图，torch 与 onnx 喂同一张，比对数值
        rng = np.random.default_rng(0)
        img = ((rng.random((1, 3, RES, RES), dtype=np.float32) - 0.5) / 0.5)
        torch_img = model(torch.from_numpy(img), None).numpy()[0]

    onnx_txt = [sess_t.run(None, {in_t.name: tk.numpy().astype(np.int64)})[0][0] for tk in toks]
    onnx_img = sess_i.run(None, {in_i.name: img})[0][0]

    print("\n== 2) 数值保真：ONNX(fp16) vs PyTorch(fp32)，逐条余弦应 ≥ 0.99 ==")
    print(f"  embed_dim: image={onnx_img.shape[0]}, text={onnx_txt[0].shape[0]} (期望 768)")
    print(f"  image  torch~onnx = {cos(torch_img, onnx_img):.4f}")
    for t, a, b in zip(texts, torch_txt, onnx_txt):
        print(f"  text   torch~onnx = {cos(a, b):.4f}   ({t})")

    print("\n== 3) 语义区分：不同文本余弦应明显 < 0.95；相同文本 = 1.0 ==")
    for i in range(len(texts)):
        for j in range(i + 1, len(texts)):
            print(f"  {cos(onnx_txt[i], onnx_txt[j]):+.4f}   「{texts[i]}」 vs 「{texts[j]}」")
    same = sess_t.run(None, {in_t.name: toks[0].numpy().astype(np.int64)})[0][0]
    print(f"  {cos(onnx_txt[0], same):+.4f}   「{texts[0]}」 vs 自身（应≈1.0）")


if __name__ == "__main__":
    main()
