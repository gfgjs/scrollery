# -*- coding: utf-8 -*-
"""
通用 Chinese-CLIP `.pt` → ONNX 导出脚本（适配 .models/ 子文件夹结构 + 可配置 batch 轴）。

本脚本由 tools/export_clip_l14_336_onnx.py 泛化而来，主要差异：

  1. 适配新的 .models/ 目录结构 —— 每个模型在自己的子文件夹下：
         .models/clip_cn_<arch>/clip_cn_<arch>.pt   (+ huggingface 的 config.json)
     产物（.onnx + .extra_file）也落回**同一个子文件夹**。

  2. batch 轴可由命令行配置，便于基准测试不同轴的耗时/性能（见 bench_clip_onnx.py）：
         --dynamic-batch      图像塔 batch 轴 = 动态（接受任意批；强 GPU 整批推理才划算）
         --batch N （默认 1）  图像塔 batch 轴 = 固定 N（ORT 静态形状优化更充分，单/定批更快）
     文本塔 batch **始终固定 = 1**（查询是单条），空间维 S×S、序列长 52 也始终固定（契约要求）。

  3. 产物文件名内嵌 batch 配置，多变体可共存对比：
         <prefix>.img.dyn.<prec>.onnx      （动态 batch）
         <prefix>.img.b<N>.<prec>.onnx     （固定 batch=N，如 b1 / b4 / b8）
         <prefix>.txt.<prec>.onnx          （文本塔，batch 恒为 1，无标签）
     其中 <prefix> = 去掉 clip_cn_ 前缀的 arch（如 vit-l-14-336），<prec> = fp16/fp32。

  4. --arch 选单个模型；--all 批量导出 .models/ 下所有**已存在 .pt** 的模型。

  5. 精度：默认 **fp32**（cn-clip 的 BERT 文本编码器在 fp16 下数值塌缩，见踩坑 #6）；
     --fp16 走「转 fp16 + 加载/数值双验证 + 不达标自动回退 fp32」流程（轻量化场景如 B/16 可试）。

契约（详见 src-tauri/src/ai/clip.rs / profile.rs）：
  - 图像输入名 "image" [N,3,S,S] → 输出 "unnorm_image_features" [N,768/512]（未归一化，Rust 端再 L2）
    batch 轴 N 由本脚本参数决定固定/动态；Rust 端 image_input_fixed_batch 探测：固定→分块、动态→整批。
  - 文本输入名 "text" [1,52] → 输出 "unnorm_text_features" [1,768/512]（batch 固定 1）。

几个关键踩坑（原样保留，按出现顺序）：
  1. 官方 cn_clip.deploy 的 fp16 走 onnxmltools，其同名入口在新版已退化为抛异常的桩
     → 这里改用 onnxruntime 自带的 convert_float_to_float16（正确处理视觉模型里的 Cast 节点）。
  2. torch 2.12 默认 dynamo 导出器不能稳妥处理 (None, text) / (image, None) 占位入参
     → dynamo=False 强制旧 TorchScript 路径。
  3. torch 2.12 把注意力融合成 aten::scaled_dot_product_attention（需 opset≥14）。
  4. **opset 必须 ≥17**：opset14 把 LayerNorm 拆成 ReduceMean/Sqrt/... 原语，转 fp16 后 ORT 的
     SimplifiedLayerNormFusion 会因中间 Cast 崩溃；opset17 导成单个 LayerNormalization 算子规避。
  5. torch 旧导出器把上 GB 的 fp32 图一次性写盘，在 Windows 触发 [Errno 22]
     → 改为导出到内存 BytesIO，再用 onnx 外部数据保存（按块写盘）。
  6. fp16 产物必须双重验证：① ORT「默认全套图优化」能建会话+推理；② 与原始 PyTorch(fp32) 输出
     逐条余弦 ≥0.99。CLIP 的 BERT 文本编码器在 fp16 下数值不稳（注意力/softmax/LayerNorm 溢出），
     会「能加载但算错」（所有文本塌缩成几乎相同向量）。任一验证不过 → 该编码器回退 fp32。

依赖（全局 Python311 已装齐）：
  torch 2.12 / torchvision / cn_clip 1.6 / onnx 1.21 / onnxruntime 1.26 / onnxscript / six

用法示例（PowerShell，在仓库根或任意目录均可，脚本自解析自身位置）：
  python .models/export_clip_onnx.py --arch ViT-L-14-336                 # 固定 batch=1, fp32
  python .models/export_clip_onnx.py --arch ViT-L-14-336 --batch 8       # 固定 batch=8, fp32
  python .models/export_clip_onnx.py --arch ViT-L-14-336 --dynamic-batch # 动态 batch, fp32
  python .models/export_clip_onnx.py --arch ViT-B-16 --fp16              # B/16 fp16（带回退）
  python .models/export_clip_onnx.py --all --batch 4                     # 所有已存在模型, 固定 batch=4
  python .models/export_clip_onnx.py --arch ViT-L-14 --batch 8 --skip-text  # 只导图像塔（测 batch 用）
"""

