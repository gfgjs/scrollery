# InsightFace SCRFD + ArcFace 参考实现，用于对拍 Rust 的检测与嵌入路径。
# 对照输出顺序、预处理、anchor 解码和距离解码；同脸向量余弦阈值为 0.99。
#
# ⚠️ 许可（务必自行确认）
#   SCRFD (scrfd_10g_bnkps.onnx) 与 ArcFace (w600k_r50.onnx) 均来自 InsightFace model-zoo，
#   **权重仅限非商业研究用途**。本项目商业发行默认轨是 YuNet+SFace（MIT/Apache），SCRFD 轨
#   commercial_ok=false，仅供测试/个人。下载与使用前请亲自核实 InsightFace 的 LICENSE。
#
# ── 准备 ────────────────────────────────────────────────────────────────────────
#   1. pip install insightface onnxruntime opencv-python numpy
#   2. 下载两个 onnx 到 models 目录（与 Rust 同一目录），文件名须与 face_profile.rs 一致：
#        scrfd_10g_bnkps.onnx   （SCRFD 检测，512 单边输入 / 多 stride anchor）
#        w600k_r50.onnx         （ArcFace R50，512 维嵌入）
#      来源：InsightFace 官方 model-zoo（buffalo_l 包内含 det_10g.onnx=SCRFD 与 w600k_r50.onnx）。
#      注意：InsightFace 的 buffalo_l 检测器文件名是 det_10g.onnx，与本项目约定的
#      scrfd_10g_bnkps.onnx 是同一模型不同命名 —— 复制时重命名，或改 face_profile.rs 的 detect_file。
#
# ── 对拍流程 ──────────────────────────────────────────────────────────────
#   A. 切 Rust 到 SCRFD 轨并 dump 向量（face_smoke 已支持 FACE_PROFILE 环境变量）：
#        cd src-tauri
#        FACE_PROFILE=scrfd-arcface-r50 cargo run --example face_smoke -- <图片> [models目录]
#      （PowerShell: $env:FACE_PROFILE='scrfd-arcface-r50'; cargo run --example face_smoke -- <图片>）
#      它把每张脸的嵌入写到 target/rust_emb.txt（每行一条逗号分隔的 512 维向量）。
#
#   B. 跑本脚本（同图、同 models 目录）取 InsightFace 基准并与 A 的 dump 比 cosine：
#        python tools/scrfd_crosscheck.py <图片> <models目录>
#
#   C. 判定铁律：**同一张脸**的 Rust vs InsightFace cosine ≥ 0.99 才算 detect_scrfd + ArcFace
#      预处理逐位正确。若 0.9~0.99：多半是检测关键点亚像素差异（对齐 warp 输入略偏），可用
#      FACE_INPUT 隔离实验（把本脚本打印的关键点喂回 face_smoke，单独验对齐+嵌入）。若 < 0.9：
#      detect_scrfd 的 anchor 解码 / 预处理 / 输出顺序有错 —— 对照下方打印的 InsightFace 框与
#      关键点逐项排查（重点：RGB 通道序、(x-127.5)/128 归一化、stride 8/16/32 的 anchor 顺序）。
#
# 用法: python scrfd_crosscheck.py <图片> <models目录>
# ──────────────────────────────────────────────────────────────────────────────
import sys
import os
import numpy as np


def io_names(p):
    """列 onnx 实际 I/O 名 + shape（验 detect_scrfd 的输出命名/顺序假设）。"""
    try:
        import onnxruntime as ort
        s = ort.InferenceSession(p, providers=["CPUExecutionProvider"])
        ins = [(i.name, i.shape) for i in s.get_inputs()]
        outs = [(o.name, o.shape) for o in s.get_outputs()]
        return ins, outs
    except Exception:
        import onnx
        m = onnx.load(p)
        return [i.name for i in m.graph.input], [o.name for o in m.graph.output]


