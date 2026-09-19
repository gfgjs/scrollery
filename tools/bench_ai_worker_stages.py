#!/usr/bin/env python3
# tools/bench_ai_worker_stages.py
# ai-worker EmbedBatch 阶段基准(独立只读驱动:不经 app/不启 app、不写应用 DB)。
#
# 目的:在固定同一样本上,用真实 ai-worker + 真实 ai_thumbs WebP 缓存,按 host 请求批
# 32/64/128 各跑一轮,采集
#   1) worker 内部分段计时(ai-worker batch.rs 的 opt-in EMBED_PERF 行,SCROLLERY_AI_WORKER_PERF=1);
#   2) 驱动侧每请求墙钟;
#   3) 同一 item 跨 32/64/128 的逐项一致性(最大绝对差 / 余弦);
#   4) 与 DB 内既有向量的抽查(确认同模型同缓存);
#   5) nvidia-smi 可用时的显存/利用率采样。
#
# 协议:exotic-protocol v3 帧(EXOT + 24B 定长头 + JSON + blob);stdout 只走帧,stderr 只走日志。
# 只读硬约束:DB 以 file:...?mode=ro 打开;缓存只读既有文件;不写应用 DB / 配置。
#
# 用法(默认值即本机 dev 现场):
#   python tools/bench_ai_worker_stages.py --out docs/planning/<本任务>/bench-ai-worker-stages.json

import argparse
import array
import hashlib
import json
import os
import queue
import sqlite3
import struct
import subprocess
import threading
import time

MAGIC = b"EXOT"
PROTO = 3
FT_HELLO, FT_READY, FT_REQUEST, FT_SUCCESS, FT_FAILURE, FT_SHUTDOWN, FT_PROGRESS = 1, 2, 3, 4, 5, 6, 7
MAX_JSON_LEN = 1 << 20
MAX_BLOB_LEN = 64 << 20

MODEL_NAME = "cn-clip-vit-b16"
IMG_FILE = "vit-b-16.img.fp16.onnx"
TXT_FILE = "vit-b-16.txt.fp16.onnx"


def log(msg):
    print("[bench] " + str(msg), flush=True)


def write_frame(stream, ftype, rid, body, blob=b""):
    js = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    stream.write(struct.pack("<4sHHQII", MAGIC, PROTO, ftype, rid, len(js), len(blob)))
    stream.write(js)
    stream.write(blob)
    stream.flush()