import argparse
import io
import os
import sys

import numpy as np
import torch
import onnxruntime as ort

import cn_clip.clip as clip
from cn_clip.clip.utils import _MODEL_INFO, _MODELS, create_model

from onnx import load_model_from_string, save_model
# ORT 自带 fp16 转换器：正确处理视觉模型里的 Cast 节点（onnxconverter_common 不行）。
from onnxruntime.transformers.float16 import convert_float_to_float16

# 脚本现位于 .models/ 自身 → 模型根就是脚本所在目录。
MODELS_DIR = os.path.dirname(os.path.abspath(__file__))

CONTEXT_LENGTH = 52
OPSET = 17  # 见文件头踩坑 #3/#4（必须 ≥17）

# 本仓库目前持有的 5 个规格（与 cn_clip 的 _MODELS / _MODEL_INFO 键一致）。
ARCH_CHOICES = ["ViT-B-16", "ViT-L-14", "ViT-L-14-336", "ViT-H-14", "RN50"]


def derive_paths(arch: str):
    """按 cn_clip 的 _MODELS 映射 + 新的子文件夹结构，推导 (子目录, checkpoint 路径, 产物名前缀)。

    例：ViT-L-14-336
        ckpt_name = clip_cn_vit-l-14-336.pt
        子目录    = .models/clip_cn_vit-l-14-336/        （= ckpt 去掉 .pt 的 stem）
        checkpoint= .models/clip_cn_vit-l-14-336/clip_cn_vit-l-14-336.pt
        产物前缀  = .models/clip_cn_vit-l-14-336/vit-l-14-336   （去掉 clip_cn_）
    """
    ckpt_name = _MODELS[arch][1]                          # clip_cn_vit-l-14-336.pt
    stem = ckpt_name.removesuffix(".pt")                  # clip_cn_vit-l-14-336（= 子文件夹名）
    prefix = stem.removeprefix("clip_cn_")                # vit-l-14-336
    subdir = os.path.join(MODELS_DIR, stem)
    return subdir, os.path.join(subdir, ckpt_name), os.path.join(subdir, prefix)


def axis_tag(dynamic: bool, batch: int) -> str:
    """图像塔 batch 配置 → 文件名标签：动态用 'dyn'，固定用 'b<N>'（b1 / b4 / b8 …）。"""
    return "dyn" if dynamic else f"b{batch}"


def _trace_to_proto(model, args, input_names, output_names, fold: bool, dynamic_axes=None):
    """用旧版 TorchScript 导出器把模型追踪到内存 ONNX proto（绕开 Windows 大文件写盘 bug）。
    dynamic_axes：可把指定张量的某些轴标记为动态（如图像塔的 batch 轴）。"""
    buf = io.BytesIO()
    torch.onnx.export(
        model, args, buf,
        input_names=input_names, output_names=output_names,
        dynamic_axes=dynamic_axes,
        export_params=True, do_constant_folding=fold, opset_version=OPSET,
        verbose=False, dynamo=False,  # dynamo=False：见文件头踩坑 #2
    )
    return load_model_from_string(buf.getvalue())


def _save_external(model, path: str) -> None:
    """外部数据格式保存（小 .onnx 头 + 同名 .extra_file 权重）；onnx 按块写盘，规避大文件单写 bug。"""
    # 必须连同旧 .extra_file 一并删除！onnx 写外部数据用**追加模式('ab')**，残留旧权重文件会被
    # 追加而非覆盖 → 体积正好翻倍，且 header 偏移指向新追加段、模型仍能加载（极隐蔽）。
    for p in (path, path + ".extra_file"):
        if os.path.exists(p):
            os.remove(p)
    save_model(
        model, path,
        location="{}.extra_file".format(os.path.basename(path)),
        save_as_external_data=True, all_tensors_to_one_file=True,
        size_threshold=1024, convert_attribute=True,
    )


def _cos(a, b):
    a = a / (np.linalg.norm(a) + 1e-8)
    b = b / (np.linalg.norm(b) + 1e-8)
    return float(a @ b)