def main():
    if len(sys.argv) < 3:
        print("用法: python scrfd_crosscheck.py <图片> <models目录>")
        sys.exit(2)

    img_path = sys.argv[1]
    models = sys.argv[2]
    scrfd = os.path.join(models, "scrfd_10g_bnkps.onnx")
    arcface = os.path.join(models, "w600k_r50.onnx")

    for f in (scrfd, arcface):
        if not os.path.exists(f):
            print(f"缺模型文件: {f}\n（见脚本顶部「准备」：从 InsightFace model-zoo 下载并重命名）")
            sys.exit(1)

    # ── 1. onnx I/O 名（对照 face.rs::detect_scrfd 的输出顺序假设）────────────────
    print("=== SCRFD I/O ===")
    si, so = io_names(scrfd)
    print("inputs :", si)
    print("outputs:", so)
    print("  ↑ detect_scrfd 假设的多 stride 输出（score/bbox/kps × stride 8/16/32）须与此顺序吻合")
    print("=== ArcFace I/O ===")
    ai, ao = io_names(arcface)
    print("inputs :", ai)
    print("outputs:", ao)

    import cv2

    # ── 2. InsightFace 权威检测 + 嵌入（基准）─────────────────────────────────────
    # 直接用 insightface 的 SCRFD/ArcFace 封装，避免手写 anchor 解码（那正是要被对拍的对象）。
    from insightface.model_zoo import SCRFD, ArcFaceONNX
    from insightface.utils import face_align

    det = SCRFD(scrfd)
    det.prepare(ctx_id=-1, input_size=(640, 640))  # ctx_id=-1 → CPU
    rec = ArcFaceONNX(arcface)
    rec.prepare(ctx_id=-1)

    img = cv2.imread(img_path)  # BGR（insightface 内部按需转 RGB）
    if img is None:
        print(f"图片读取失败: {img_path}")
        sys.exit(1)
    h, w = img.shape[:2]
    print(f"\n图片: {img_path}  {w}x{h}")

    bboxes, kpss = det.detect(img, thresh=0.5, input_size=(640, 640))
    n = 0 if bboxes is None else len(bboxes)
    print(f"\nInsightFace 检测到 {n} 张人脸")
    feats = []
    for i in range(n):
        box = bboxes[i]
        kps = kpss[i]
        score = box[4]
        print(f"  [{i}] score={score:.3f} bbox=[{box[0]:.0f},{box[1]:.0f},{box[2]:.0f},{box[3]:.0f}]")
        print(f"      关键点: {[[round(float(p[0]),1), round(float(p[1]),1)] for p in kps]}")
        aligned = face_align.norm_crop(img, kps, image_size=112)  # 5 点相似变换 → 112
        feat = rec.get_feat(aligned).flatten()  # 512
        feats.append(feat)
        nrm = np.linalg.norm(feat)
        unit = feat / max(nrm, 1e-12)
        print(f"      嵌入 dim={feat.shape[0]} L2norm(原始)={nrm:.3f} 归一化前5维={[round(float(v),3) for v in unit[:5]]}")

    # ── 3. 与 Rust(face_smoke, FACE_PROFILE=scrfd-arcface-r50) dump 对拍 ───────────
    rp = r"C:/workspace/scrollery/src-tauri/target/rust_emb.txt"
    if os.path.exists(rp):
        rust = [np.array([float(x) for x in ln.split(",")]) for ln in open(rp) if ln.strip()]
        print(f"\n=== Rust vs InsightFace cosine (脸数 rust={len(rust)} insightface={len(feats)}) ===")
        if len(rust) and len(rust[0]) != 512:
            print(f"⚠️ Rust 向量维度 {len(rust[0])} ≠ 512 —— 是否忘了设 FACE_PROFILE=scrfd-arcface-r50？")
        for i in range(min(len(rust), len(feats))):
            ov = feats[i] / max(np.linalg.norm(feats[i]), 1e-12)
            rv = rust[i] / max(np.linalg.norm(rust[i]), 1e-12)
            c = float(rv @ ov)
            flag = "OK(逐位正确)" if c >= 0.99 else ("接近(查亚像素)" if c >= 0.9 else "偏差大(查解码/预处理)")
            print(f"  脸{i}: cosine = {c:.4f}  [{flag}]")
    else:
        print(f"\n(未找到 {rp}；先跑：cd src-tauri && FACE_PROFILE=scrfd-arcface-r50 "
              f"cargo run --example face_smoke -- <图片> {models})")


if __name__ == "__main__":
    main()