def read_exact(stream, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = stream.read(n - len(buf))
        if not chunk:
            raise EOFError("worker stdout 结束")
        buf += chunk
    return bytes(buf)


def read_frame(stream):
    magic, ver, ftype, rid, jlen, blen = struct.unpack("<4sHHQII", read_exact(stream, 24))
    if magic != MAGIC:
        raise ValueError("帧 magic 不匹配")
    if ver != PROTO:
        raise ValueError("协议版本 %d != %d" % (ver, PROTO))
    if jlen > MAX_JSON_LEN or blen > MAX_BLOB_LEN:
        raise ValueError("帧长度超限")
    js = read_exact(stream, jlen) if jlen else b""
    blob = read_exact(stream, blen) if blen else b""
    return ftype, rid, (json.loads(js.decode("utf-8")) if js else {}), blob


def parse_kv(msg):
    out = {}
    for tok in msg.split():
        if "=" in tok:
            k, _, v = tok.partition("=")
            try:
                out[k] = int(v)
            except ValueError:
                try:
                    out[k] = float(v)
                except ValueError:
                    out[k] = v
    return out


class Worker:
    """ai-worker 子进程封装:自己启动、自己回收;stdout 帧走队列,stderr 日志走线程解析。"""

    def __init__(self, exe, cwd, env_extra):
        env = dict(os.environ)
        env.update(env_extra)
        self.proc = subprocess.Popen(
            [exe],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            env=env,
            bufsize=0,
        )
        self.frames = queue.Queue()
        self.logs = []
        self.perf = []
        self.lock = threading.Lock()
        threading.Thread(target=self.pump_stdout, daemon=True).start()
        threading.Thread(target=self.pump_stderr, daemon=True).start()

    def pump_stdout(self):
        try:
            while True:
                self.frames.put(("frame", read_frame(self.proc.stdout)))
        except Exception as e:
            self.frames.put(("error", repr(e)))

    def pump_stderr(self):
        try:
            for raw in self.proc.stderr:
                line = raw.decode("utf-8", "replace").strip()
                if not line:
                    continue
                rec = None
                try:
                    rec = json.loads(line)
                except Exception:
                    rec = None
                if not isinstance(rec, dict):
                    rec = {"msg": line}
                with self.lock:
                    self.logs.append(rec)
                    msg = rec.get("msg")
                    if isinstance(msg, str) and msg.startswith("EMBED_PERF"):
                        self.perf.append(parse_kv(msg))
        except Exception:
            pass

    def recv(self, timeout):
        kind, payload = self.frames.get(timeout=timeout)
        if kind == "error":
            raise RuntimeError("读取 worker 帧失败:" + str(payload))
        return payload

    def request(self, rid, body, timeout):
        write_frame(self.proc.stdin, FT_REQUEST, rid, body)
        progress = []
        while True:
            ftype, _rid, rbody, blob = self.recv(timeout)
            if ftype == FT_PROGRESS:
                progress.append(rbody)
                continue
            return ftype, rbody, blob, progress

    def close(self):
        try:
            write_frame(self.proc.stdin, FT_SHUTDOWN, 0, {})
        except Exception:
            pass
        try:
            self.proc.wait(timeout=15)
        except Exception:
            self.proc.kill()
            try:
                self.proc.wait(timeout=10)
            except Exception:
                pass


class GpuSampler(threading.Thread):
    """nvidia-smi 轮询采样(1s);工具不可用即静默退出,不扩大探测。"""

    def __init__(self, interval=1.0):
        super().__init__(daemon=True)
        self.interval = interval
        self.samples = []
        self.proc = None

    def run(self):
        try:
            self.proc = subprocess.Popen(
                ["nvidia-smi", "--query-gpu=utilization.gpu,memory.used,power.draw",
                 "--format=csv,noheader,nounits", "-l", str(int(self.interval))],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
            )
            for line in self.proc.stdout:
                parts = [p.strip() for p in line.split(",")]
                if len(parts) < 2:
                    continue
                rec = {"t": round(time.time(), 3)}
                for key, val in zip(("gpu_util_pct", "mem_used_mib", "power_w"), parts):
                    try:
                        rec[key] = float(val)
                    except ValueError:
                        rec[key] = None
                self.samples.append(rec)
        except Exception:
            pass

    def stop(self):
        if self.proc is not None:
            try:
                self.proc.terminate()
            except Exception:
                pass


def gpu_probe():
    try:
        r = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"],
            capture_output=True, text=True, timeout=20,
        )
        if r.returncode == 0 and r.stdout.strip():
            return r.stdout.strip()
    except Exception:
        pass
    return None


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def descriptor(role, path):
    return {
        "role": role,
        "handle": {"kind": "path", "value": path.replace(os.sep, "/")},
        "len": os.stat(path).st_size,
        "sha256": sha256_file(path),
    }


def cache_hex(cache_key):
    return "%016x" % (cache_key & 0xFFFFFFFFFFFFFFFF)