def _fp16_ok(path: str, feeds: list, refs: list) -> bool:
    """双重验证 fp16：ORT 默认优化能建会话+推理，且每条输出（取首行）与 PyTorch 参考余弦 ≥0.99。"""
    try:
        sess = ort.InferenceSession(path, providers=["CPUExecutionProvider"])
        for feed, ref in zip(feeds, refs):
            out = sess.run(None, feed)[0][0]
            c = _cos(out, ref)
            if c < 0.99:  # 数值塌缩/溢出 → fp16 不可用（CLIP 文本编码器尤甚）
                print(f"    [WARN] fp16 数值偏差过大（与 torch 余弦={c:.4f}<0.99）→ 回退 fp32")
                return False
        return True
    except Exception as e:  # noqa: BLE001
        print(f"    [WARN] fp16 在 ORT 默认优化下不可加载 → 回退 fp32：{str(e)[:140]}")
        return False


def _emit(proto_fp32, base_no_ext: str, prefer_fp16: bool, feeds: list, refs: list) -> str:
    """落地一个编码器：默认直接 fp32；prefer_fp16 时优先 fp16（加载+数值双验证），不达标回退 fp32。

    base_no_ext：不含精度后缀与扩展名的完整路径前缀（如 .../vit-l-14-336.img.b8）。
    返回最终精度后缀 'fp16' / 'fp32'（用于打印与文件名）。"""
    fp16_path = f"{base_no_ext}.fp16.onnx"
    fp32_path = f"{base_no_ext}.fp32.onnx"

    def _rm(path):
        for p in (path, path + ".extra_file"):
            if os.path.exists(p):
                os.remove(p)

    if not prefer_fp16:
        _rm(fp16_path)               # 清掉可能残留的旧 fp16 变体，避免误用
        _save_external(proto_fp32, fp32_path)
        return "fp32"

    # keep_io_types=True：io 仍 fp32（Rust 端继续喂/取 f32），只把内部权重/计算转 fp16。
    model_fp16 = convert_float_to_float16(proto_fp32, keep_io_types=True)
    _save_external(model_fp16, fp16_path)
    if _fp16_ok(fp16_path, feeds, refs):
        _rm(fp32_path)
        return "fp16"

    _rm(fp16_path)                   # 回退：删掉不可用的 fp16，落 fp32（精确、必定可加载）
    _save_external(proto_fp32, fp32_path)
    return "fp32"


