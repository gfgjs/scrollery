# -*- coding: utf-8 -*-
"""
对 export_clip_onnx.py 导出的图像编码器 ONNX 做「正确性校验 + 耗时/性能基准」。

直接服务于「测试不同轴的耗时和性能」：对同一模型分别导出 b1 / b4 / b8 / dyn 等变体后，
用本脚本逐个跑基准，对比单张延迟（latency）与吞吐（throughput, images/s）。

两件事：
  1) --check（可选，需加载原始 .pt，较慢）：io 契约 + 数值保真（ONNX vs PyTorch fp32，余弦应 ≥0.99）
     + 语义区分（不同文本余弦应明显 <0.95）。沿用 tools/validate_clip_l14_336_onnx.py 的三项检查。
  2) 基准（默认就做）：按模型声明的 batch（探测固定大小或用 --infer-batch 指定）反复推理图像编码器，
     报告 warmup 后的平均/中位延迟与吞吐。

EP（执行器）：默认按可用性自动选（DmlExecutionProvider > CUDA > CPU）；--provider 可强制。
注意本机当前 ORT 仅装了 CPU/Azure（App 运行时才有 DirectML）；GPU 基准请到带 DML 的环境跑。

用法示例（PowerShell）：
  # 先导出几个变体
  python .models/export_clip_onnx.py --arch ViT-L-14-336 --batch 1 --skip-text
  python .models/export_clip_onnx.py --arch ViT-L-14-336 --batch 8 --skip-text
  python .models/export_clip_onnx.py --arch ViT-L-14-336 --dynamic-batch --skip-text
  # 再分别基准
  python .models/bench_clip_onnx.py --arch ViT-L-14-336 --batch 1
  python .models/bench_clip_onnx.py --arch ViT-L-14-336 --batch 8
  python .models/bench_clip_onnx.py --arch ViT-L-14-336 --dynamic-batch --infer-batch 8
  # 带数值/语义校验
  python .models/bench_clip_onnx.py --arch ViT-B-16 --batch 1 --prec fp16 --check
"""

import argparse
import os
import time

import numpy as np
import onnxruntime as ort

from cn_clip.clip.utils import _MODEL_INFO, _MODELS

# 与 export_clip_onnx.py 共用同一套路径/命名规则（脚本在 .models/ 自身）。
MODELS_DIR = os.path.dirname(os.path.abspath(__file__))
CTX = 52


def derive_paths(arch: str):
    ckpt_name = _MODELS[arch][1]
    stem = ckpt_name.removesuffix(".pt")
    prefix = stem.removeprefix("clip_cn_")
    subdir = os.path.join(MODELS_DIR, stem)
    return subdir, os.path.join(subdir, ckpt_name), os.path.join(subdir, prefix)


def axis_tag(dynamic: bool, batch: int) -> str:
    return "dyn" if dynamic else f"b{batch}"


def pick_provider(requested: str | None) -> str:
    """选 EP：显式优先；否则 DML > CUDA > CPU（按当前 ORT 可用性）。"""
    avail = ort.get_available_providers()
    if requested:
        if requested not in avail:
            raise SystemExit(f"[ERROR] 请求的 EP {requested} 不可用；当前可用：{avail}")
        return requested
    for ep in ("DmlExecutionProvider", "CUDAExecutionProvider", "CPUExecutionProvider"):
        if ep in avail:
            return ep
    return "CPUExecutionProvider"


def declared_batch(sess: ort.InferenceSession) -> int | None:
    """读图像输入声明的 batch 维：整数→固定大小；str/-1→动态（返回 None）。"""
    dim0 = sess.get_inputs()[0].shape[0]
    return dim0 if isinstance(dim0, int) and dim0 > 0 else None


def run_check(arch: str, img_onnx: str, txt_onnx: str, provider: str, res: int):
    """正确性校验：io 契约 + 数值保真 + 语义区分（需原始 .pt 作为 fp32 基准）。"""
    import torch  # 仅 --check 时才引入（torch 加载较重）
    import cn_clip.clip as clip
    from cn_clip.clip.utils import create_model

    _, ckpt, _ = derive_paths(arch)
    with open(ckpt, "rb") as f:
        model = create_model(_MODEL_INFO[arch]["struct"], torch.load(f, map_location="cpu")).float().eval()

    sess_i = ort.InferenceSession(img_onnx, providers=[provider])
    in_i, out_i = sess_i.get_inputs()[0], sess_i.get_outputs()[0]
    print("== 1) io 契约 ==")
    print(f"  image: in='{in_i.name}'{in_i.shape}{in_i.type} -> out='{out_i.name}'{out_i.shape}")

    rng = np.random.default_rng(0)
    img = ((rng.random((1, 3, res, res), dtype=np.float32) - 0.5) / 0.5)
    with torch.no_grad():
        torch_img = model(torch.from_numpy(img), None).numpy()[0]
    onnx_img = sess_i.run(None, {in_i.name: img})[0][0]
    print("\n== 2) 数值保真：ONNX vs PyTorch(fp32)，余弦应 ≥0.99 ==")
    print(f"  embed_dim={onnx_img.shape[0]}    image torch~onnx = {_cos(torch_img, onnx_img):.4f}")

    if txt_onnx and os.path.isfile(txt_onnx):
        sess_t = ort.InferenceSession(txt_onnx, providers=[provider])
        in_t = sess_t.get_inputs()[0]
        texts = ["一只猫", "一辆红色的汽车", "雪山日落的风景", "a cute puppy dog"]
        toks = [clip.tokenize([t], context_length=CTX) for t in texts]
        with torch.no_grad():
            torch_txt = [model(None, tk).numpy()[0] for tk in toks]
        onnx_txt = [sess_t.run(None, {in_t.name: tk.numpy().astype(np.int64)})[0][0] for tk in toks]
        for t, a, b in zip(texts, torch_txt, onnx_txt):
            print(f"  text  torch~onnx = {_cos(a, b):.4f}   ({t})")
        print("\n== 3) 语义区分：不同文本余弦应明显 <0.95 ==")
        for i in range(len(texts)):
            for j in range(i + 1, len(texts)):
                print(f"  {_cos(onnx_txt[i], onnx_txt[j]):+.4f}   「{texts[i]}」 vs 「{texts[j]}」")