def blob_to_f32(blob):
    a = array.array("f")
    a.frombytes(blob[: (len(blob) // 4) * 4])
    return a


def max_abs_diff(a, b):
    m = 0.0
    for x, y in zip(a, b):
        d = abs(x - y)
        if d > m:
            m = d
    return m


def cosine(a, b):
    dot = 0.0
    na = 0.0
    nb = 0.0
    for x, y in zip(a, b):
        dot += x * y
        na += x * x
        nb += y * y
    if na <= 0.0 or nb <= 0.0:
        return 0.0
    return dot / ((na ** 0.5) * (nb ** 0.5))


def open_db_ro(db_path):
    uri = "file:" + db_path.replace(os.sep, "/") + "?mode=ro"
    return sqlite3.connect(uri, uri=True, timeout=20)


def pick_sample(con, count):
    """按 id 均匀抽 count 项(JOIN ai_embeddings 保证有既有向量,ai_status=2 已完成)。"""
    rows = con.execute(
        "SELECT m.id FROM media_items m "
        "JOIN ai_embeddings e ON e.item_id = m.id AND e.model_name = ? "
        "WHERE m.ai_status = 2 AND m.is_deleted = 0 ORDER BY m.id",
        (MODEL_NAME,),
    ).fetchall()
    total = len(rows)
    if total == 0:
        raise SystemExit("DB 内无可用样本(ai_status=2 且有向量)")
    step = max(1, total // count)
    picked = [r[0] for r in rows[::step]][:count]
    meta = {}
    for i in range(0, len(picked), 400):
        chunk = picked[i : i + 400]
        q = "SELECT id, cache_key FROM media_items WHERE id IN (%s)" % ",".join("?" * len(chunk))
        for item_id, cache_key in con.execute(q, chunk):
            meta[item_id] = cache_key
    return total, [(i, meta[i]) for i in picked if i in meta]


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    repo = os.path.dirname(here)
    ap = argparse.ArgumentParser()
    ap.add_argument("--appdata", default=os.environ.get("APPDATA", ""))
    ap.add_argument("--worker-exe", default=os.path.join(repo, "target", "debug", "ai-worker.exe"))
    ap.add_argument("--ort-dylib", default=os.path.join(
        repo, "node_modules", "onnxruntime-node", "bin", "napi-v6", "win32", "x64", "onnxruntime.dll"))
    ap.add_argument("--count", type=int, default=1024)
    ap.add_argument("--sizes", default="32,64,128")
    ap.add_argument("--provider", default="directml")
    ap.add_argument("--max-batch-declared", type=int, default=128)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-gpu-sampler", action="store_true")
    ap.add_argument("--profile-note", default=None,
                    help="本次 worker 可执行文件对应的构建 profile 说明(不写死;不传则记录为未声明)")
    args = ap.parse_args()

    sizes = [int(s) for s in args.sizes.split(",") if s.strip()]
    root = os.path.join(args.appdata, "com.scrollery.app")
    db_path = os.path.join(root, "scrollery.db")
    models_dir = os.path.join(root, "models")
    ai_cache_dir = os.path.join(root, "cache", "ai_thumbs")
    img_path = os.path.join(models_dir, IMG_FILE)
    txt_path = os.path.join(models_dir, TXT_FILE)
    for p in (args.worker_exe, args.ort_dylib, db_path, img_path, txt_path, ai_cache_dir):
        if not os.path.exists(p):
            raise SystemExit("缺少必要文件:" + p)

    log("DB 抽样(只读 mode=ro)")
    con = open_db_ro(db_path)
    total, picked = pick_sample(con, args.count)
    items = []
    missing = 0
    for item_id, cache_key in picked:
        key = cache_hex(cache_key)
        if os.path.exists(os.path.join(ai_cache_dir, key[:2], key + ".webp")):
            items.append({"item_id": item_id, "cache_key": key, "fingerprint": key})
        else:
            missing += 1
    log("AI 候选 %d 项(DB 已完成 %d),抽样 %d,缓存缺失 %d,实取 %d" % (len(picked), total, args.count, missing, len(items)))
    if not items:
        raise SystemExit("抽样内没有任何可用缓存,停止")

    gpu_info = gpu_probe()
    log("GPU:" + str(gpu_info))
    sampler = None
    if gpu_info and not args.no_gpu_sampler:
        sampler = GpuSampler(1.0)
        sampler.start()

    worker = Worker(
        args.worker_exe,
        repo,
        {"RUST_LOG": "info", "SCROLLERY_AI_WORKER_PERF": "1", "ORT_DYLIB_PATH": args.ort_dylib},
    )
    rid = 1
    started = time.time()
    try:
        write_frame(worker.proc.stdin, FT_HELLO, 0, {"host_version": "bench-driver", "protocol_version": PROTO, "max_blob_len": MAX_BLOB_LEN})
        ftype, _rid, ready, _blob, _p = None, None, None, None, None
        ftype, _rid, ready, _blob = worker.recv(60)
        if ftype != FT_READY:
            raise SystemExit("握手失败:首个非 Progress 帧 %d" % ftype)
        log("Ready:" + json.dumps(ready, ensure_ascii=False))

        init_body = {
            "op": "session_init",
            "session_id": 1,
            "models": [descriptor("image_encoder", img_path), descriptor("text_encoder", txt_path)],
            "model_profile": {
                "arch_id": MODEL_NAME,
                "image_file": IMG_FILE,
                "text_file": TXT_FILE,
                "batch_size": args.max_batch_declared,
                "face_profile_id": None,
            },
            "models_root": models_dir.replace(os.sep, "/"),
            "ai_cache_dir": ai_cache_dir.replace(os.sep, "/"),
            "image_provider": args.provider,
        }
        t0 = time.time()
        ftype, body, _blob, progress = worker.request(rid, init_body, 600)
        init_wall_ms = round((time.time() - t0) * 1000, 1)
        rid += 1
        if ftype != FT_SUCCESS:
            raise SystemExit("SessionInit 失败:" + json.dumps(body, ensure_ascii=False))
        session = body.get("session") or {}
        log("SessionInit %dms provider=%s gpu=%s embed_dim=%s" % (init_wall_ms, session.get("provider"), session.get("gpu_name"), session.get("embed_dim")))

        passes = []
        vectors = {}

        def run_pass(label, batch, measured):
            nonlocal rid
            reqs = []
            per_item = {}
            rids = []
            for start in range(0, len(items), batch):
                chunk = items[start : start + batch]
                req_body = {"op": "embed_batch", "items": chunk}
                t = time.time()
                ftype, body, blob, _prog = worker.request(rid, req_body, 900)
                wall_us = int(round((time.time() - t) * 1_000_000))
                if ftype != FT_SUCCESS:
                    raise SystemExit("EmbedBatch 失败(req=%d):%s" % (rid, json.dumps(body, ensure_ascii=False)))
                results = (body.get("embed") or {}).get("results") or []
                ok = [r for r in results if r.get("status") == "ok"]
                emb_dim = int(session.get("embed_dim") or 512)
                if len(blob) != len(ok) * emb_dim * 4:
                    raise SystemExit("blob 长度 %d != %d×%d×4" % (len(blob), len(ok), emb_dim))
                for idx, r in enumerate(ok):
                    vec = blob_to_f32(blob[idx * emb_dim * 4 : (idx + 1) * emb_dim * 4])
                    per_item[r["item_id"]] = vec
                rec = {
                    "rid": rid,
                    "items": len(chunk),
                    "ok": len(ok),
                    "err": len(results) - len(ok),
                    "wall_us": wall_us,
                    "items_per_s": round(len(chunk) * 1_000_000.0 / max(wall_us, 1), 2),
                }
                reqs.append(rec)
                rids.append(rid)
                rid += 1

            # worker 分段行按 req 归属本轮(日志在本帧之前写出;stderr 解析线程可能略滞后,故有限重试)
            want = set(rids)
            lines = []
            for _ in range(6):
                lines = [l for l in worker.perf if l.get("req") in want]
                if len(lines) >= len(want):
                    break
                time.sleep(0.2)
            stage_keys = ("wall_us", "decode_us", "preprocess_us", "wait_input_us", "drain_us",
                          "assemble_us", "enc_pool_us", "enc_prep_us", "enc_run_us",
                          "enc_extract_us", "enc_runs", "groups")
            summary = {
                "batch": batch,
                "measured": measured,
                "requests": len(reqs),
                "items": sum(r["items"] for r in reqs),
                "wall_us_total": sum(r["wall_us"] for r in reqs),
                "ok_total": sum(r["ok"] for r in reqs),
                "err_total": sum(r["err"] for r in reqs),
            }
            if reqs:
                # 聚合吞吐口径 = 总项数 / 总请求耗时(勿用逐请求速率算术平均)
                summary["items_per_s_aggregate"] = round(
                    summary["items"] * 1_000_000.0 / max(summary["wall_us_total"], 1), 2)
                summary["items_per_s_per_request_mean_ref_only"] = round(
                    sum(r["items_per_s"] for r in reqs) / len(reqs), 2)
                summary["wall_us_per_request_mean"] = int(
                    sum(r["wall_us"] for r in reqs) / len(reqs))
                summary["wall_us_per_item_mean"] = int(summary["wall_us_total"] / max(summary["items"], 1))
            if lines:
                st = {k: sum(l.get(k, 0) for l in lines) for k in stage_keys}
                n_items = sum(l.get("items", 0) for l in lines)
                summary["worker_stage_sums"] = st
                summary["worker_stage_us_per_item"] = {
                    k: (round(st[k] / n_items, 1) if n_items else None) for k in stage_keys}
            summary["label"] = label
            summary["requests_detail"] = reqs
            passes.append(summary)
            if measured:
                vectors[batch] = per_item
            log("pass %s batch=%d -> %s items/s(聚合), %s us/item; 分段/项(us): wait %s run %s assemble %s" % (
                label, batch, summary.get("items_per_s_aggregate"), summary.get("wall_us_per_item_mean"),
                (summary.get("worker_stage_us_per_item") or {}).get("wait_input_us"),
                (summary.get("worker_stage_us_per_item") or {}).get("enc_run_us"),
                (summary.get("worker_stage_us_per_item") or {}).get("assemble_us")))
            return summary

        # 预热:整样本跑一遍 64(更热的 OS 缓存,并给出 64 的一次重复测量)
        run_pass("warmup", 64, False)
        for size in sizes:
            run_pass("measured", size, True)

        # 跨批一致性 + DB 抽查(同一向量空间)
        consistency = []
        ref = sizes[0]
        for other in sizes[1:]:
            common = [i for i in vectors.get(ref, {}) if i in vectors.get(other, {})]
            worst = None
            for item_id in common:
                a = vectors[ref][item_id]
                b = vectors[other][item_id]
                d = max_abs_diff(a, b)
                c = cosine(a, b)
                if worst is None or d > worst["max_abs_diff"]:
                    worst = {"item_id": item_id, "max_abs_diff": d, "cosine": c}
            consistency.append({
                "a_batch": ref,
                "b_batch": other,
                "items": len(common),
                "max_abs_diff": (worst or {}).get("max_abs_diff"),
                "min_cosine": None if worst is None else worst["cosine"],
                "worst_item": worst,
            })

        ref_batch = 64 if 64 in vectors else ref
        spot_ids = list(vectors.get(ref_batch, {}).keys())
        spot_ids = spot_ids[:: max(1, len(spot_ids) // 32)][:32]
        spot = []
        for item_id in spot_ids:
            row = con.execute(
                "SELECT embedding FROM ai_embeddings WHERE item_id = ? AND model_name = ?",
                (item_id, MODEL_NAME),
            ).fetchone()
            if not row:
                continue
            db_vec = blob_to_f32(row[0])
            wv = vectors[ref_batch][item_id]
            spot.append({
                "item_id": item_id,
                "dim_db": len(db_vec),
                "dim_worker": len(wv),
                "max_abs_diff": max_abs_diff(db_vec, wv),
                "cosine": cosine(db_vec, wv),
            })
        db_check = {
            "model_name": MODEL_NAME,
            "checked": len(spot),
            "max_abs_diff": max([s["max_abs_diff"] for s in spot]) if spot else None,
            "min_cosine": min([s["cosine"] for s in spot]) if spot else None,
            "items": spot,
        }
        if sampler is not None:
            sampler.stop()

        result = {
            "kind": "ai-worker EmbedBatch 阶段基准",
            "driver": "tools/bench_ai_worker_stages.py",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "wall_seconds_total": round(time.time() - started, 1),
            "environment": {
                "worker_exe": args.worker_exe,
                "worker_exe_bytes": os.stat(args.worker_exe).st_size,
                "worker_exe_sha256": sha256_file(args.worker_exe),
                "worker_exe_mtime_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(os.stat(args.worker_exe).st_mtime)),
                "cargo_profile_note": args.profile_note or "未声明(用 --profile-note 记录构建 profile)",
                "models_dir": models_dir,
                "image_model": IMG_FILE,
                "text_model": TXT_FILE,
                "image_model_bytes": os.stat(img_path).st_size,
                "image_provider_requested": args.provider,
                "ai_cache_dir": ai_cache_dir,
                "db_path": db_path,
                "gpu": gpu_info,
            },
            "sample": {
                "requested": args.count,
                "db_completed_with_vector": total,
                "used": len(items),
                "cache_missing": missing,
                "first_item_id": items[0]["item_id"],
                "last_item_id": items[-1]["item_id"],
            },
            "session_init": {
                "wall_ms": init_wall_ms,
                "ready_session": session,
                "progress": progress,
            },
            "passes": passes,
            "throughput_by_batch": [
                {"batch": p["batch"], "label": p["label"], "items": p["items"],
                 "sum_wall_s": round(p["wall_us_total"] / 1e6, 3),
                 "items_per_s_aggregate": p.get("items_per_s_aggregate")}
                for p in passes
            ],
            "worker_perf_lines": worker.perf,
            "consistency_across_batches": consistency,
            "db_vector_spot_check": db_check,
            "gpu_samples": [] if sampler is None else sampler.samples,
            "worker_log_tail": worker.logs[-30:],
            "notes": [
                "热缓存基线:ai_thumbs 已完整存在,不含 host 缺缓存现场派生(禁止与本机冷缓存端到端数直接 A/B)",
                "worker 分段来自 batch.rs 的 opt-in EMBED_PERF 行(wall_us/decode_us/preprocess_us/wait_input_us/assemble_us/enc_*);decode/preprocess 为线程内求和,可超墙钟",
                "驱动不启动 app、不写应用 DB;DB 以 mode=ro 打开",
                "构建为 debug(opt0 本地 crate),与当前运行中的 dev 主程序一致;发布构建数值会更低",
            ],
        }
    finally:
        worker.close()
        con.close()

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, indent=2)
    log("结果写入 " + args.out)


if __name__ == "__main__":
    main()