def export_one(arch: str, dynamic: bool, batch: int, prefer_fp16: bool, skip_text: bool) -> None:
    """导出单个模型的图像/文本编码器。batch/dynamic 仅作用于图像塔；文本塔恒为 batch=1。"""
    subdir, ckpt_path, save_prefix = derive_paths(arch)
    tag = axis_tag(dynamic, batch)
    img_batch = 1 if dynamic else batch  # 动态用 1 张追踪（接受任意批）；固定用 N 张追踪并钉死

    print(f"\n========== {arch}  (img batch={'dynamic' if dynamic else batch}, "
          f"prefer_fp16={prefer_fp16}) ==========")
    if not os.path.isfile(ckpt_path):
        print(f"[SKIP] 找不到 checkpoint：{ckpt_path}")
        return

    print(f"[1/4] 加载 checkpoint：{ckpt_path}")
    with open(ckpt_path, "rb") as f:
        checkpoint = torch.load(f, map_location="cpu")  # 默认 weights_only=True，安全

    print(f"[2/4] 构建并恢复模型（arch={arch}）")
    struct = _MODEL_INFO[arch]["struct"]
    res = _MODEL_INFO[arch]["input_resolution"]
    model = create_model(struct, checkpoint).float().eval()

    # 占位输入（仅供 onnx 追踪图）。图像按 img_batch 张追踪。
    blank = torch.zeros(img_batch, 3, res, res, dtype=torch.float32)
    text0 = clip.tokenize([""], context_length=CONTEXT_LENGTH)  # int64 [1,52]

    # 验证用真实输入 + PyTorch(fp32) 参考输出（仅 --fp16 时实际用到；必须在 del model 前算好）。
    val_texts = ["一只猫", "一辆红色的汽车", "雪山日落的风景", "a cute puppy dog"]
    val_toks = [clip.tokenize([t], context_length=CONTEXT_LENGTH) for t in val_texts]
    rng = np.random.default_rng(0)
    val_img = ((rng.random((img_batch, 3, res, res), dtype=np.float32) - 0.5) / 0.5)

    print("[3/4] 追踪编码器到内存 proto" + ("（+ 计算 PyTorch 参考输出）" if prefer_fp16 else ""))
    with torch.no_grad():
        # 图像塔 batch 轴：动态→标记 {0:'batch'}；固定→不设 dynamic_axes，追踪形状即被钉死。
        img_dyn = ({"image": {0: "batch"}, "unnorm_image_features": {0: "batch"}}
                   if dynamic else None)
        img_proto = _trace_to_proto(
            model, (blank, None), ["image"], ["unnorm_image_features"], fold=False,
            dynamic_axes=img_dyn,
        )
        img_ref = model(torch.from_numpy(val_img), None).numpy()[0] if prefer_fp16 else None

        if not skip_text:
            txt_proto = _trace_to_proto(
                model, (None, text0), ["text"], ["unnorm_text_features"], fold=True)
            txt_refs = ([model(None, tk).numpy()[0] for tk in val_toks]
                        if prefer_fp16 else None)
    del model  # 释放 fp32 torch 模型，给 fp16 转换腾内存

    print("[4/4] 落地 ONNX" + ("（fp16 + 加载/数值双验证，不达标回退 fp32）" if prefer_fp16 else "（fp32）"))
    img_feeds = [{"image": val_img}] if prefer_fp16 else []
    img_base = f"{save_prefix}.img.{tag}"
    prec_img = _emit(img_proto, img_base, prefer_fp16, img_feeds, [img_ref] if prefer_fp16 else [])

    prec_txt = None
    if not skip_text:
        txt_feeds = ([{"text": tk.numpy().astype(np.int64)} for tk in val_toks]
                     if prefer_fp16 else [])
        prec_txt = _emit(txt_proto, f"{save_prefix}.txt", prefer_fp16, txt_feeds,
                         txt_refs if prefer_fp16 else [])

    # ── 汇报产物 ──
    print(f"\n[DONE {arch}] 产物（{os.path.relpath(subdir, MODELS_DIR)}/）：")
    bases = [f"{save_prefix}.img.{tag}.{prec_img}.onnx"]
    if prec_txt is not None:
        bases.append(f"{save_prefix}.txt.{prec_txt}.onnx")
    for base in bases:
        for p in (base, base + ".extra_file"):
            if os.path.exists(p):
                print(f"  {os.path.basename(p):44s} {os.path.getsize(p)/(1024*1024):8.1f} MB")
    print(f"  图像编码器精度: {prec_img}" + (f"    文本编码器精度: {prec_txt}" if prec_txt else "  （已跳过文本塔）"))


def main() -> None:
    ap = argparse.ArgumentParser(
        description="通用 Chinese-CLIP .pt → ONNX 导出（可配置 batch 轴；默认 fp32，--fp16 带回退）。")
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--arch", choices=ARCH_CHOICES, help="导出单个模型规格。")
    g.add_argument("--all", action="store_true",
                   help="导出 .models/ 下所有**已存在 .pt** 的模型（共用同一 batch/精度配置）。")

    bg = ap.add_mutually_exclusive_group()
    bg.add_argument("--dynamic-batch", action="store_true",
                    help="图像塔 batch 轴 = 动态（接受任意批，强 GPU 整批推理才划算；喂单张反而更慢）。")
    bg.add_argument("--batch", type=int, default=1, metavar="N",
                    help="图像塔 batch 轴 = 固定 N（默认 1）。固定形状下 ORT 内存规划/算子特化更充分。")

    ap.add_argument("--fp16", action="store_true",
                    help="尝试 fp16（转 fp16 + 加载/数值双验证，不达标自动回退 fp32）。默认 fp32。")
    ap.add_argument("--skip-text", action="store_true",
                    help="只导图像编码器（基准测试不同 batch 时跳过重复导出文本塔）。")
    args = ap.parse_args()

    if not args.arch and not args.all:
        ap.error("需指定 --arch <规格> 或 --all。")
    if not args.dynamic_batch and args.batch < 1:
        ap.error("--batch 必须 ≥ 1。")

    archs = ARCH_CHOICES if args.all else [args.arch]
    if args.all:
        # 只保留子文件夹 + .pt 都存在的模型
        present = [a for a in archs if os.path.isfile(derive_paths(a)[1])]
        if not present:
            sys.exit("[ERROR] .models/ 下未找到任何模型 checkpoint。")
        print(f"[--all] 将导出：{', '.join(present)}")
        archs = present

    for arch in archs:
        export_one(arch, args.dynamic_batch, args.batch, args.fp16, args.skip_text)

    print("\n[NOTE] 若某编码器回退 fp32（文件名 .fp32.onnx），在 App 中使用时需相应改 profile 的 image_file/text_file。")


if __name__ == "__main__":
    main()