def _cos(a, b):
    a = a / (np.linalg.norm(a) + 1e-8)
    b = b / (np.linalg.norm(b) + 1e-8)
    return float(a @ b)


def run_bench(img_onnx: str, provider: str, res: int, infer_batch: int | None,
              warmup: int, iters: int):
    """图像编码器耗时基准：warmup 后计时 iters 次，报告每批延迟与每张吞吐。"""
    so = ort.SessionOptions()
    so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL  # 与 App 默认一致
    sess = ort.InferenceSession(img_onnx, sess_options=so, providers=[provider])
    in_name = sess.get_inputs()[0].name

    fixed = declared_batch(sess)
    if fixed is not None:
        if infer_batch and infer_batch != fixed:
            print(f"  [WARN] 模型 batch 钉死为 {fixed}，忽略 --infer-batch {infer_batch}")
        bs = fixed
    else:  # 动态轴：用 --infer-batch 决定喂多大批（默认 1）
        bs = infer_batch or 1

    rng = np.random.default_rng(0)
    x = ((rng.random((bs, 3, res, res), dtype=np.float32) - 0.5) / 0.5)

    for _ in range(warmup):
        sess.run(None, {in_name: x})

    ts = []
    for _ in range(iters):
        t0 = time.perf_counter()
        sess.run(None, {in_name: x})
        ts.append((time.perf_counter() - t0) * 1000.0)  # ms
    ts.sort()
    mean, med, p90 = sum(ts) / len(ts), ts[len(ts) // 2], ts[int(len(ts) * 0.9)]

    print(f"\n== 基准（{os.path.basename(img_onnx)}）==")
    print(f"  EP={provider}   batch轴={'fixed' if fixed is not None else 'dynamic'}   实际推理 batch={bs}")
    print(f"  warmup={warmup}  iters={iters}")
    print(f"  每批延迟  mean={mean:7.2f} ms   median={med:7.2f} ms   p90={p90:7.2f} ms")
    print(f"  每张延迟  mean={mean / bs:7.2f} ms   吞吐 ≈ {bs / (mean / 1000.0):7.1f} images/s")


def main():
    ap = argparse.ArgumentParser(description="CLIP 图像编码器 ONNX 正确性校验 + 耗时基准")
    ap.add_argument("--arch", required=True,
                    choices=["ViT-B-16", "ViT-L-14", "ViT-L-14-336", "ViT-H-14", "RN50"])
    bg = ap.add_mutually_exclusive_group()
    bg.add_argument("--dynamic-batch", action="store_true", help="基准/校验动态 batch 变体。")
    bg.add_argument("--batch", type=int, default=1, metavar="N", help="基准/校验固定 batch=N 变体（默认 1）。")
    ap.add_argument("--prec", default="fp32", choices=["fp16", "fp32"], help="精度后缀（默认 fp32）。")
    ap.add_argument("--provider", default=None, help="强制 EP（如 CPUExecutionProvider）。默认自动选。")
    ap.add_argument("--infer-batch", type=int, default=None, metavar="N",
                    help="动态轴时实际喂入的 batch 大小（默认 1）；固定轴时此项被忽略。")
    ap.add_argument("--warmup", type=int, default=3, help="计时前预热次数（默认 3）。")
    ap.add_argument("--iters", type=int, default=20, help="计时迭代次数（默认 20）。")
    ap.add_argument("--check", action="store_true", help="额外做数值/语义校验（需加载原始 .pt，较慢）。")
    args = ap.parse_args()

    subdir, _, prefix = derive_paths(args.arch)
    res = _MODEL_INFO[args.arch]["input_resolution"]
    tag = axis_tag(args.dynamic_batch, args.batch)
    img_onnx = f"{prefix}.img.{tag}.{args.prec}.onnx"
    txt_onnx = f"{prefix}.txt.{args.prec}.onnx"
    if not os.path.isfile(img_onnx):
        raise SystemExit(f"[ERROR] 找不到图像 ONNX：{img_onnx}\n  请先用 export_clip_onnx.py 导出对应变体。")

    provider = pick_provider(args.provider)
    if args.check:
        run_check(args.arch, img_onnx, txt_onnx, provider, res)
    run_bench(img_onnx, provider, res, args.infer_batch, args.warmup, args.iters)


if __name__ == "__main__":
    main()
