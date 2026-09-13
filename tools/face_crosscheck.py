# F8 对拍基准：OpenCV YuNet+SFace 参考实现 + 列 onnx 实际 I/O 名。
# 用法: python face_crosscheck.py <图片> <models目录>
import sys, numpy as np

img_path = sys.argv[1]
models = sys.argv[2]
yunet = models + "/face_detection_yunet_2023mar.onnx"
sface = models + "/face_recognition_sface_2021dec.onnx"

# ── 1. 列 onnx 实际 I/O 名（验 face.rs 的 YuNet 输出命名假设）──────────────────
def io_names(p):
    try:
        import onnxruntime as ort
        s = ort.InferenceSession(p, providers=['CPUExecutionProvider'])
        ins = [(i.name, i.shape) for i in s.get_inputs()]
        outs = [(o.name, o.shape) for o in s.get_outputs()]
        return ins, outs
    except Exception as e:
        import onnx
        m = onnx.load(p)
        ins = [i.name for i in m.graph.input]
        outs = [o.name for o in m.graph.output]
        return ins, outs

print("=== YuNet I/O ===")
yi, yo = io_names(yunet)
print("inputs :", yi)
print("outputs:", yo)
print("=== SFace I/O ===")
si, so = io_names(sface)
print("inputs :", si)
print("outputs:", so)

# ── 2. OpenCV 参考检测 + 嵌入 ──────────────────────────────────────────────────
import cv2
img = cv2.imread(img_path)  # BGR
h, w = img.shape[:2]
print(f"\n图片: {img_path}  {w}x{h}")

det = cv2.FaceDetectorYN.create(yunet, "", (w, h), score_threshold=0.6, nms_threshold=0.3, top_k=50)
det.setInputSize((w, h))
_, faces = det.detect(img)
faces = faces if faces is not None else []
print(f"\nOpenCV 检测到 {len(faces)} 张人脸 (每行: x,y,w,h, 5×关键点, score)")
for i, f in enumerate(faces):
    x, y, fw, fh = f[0:4]
    lms = f[4:14].reshape(5, 2)
    score = f[14]
    print(f"  [{i}] score={score:.3f} bbox=[{x:.0f},{y:.0f},{fw:.0f},{fh:.0f}]")
    print(f"      关键点: {[[round(float(p[0]),1), round(float(p[1]),1)] for p in lms]}")

rec = cv2.FaceRecognizerSF.create(sface, "")
feats = []
for i, f in enumerate(faces):
    aligned = rec.alignCrop(img, f)
    feat = rec.feature(aligned).flatten()  # 128
    feats.append(feat)
    n = np.linalg.norm(feat)
    unit = feat / n
    print(f"\n  脸{i} 嵌入: dim={feat.shape[0]} L2norm(原始)={n:.3f}")
    print(f"      归一化前5维: {[round(float(v),3) for v in unit[:5]]}")

if len(feats) >= 2:
    print("\n两两 cosine:")
    for i in range(len(feats)):
        for j in range(i+1, len(feats)):
            a = feats[i]/np.linalg.norm(feats[i]); b = feats[j]/np.linalg.norm(feats[j])
            print(f"  [{i}x{j}] = {float(a@b):.4f}")

# ── 与 Rust(face_smoke) 嵌入对拍：判正铁律 cosine >= 0.99 ──────────────────────
import os
rp = r"C:/workspace/scrollery/src-tauri/target/rust_emb.txt"
if os.path.exists(rp):
    rust = [np.array([float(x) for x in ln.split(",")]) for ln in open(rp) if ln.strip()]
    print(f"\n=== Rust vs OpenCV cosine (脸数 rust={len(rust)} opencv={len(feats)}) ===")
    for i in range(min(len(rust), len(feats))):
        ov = feats[i] / np.linalg.norm(feats[i])
        rv = rust[i] / np.linalg.norm(rust[i])
        c = float(rv @ ov)
        flag = "OK" if c >= 0.99 else ("接近" if c >= 0.95 else "偏差大")
        print(f"  脸{i}: cosine = {c:.4f}  [{flag}]")
else:
    print(f"\n(未找到 {rp}，先跑 face_smoke 生成)")
