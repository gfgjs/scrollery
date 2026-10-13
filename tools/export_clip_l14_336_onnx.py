# -*- coding: utf-8 -*-
"""
把 Chinese-CLIP ViT-L/14@336 的 PyTorch 权重导出为 ONNX（默认 fp32；启用 fp16 时失败回退 fp32）。

产物（落地到 .models/，文件名匹配 profile `cn-clip-vit-l14-336`）：
  - vit-l-14-336.img.fp16.onnx (+ .extra_file)   图像编码器（回退时为 .img.fp32.onnx）
  - vit-l-14-336.txt.fp16.onnx (+ .extra_file)   文本编码器（回退时为 .txt.fp32.onnx）

契约（详见 src-tauri/src/ai/clip.rs / profile.rs）：
  - 图像输入名 "image" [N,3,S,S] → 输出 "unnorm_image_features" [N,768]（未归一化，Rust 端再 L2）
    batch 轴 N 为**动态**（区别于 eisneim B/16 钉死为 1），强 GPU 可整批推理。
  - 文本输入名 "text" [1,52] → 输出 "unnorm_text_features" [1,768]（batch 固定 1，查询单条）

导出时需保留的约束：
  1. 官方 cn_clip.deploy 的 fp16 走 onnxmltools，其同名入口在新版已退化为抛异常的桩
     （convert_large_onnx.bat 的注释记录了这个「依赖地狱」）→ 这里改用 onnxruntime 自带转换器。
  2. torch 2.12 默认 dynamo 导出器不能稳妥处理 (None, text) 占位入参 → dynamo=False 强制旧路径。
  3. torch 2.12 把注意力融合成 aten::scaled_dot_product_attention（需 opset≥14）。
  4. **opset 必须 ≥17**：opset14 把 LayerNorm 拆成 ReduceMean/Sqrt/... 原语，转 fp16 后
     ORT 的 SimplifiedLayerNormFusion 会因中间插入的 Cast 节点崩溃
     （GetIndexFromName ... InsertedPrecisionFreeCast 不存在）。opset17 把 LayerNorm 导成
     单个 LayerNormalization 算子，没有可被重融合的原语，规避该崩溃。
  5. torch 旧导出器把 ~1.2GB 的 fp32 图一次性写盘，在 Windows 触发 [Errno 22]（大文件单次写入）
     → 改为导出到内存 BytesIO，再用 onnx 的外部数据保存（按块写盘）。
  6. fp16 产物必须双重验证：① 用 ORT「默认全套图优化」建会话+推理；
     ② 与原始 PyTorch(fp32) 输出逐条比对余弦 ≥0.99。**只「能加载」不够**——CLIP 的 BERT
     文本编码器在 fp16 下数值不稳（注意力/softmax/LayerNorm 溢出），会「能加载但算错」：
      所有文本可能塌缩成几乎相同向量。任一验证不过，该编码器回退 fp32。

依赖：
  torch 2.12 / torchvision 0.27 / cn_clip 1.6 / onnx 1.21 / onnxruntime 1.26 / onnxscript / six
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

ROOT = os.path.dirname(os.path.abspath(__file__))
# 脚本位于 tools/ 子目录 → .models 在上一级（项目根）。
MODELS_DIR = os.path.join(os.path.dirname(ROOT), ".models")

# 这三项由命令行 --arch 在 main() 中按 cn_clip 的 _MODELS 映射推导（见 _derive_paths）。
ARCH = "ViT-L-14-336"            # 默认；可 --arch ViT-L-14 等
CKPT_PATH = ""                   # .models/clip_cn_<...>.pt
SAVE_PREFIX = ""                 # .models/<前缀>，产物名前缀

CONTEXT_LENGTH = 52
OPSET = 17  # 见文件头踩坑 #3/#4


def _derive_paths(arch: str):
    """按 cn_clip 的 _MODELS 映射，从 arch 推导 checkpoint 路径与产物名前缀。
    例：ViT-L-14 → 输入 clip_cn_vit-l-14.pt，前缀 vit-l-14（去掉 clip_cn_ 与 .pt）。"""
    ckpt_name = _MODELS[arch][1]                                  # clip_cn_vit-l-14.pt
    prefix = ckpt_name.removeprefix("clip_cn_").removesuffix(".pt")  # vit-l-14
    return os.path.join(MODELS_DIR, ckpt_name), os.path.join(MODELS_DIR, prefix)

# 是否尝试 fp16：对 ViT-L 文本编码器，fp16 会数值塌缩（见文件头踩坑 #6），实测不可用，
# 故默认 False 直接产 fp32。轻量化场景请用 ViT-B/16（其 fp16 由 eisneim 仓库直供）。
# 置 True 可对将来 fp16 稳定的模型走「转 fp16 + 加载/数值双验证 + 不达标回退」流程。
PREFER_FP16 = False


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
    """外部数据格式保存（小 .onnx 头 + 同名 .extra_file 权重），与 B/16 导出一致；onnx 按块写盘。"""
    # 必须连同旧 .extra_file 一并删除！onnx 写外部数据用 **追加模式('ab')**，残留旧权重文件
    # 会被追加而非覆盖 → 体积正好翻倍，且 header 偏移指向新追加段、模型仍能正常加载（极隐蔽）。
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
    """双重验证 fp16：ORT 默认优化能建会话+推理，且每条输出与 PyTorch 参考余弦 ≥0.99。"""
    try:
        sess = ort.InferenceSession(path, providers=["CPUExecutionProvider"])
        for feed, ref in zip(feeds, refs):
            out = sess.run(None, feed)[0][0]
            c = _cos(out, ref)
            if c < 0.99:  # 数值塌缩/溢出 → fp16 不可用（CLIP 文本编码器尤甚）
                print(f"  [WARN] fp16 数值偏差过大（与 torch 余弦={c:.4f}<0.99）→ 回退 fp32")
                return False
        return True
    except Exception as e:  # noqa: BLE001
        print(f"  [WARN] fp16 在 ORT 默认优化下不可加载 → 回退 fp32：{str(e)[:140]}")
        return False


def _emit(proto_fp32, role: str, feeds: list, refs: list):
    """落地一个编码器：优先 fp16（经加载+数值双验证），不达标回退 fp32。返回最终精度后缀。"""
    fp16_path = f"{SAVE_PREFIX}.{role}.fp16.onnx"
    fp32_path = f"{SAVE_PREFIX}.{role}.fp32.onnx"

    # 默认直接产 fp32（ViT-L 走高阶/精确路线；fp16 对其文本编码器塌缩，见 PREFER_FP16 说明）。
    if not PREFER_FP16:
        for p in (fp16_path, fp16_path + ".extra_file"):
            if os.path.exists(p):
                os.remove(p)
        _save_external(proto_fp32, fp32_path)
        return "fp32"

    # keep_io_types=True：io 仍 fp32（Rust 端继续喂 f32、取 f32），只把内部权重/计算转 fp16。
    model_fp16 = convert_float_to_float16(proto_fp32, keep_io_types=True)
    _save_external(model_fp16, fp16_path)
    if _fp16_ok(fp16_path, feeds, refs):
        for p in (fp32_path, fp32_path + ".extra_file"):
            if os.path.exists(p):
                os.remove(p)
        return "fp16"

    # 回退 fp32：删掉不可用的 fp16，落 fp32（数值精确、必定可加载）。
    for p in (fp16_path, fp16_path + ".extra_file"):
        if os.path.exists(p):
            os.remove(p)
    _save_external(proto_fp32, fp32_path)
    return "fp32"


def main() -> None:
    ap = argparse.ArgumentParser(description="Chinese-CLIP .pt → ONNX 导出（优先 fp16，失败回退 fp32）")
    ap.add_argument("--arch", default="ViT-L-14-336",
                    choices=["ViT-B-16", "ViT-L-14", "ViT-L-14-336", "ViT-H-14", "RN50"],
                    help="模型规格；checkpoint 与产物名按此自动推导。")
    ap.add_argument("--dynamic-batch", action="store_true",
                    help="图像塔 batch 轴设为动态（默认固定=1）。固定形状下 ORT 能做更充分的"
                         "内存规划/算子特化，单张推理更快（与 B/16 一致）；动态轴仅在强 GPU 整批"
                         "推理时才可能更划算，但喂单张反而更慢。")
    args = ap.parse_args()

    global ARCH, CKPT_PATH, SAVE_PREFIX
    ARCH = args.arch
    CKPT_PATH, SAVE_PREFIX = _derive_paths(ARCH)

    if not os.path.isfile(CKPT_PATH):
        sys.exit(f"[ERROR] 找不到 checkpoint：{CKPT_PATH}")

    print(f"[1/4] 加载 checkpoint：{CKPT_PATH}")
    with open(CKPT_PATH, "rb") as f:
        checkpoint = torch.load(f, map_location="cpu")  # 默认 weights_only=True，安全

    print(f"[2/4] 构建并恢复模型（arch={ARCH}）")
    struct = _MODEL_INFO[ARCH]["struct"]
    res = _MODEL_INFO[ARCH]["input_resolution"]  # 336
    model = create_model(struct, checkpoint).float().eval()

    blank = torch.zeros(1, 3, res, res, dtype=torch.float32)  # 占位（仅供 onnx 追踪图）
    text0 = clip.tokenize([""], context_length=CONTEXT_LENGTH)  # int64 [1,52]

    # 验证用真实输入 + PyTorch(fp32) 参考输出（必须在 del model 之前算好）。
    val_texts = ["一只猫", "一辆红色的汽车", "雪山日落的风景", "a cute puppy dog"]
    val_toks = [clip.tokenize([t], context_length=CONTEXT_LENGTH) for t in val_texts]
    rng = np.random.default_rng(0)
    val_img = ((rng.random((1, 3, res, res), dtype=np.float32) - 0.5) / 0.5)

    print("[3/4] 追踪两个编码器到内存 proto + 计算 PyTorch 参考输出")
    with torch.no_grad():
        txt_proto = _trace_to_proto(model, (None, text0), ["text"], ["unnorm_text_features"], fold=True)
        # 图像塔 batch 轴：默认固定=1（ORT 静态形状优化更充分，单张更快）；
        #   --dynamic-batch 时设为动态，强 GPU 整批推理（encode_image_batch 检测到动态轴即整批送入）。
        #   文本塔始终固定 batch=1（查询是单条）。空间维 S×S 与序列长 52 始终固定（契约要求）。
        img_dyn = ({"image": {0: "batch"}, "unnorm_image_features": {0: "batch"}}
                   if args.dynamic_batch else None)
        img_proto = _trace_to_proto(
            model, (blank, None), ["image"], ["unnorm_image_features"], fold=False,
            dynamic_axes=img_dyn,
        )
        txt_refs = [model(None, tk).numpy()[0] for tk in val_toks]
        img_ref = model(torch.from_numpy(val_img), None).numpy()[0]
    del model  # 释放 fp32 torch 模型（~2.4GB），给 fp16 转换腾内存

    print("[4/4] 转 fp16 并双验证（加载+数值），不达标回退 fp32")
    txt_feeds = [{"text": tk.numpy().astype(np.int64)} for tk in val_toks]
    img_feeds = [{"image": val_img}]
    prec_txt = _emit(txt_proto, "txt", txt_feeds, txt_refs)
    prec_img = _emit(img_proto, "img", img_feeds, [img_ref])

    print("\n[DONE] 产物（.models/）：")
    for role, prec in (("img", prec_img), ("txt", prec_txt)):
        base = f"{SAVE_PREFIX}.{role}.{prec}.onnx"
        for p in (base, base + ".extra_file"):
            if os.path.exists(p):
                print(f"  {os.path.basename(p):42s} {os.path.getsize(p)/(1024*1024):8.1f} MB")
    print(f"\n图像编码器精度: {prec_img}    文本编码器精度: {prec_txt}")
    if prec_img != "fp16" or prec_txt != "fp16":
        print("[NOTE] 有编码器回退 fp32（文件名为 .fp32.onnx）；需相应改 profile 的 image_file/text_file。")


if __name__ == "__main__":
    main()
