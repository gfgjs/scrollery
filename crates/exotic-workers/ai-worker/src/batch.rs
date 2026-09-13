// crates/exotic-workers/ai-worker/src/batch.rs
//! EmbedBatch / FaceDetectEmbed 批处理(Part6 §3.2.1a 逐项化:项失败不连坐整批)。
//!
//! 载运契约(与 message.rs 文档一致,host 侧按此校验):
//!   - 嵌入本体不进 JSON,走 Success 帧同帧 blob(f32 LE);
//!   - EmbedBatch blob 按 results 中 **Ok 项顺序**连续排布,每项 `embed_dim × 4` 字节;
//!   - FaceDetectEmbed blob 按 Ok 项顺序、项内按 faces 顺序,每脸 `face_embed_dim × 4` 字节;
//!   - 维度不符 = EmbedDimMismatch(terminal,系统性错误必然全批皆错 → 整批 Failure)。
//!
//! 路径安全:worker 只凭 `cache_key` 拼 `{ai_cache_dir}/{key[..2]}/{key}.webp`,key 白名单
//! 限定 ASCII 字母数字(无分隔符/无点)→ join 不可能越出缓存根(Part6 §3.2.1a ②)。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use exotic_protocol::{
    EmbedBatchSuccess, EmbedItem, EmbedResult, FaceBatchSuccess, FaceDet, FaceItem, FaceItemResult,
    FailureBody, Frame, FrameType, SuccessBody, TextEmbedSuccess, WorkerErrorCode, MAX_BLOB_LEN,
};
use ndarray::Array4;
use scrollery_ai_core::{clip, face, DecodedImage};

use crate::session::SessionState;

/// source_path 回退解码的源文件字节上限(与 psd-worker 同值;拦巨文件吃满内存)。
const MAX_SOURCE_FILE_BYTES: u64 = 512 << 20;

/// 推理子批上限(T18.5b 流水重叠):推理侧从解码 channel 攒到即推,不等全批。
/// 对动态/bN 导出保留组批效率;B/16 固定 batch=1 导出内部仍逐张,无行为差别。
const INFER_SUB_BATCH: usize = 16;

/// 实际解码线程数 = min(逻辑核数, 任务数)。并行度**探测机器核数、不设固定上限**
/// (2026-07-03 用户拍板,推翻此前 16/32 封顶,对齐进程内 rayon 全池语义);worker
/// 进程为 BELOW_NORMAL 优先级,前台需要 CPU 时由系统调度让路(host 的 CPU permit
/// 保持「1 批=1 槽」记账)。
fn decode_threads(count: usize) -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(count)
        .max(1)
}

/// 按索引并行执行 `work`,结果按原索引序返回(items 序即协议契约序,不可乱)。
/// std::thread::scope + 原子游标领活:零新依赖;槽位互斥锁只在写回瞬间持有。
fn parallel_map_indexed<T: Send>(
    n_threads: usize,
    count: usize,
    work: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    if count == 0 {
        return Vec::new();
    }
    if n_threads <= 1 {
        return (0..count).map(work).collect();
    }
    let mut slots: Vec<Option<T>> = Vec::with_capacity(count);
    slots.resize_with(count, || None);
    let out = Mutex::new(slots);
    let cursor = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..n_threads {
            s.spawn(|| loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= count {
                    break;
                }
                let v = work(i);
                out.lock().unwrap_or_else(|p| p.into_inner())[i] = Some(v);
            });
        }
    });
    out.into_inner()
        .unwrap_or_else(|p| p.into_inner())
        .into_iter()
        .map(|v| v.expect("scope 退出即全部索引已处理"))
        .collect()
}

/// EncodeText 单批文本数上限(防御:查询通常一批一条;超限= host bug)。
const MAX_TEXTS_PER_BATCH: usize = 64;
/// EncodeText 单条文本字节上限(防御:tokenizer 会按上下文长度截断,但不给
/// 异常 host 用超长串把 worker 拖进无谓的分词开销)。
const MAX_TEXT_BYTES: usize = 8 * 1024;

// WorkerLogLine 行协议输出(D-314 单一 schema:reviewer 深审揪出本文件曾漏收编,
// 裸 eprintln 行会被 supervisor 以 WARN+unparsed 兜底转发,正常批诊断被抬成 WARN 噪音)。
// 定级同 main.rs 原则:批诊断=info、推理/单项失败(worker 继续服务)=warn。
fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}

/// 整批失败帧(逐项 Err 之外的系统性失败:批超限/推理错误/维度红线/blob 超限)。
fn batch_failure(request_id: u64, code: WorkerErrorCode, message: String) -> Frame {
    let body = FailureBody {
        item_id: None,
        input_fingerprint: None,
        code,
        retryable: code.default_retryable(),
        message,
    };
    Frame::control(FrameType::Failure, request_id, &body).unwrap()
}

/// `cache_key` → 缓存文件路径。白名单校验失败返回 None(调用方回逐项 MalformedInput)。
pub fn cache_webp_path(ai_cache_dir: &Path, key: &str) -> Option<PathBuf> {
    if key.len() < 3 || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    // 白名单已保证纯 ASCII,字节切片即字符切片。
    Some(ai_cache_dir.join(&key[..2]).join(format!("{key}.webp")))
}

/// 读 + 解码一个 ai_cache WebP。错误映射:缺文件/IO → IoError(retryable,cache 可能尚未
/// 生成或已被清理);解码失败 → MalformedInput(坏缓存,重试无意义)。
fn load_cache_image(
    ai_cache_dir: &Path,
    key: &str,
) -> Result<image::DynamicImage, WorkerErrorCode> {
    let path = cache_webp_path(ai_cache_dir, key).ok_or(WorkerErrorCode::MalformedInput)?;
    let bytes = std::fs::read(&path).map_err(|_| WorkerErrorCode::IoError)?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
        .map_err(|_| WorkerErrorCode::MalformedInput)
}

/// 把 `Vec<f32>` 嵌入按小端追加进 blob(布局契约见模块头)。
fn append_embedding(blob: &mut Vec<u8>, emb: &[f32]) {
    blob.reserve(emb.len() * 4);
    for &f in emb {
        blob.extend_from_slice(&f.to_le_bytes());
    }
}

/// 处理 EmbedBatch:逐项并行解码(不连坐,T18.5)→ 组批一次推理 → 结果与 blob 按请求序装配。
pub fn handle_embed(sess: &SessionState, request_id: u64, items: &[EmbedItem]) -> Frame {
    if items.len() as u32 > sess.batch_size {
        return batch_failure(
            request_id,
            WorkerErrorCode::MalformedInput,
            format!(
                "批大小 {} 超过 session 声明 {}",
                items.len(),
                sess.batch_size
            ),
        );
    }

    // 1-2. 解码/预处理与推理**流水重叠**(T18.5b):解码线程池喂有界 channel,本线程
    //      攒子批边收边推——CPU(解码)与 GPU(推理)同时有活。此前「先全解完再推」
    //      的相位交替使 GPU/CPU 利用率互为镜像(实测各仅 ~35%/50%),批耗时=两段之和;
    //      重叠后=两段取大。有界容量封顶在飞张量内存(600KB/张 × 2×threads)。
    //      失败仍逐项不连坐;结果按 items 原序落槽(与 blob 布局对齐)。
    let side = sess.profile.image_size as usize;
    let threads = decode_threads(items.len());
    let pool = match sess.pool.clip_image_session.as_ref() {
        Some(p) => p,
        // load() 已保证 clip_ready;此臂仅防御(池在运行期不会消失)。
        None => {
            return batch_failure(
                request_id,
                WorkerErrorCode::SessionExpired,
                "CLIP 图像编码器不在会话中".into(),
            )
        }
    };
    let mut item_err: Vec<Option<WorkerErrorCode>> = vec![None; items.len()];
    let mut embeds: Vec<Option<Vec<f32>>> = Vec::with_capacity(items.len());
    embeds.resize_with(items.len(), || None);
    // 批级失败(推理错误/维度红线)带出 scope;设置后须尽快 drop(rx) 解除解码线程
    // 在满 channel 上的 send 阻塞,否则 scope 等待与 send 互相卡死。
    let mut batch_fail: Option<Frame> = None;
    // cursor 在 scope 外声明:被 spawn 线程借用,须活过整个 'scope。
    let cursor = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, Result<Array4<f32>, WorkerErrorCode>)>(
            threads * 2,
        );
        for _ in 0..threads {
            let tx = tx.clone();
            let cursor = &cursor;
            s.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                let r = load_cache_image(&sess.ai_cache_dir, &items[i].cache_key)
                    .map(|img| clip::preprocess_image(&img, &sess.profile));
                if tx.send((i, r)).is_err() {
                    break; // 推理侧已终止(批级失败提前收摊)
                }
            });
        }
        drop(tx); // 主线程不发;解码线程全退后 recv 得 Disconnected 即收尾

        let mut done = false;
        while !done {
            // 阻塞等第一个可推理项(解码错误项就地记账,不占子批位)。
            let mut pending: Vec<(usize, Array4<f32>)> = Vec::with_capacity(INFER_SUB_BATCH);
            loop {
                match rx.recv() {
                    Ok((i, Ok(t))) => {
                        pending.push((i, t));
                        break;
                    }
                    Ok((i, Err(code))) => item_err[i] = Some(code),
                    Err(_) => {
                        done = true;
                        break;
                    }
                }
            }
            // 非阻塞攒满子批(解码慢时不等,拿到多少推多少——重叠优先于批满)。
            while pending.len() < INFER_SUB_BATCH {
                match rx.try_recv() {
                    Ok((i, Ok(t))) => pending.push((i, t)),
                    Ok((i, Err(code))) => item_err[i] = Some(code),
                    Err(_) => break,
                }
            }
            if pending.is_empty() {
                continue; // done 收尾轮无残批
            }

            let n = pending.len();
            let mut batch = Array4::<f32>::zeros((n, 3, side, side));
            {
                // 平坦 memcpy 拼子批(新建张量必为标准布局;逐元素 assign 是 dev 热点)。
                let dst = batch.as_slice_mut().expect("零初始化张量必为标准布局");
                let stride = 3 * side * side;
                for (bi, (_, t)) in pending.iter().enumerate() {
                    dst[bi * stride..(bi + 1) * stride]
                        .copy_from_slice(t.as_slice().expect("preprocess 输出为标准布局"));
                }
            }
            match clip::encode_image_batch(pool, batch, &sess.profile) {
                Ok(vecs) => {
                    // 维度红线(terminal):模型输出与契约不符是系统性错误,整批 Failure。
                    if vecs.iter().any(|e| e.len() != sess.profile.embed_dim) {
                        batch_fail = Some(batch_failure(
                            request_id,
                            WorkerErrorCode::EmbedDimMismatch,
                            format!("嵌入维度与契约 {} 不符", sess.profile.embed_dim),
                        ));
                        break;
                    }
                    for ((i, _), v) in pending.iter().zip(vecs) {
                        embeds[*i] = Some(v);
                    }
                }
                Err(e) => {
                    log_warn(format!("EmbedBatch 推理失败:{e}"));
                    batch_fail = Some(batch_failure(
                        request_id,
                        WorkerErrorCode::InternalError,
                        "批量嵌入推理失败".into(),
                    ));
                    break;
                }
            }
        }
        drop(rx); // 批级失败提前退出时解除解码线程 send 阻塞(正常收尾时为空操作)
    });
    if let Some(frame) = batch_fail {
        return frame;
    }

    // 3. 按请求序装配 results;Ok 项按序进 blob。
    let mut results = Vec::with_capacity(items.len());
    let mut blob: Vec<u8> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item_err[i] {
            Some(code) => results.push(EmbedResult::Err {
                item_id: item.item_id,
                fingerprint: item.fingerprint.clone(),
                code,
            }),
            None => {
                let emb = embeds[i]
                    .take()
                    .expect("非批级失败路径下每个无错项必有嵌入");
                append_embedding(&mut blob, &emb);
                results.push(EmbedResult::Ok {
                    item_id: item.item_id,
                    fingerprint: item.fingerprint.clone(),
                });
            }
        }
    }
    if blob.len() > MAX_BLOB_LEN as usize {
        return batch_failure(
            request_id,
            WorkerErrorCode::ResourceLimit,
            format!("嵌入 blob {} 字节超协议上限", blob.len()),
        );
    }

    let body = SuccessBody {
        embed: Some(EmbedBatchSuccess { results }),
        ..Default::default()
    };
    Frame::with_blob(FrameType::Success, request_id, &body, blob).unwrap()
}

/// 处理 EncodeText(T17):逐条编码 → 全批原子(文本编码无逐项 IO 失败模式,任一失败
/// 即整批 Failure)。blob 按 texts 顺序连续排布,每项 `embed_dim × f32(LE)`;文本塔 EP
/// 恒 CPU(AiEnginePool 构建时已定,Part4 §8.6)。
pub fn handle_encode_text(sess: &SessionState, request_id: u64, texts: &[String]) -> Frame {
    if texts.is_empty() || texts.len() > MAX_TEXTS_PER_BATCH {
        return batch_failure(
            request_id,
            WorkerErrorCode::MalformedInput,
            format!("文本批大小 {} 非法(1..={MAX_TEXTS_PER_BATCH})", texts.len()),
        );
    }
    if texts.iter().any(|t| t.len() > MAX_TEXT_BYTES) {
        return batch_failure(
            request_id,
            WorkerErrorCode::MalformedInput,
            format!("单条文本超 {MAX_TEXT_BYTES} 字节上限"),
        );
    }
    let pool = match sess.pool.clip_text_session.as_ref() {
        Some(p) => p,
        // load() 已保证 clip_ready;此臂仅防御(与 handle_embed 的图像塔臂同构)。
        None => {
            return batch_failure(
                request_id,
                WorkerErrorCode::SessionExpired,
                "CLIP 文本编码器不在会话中".into(),
            )
        }
    };

    let mut blob: Vec<u8> = Vec::with_capacity(texts.len() * sess.profile.embed_dim * 4);
    for text in texts {
        match clip::encode_text(pool, &sess.tokenizer, text, &sess.profile) {
            Ok(emb) => {
                // 维度红线(terminal):模型输出与契约不符是系统性错误,整批 Failure。
                if emb.len() != sess.profile.embed_dim {
                    return batch_failure(
                        request_id,
                        WorkerErrorCode::EmbedDimMismatch,
                        format!("文本嵌入维度与契约 {} 不符", sess.profile.embed_dim),
                    );
                }
                append_embedding(&mut blob, &emb);
            }
            Err(e) => {
                log_warn(format!("EncodeText 推理失败:{e}"));
                return batch_failure(
                    request_id,
                    WorkerErrorCode::InternalError,
                    "文本编码失败".into(),
                );
            }
        }
    }
    if blob.len() > MAX_BLOB_LEN as usize {
        return batch_failure(
            request_id,
            WorkerErrorCode::ResourceLimit,
            format!("文本嵌入 blob {} 字节超协议上限", blob.len()),
        );
    }

    let body = SuccessBody {
        text_embed: Some(TextEmbedSuccess {
            count: texts.len() as u32,
        }),
        ..Default::default()
    };
    Frame::with_blob(FrameType::Success, request_id, &body, blob).unwrap()
}

/// 单项人脸源解码:cache_key 优先,source_path 回退(host 决定给哪个,协议 §FaceItem)。
fn load_face_image(ai_cache_dir: &Path, item: &FaceItem) -> Result<DecodedImage, WorkerErrorCode> {
    let img = if let Some(key) = &item.cache_key {
        load_cache_image(ai_cache_dir, key)?
    } else if let Some(src) = &item.source_path {
        // 信任语义同 Thumbnail.source_path(host 提供绝对路径);读盘前 stat 拦巨文件。
        let meta = std::fs::metadata(src).map_err(|_| WorkerErrorCode::IoError)?;
        if meta.len() > MAX_SOURCE_FILE_BYTES {
            return Err(WorkerErrorCode::ResourceLimit);
        }
        let bytes = std::fs::read(src).map_err(|_| WorkerErrorCode::IoError)?;
        image::load_from_memory(&bytes).map_err(|_| WorkerErrorCode::MalformedInput)?
    } else {
        // cache_key/source_path 至少给一(协议约定);都缺 = host bug。
        return Err(WorkerErrorCode::MalformedInput);
    };
    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok(DecodedImage {
        pixels: rgba.into_raw(),
        width,
        height,
        icc: None, // AI worker 子进程解码仅供推理,ICC 与此无关
    })
}

/// 处理 FaceDetectEmbed:分块全并行——块内每项在线程内跑完 解码→letterbox→检测→对齐→嵌入
/// 整链(GPU session 由池锁自串行,人脸 API 按图处理、项内多脸已批量);几何走 JSON、
/// 嵌入走 blob。`det_score_thresh` 为请求快照,覆盖 profile 默认。
pub fn handle_face(
    sess: &SessionState,
    request_id: u64,
    items: &[FaceItem],
    det_score_thresh: f32,
) -> Frame {
    let (Some(fp0), Some(detect_pool), Some(embed_pool)) = (
        sess.face_profile.as_ref(),
        sess.pool.face_detect_session.as_ref(),
        sess.pool.face_embed_session.as_ref(),
    ) else {
        // 会话未载人脸角色 → host 需带 face_profile_id 重发 SessionInit(retryable)。
        return batch_failure(
            request_id,
            WorkerErrorCode::SessionExpired,
            "会话未加载人脸角色".into(),
        );
    };
    // 阈值快照进行为参数(同图不同阈值产出不同结果,host 已将其纳入指纹)。
    let mut fp = fp0.clone();
    fp.det_score_thresh = det_score_thresh;

    // 批上限防御与 EmbedBatch 对称(face 波补,原先仅 embed 有此检查)。
    if items.len() as u32 > sess.batch_size {
        return batch_failure(
            request_id,
            WorkerErrorCode::MalformedInput,
            format!(
                "批大小 {} 超过 session 声明 {}",
                items.len(),
                sess.batch_size
            ),
        );
    }

    // 分块全并行(2026-07-03 修订,GUI 人脸批超时根因之二):块内每项在线程内跑完
    // 解码→letterbox→检测→嵌入 整链;GPU session 由池锁自串行(与进程内 rayon par_iter
    // 同构)。原「并行解码、串行推理」把 letterbox(全尺寸→640 缩放,dev 构建单张秒级)
    // 留在串行段,批耗时被它主导。
    // 并行度=探测核数(T16-R2 方案 C):host 三级定源(方案 A)后,人脸源恒为 640 级
    // 小图(RGBA ~1.6-2.5MB/张)或短边 ≤640 的小原图,原 FACE_DECODE_CHUNK=4 的驻留
    // 封顶(为全尺寸原图 ~100MB/张 而设)失义,已撤销。
    let batch_t0 = std::time::Instant::now();
    let mut results = Vec::with_capacity(items.len());
    let mut blob: Vec<u8> = Vec::new();
    let chunk_size = decode_threads(items.len());
    // 分段耗时累计(ms,2026-07-03 性能取证):解码 / 检测(含 letterbox 与 session 池
    // 等待)/ 嵌入(含对齐与池等待)。线程内逐项测量,串行段汇总,批尾一行输出。
    let (mut sum_decode, mut sum_detect, mut sum_embed) = (0u128, 0u128, 0u128);
    let mut n_ok = 0usize;
    let mut n_faces = 0usize;
    for chunk in items.chunks(chunk_size.max(1)) {
        // 线程内产出:Ok(检测+嵌入+实际解码尺寸+分段耗时) / Err((错误码, 可选日志));
        // 日志带回主线程串行输出,避免多线程交错 stderr。
        let outs = parallel_map_indexed(chunk_size, chunk.len(), |i| {
            let item = &chunk[i];
            let t0 = std::time::Instant::now();
            let decoded = load_face_image(&sess.ai_cache_dir, item).map_err(|code| (code, None))?;
            let t1 = std::time::Instant::now();
            face::detect_faces(detect_pool, &decoded, &fp)
                .and_then(|faces| {
                    let t2 = std::time::Instant::now();
                    face::embed_faces(embed_pool, &decoded, &faces, &fp)
                        .map(|embs| (faces, embs, t2))
                })
                .map(|(faces, embs, t2)| {
                    let seg_ms = [
                        t1.duration_since(t0).as_millis(),
                        t2.duration_since(t1).as_millis(),
                        t2.elapsed().as_millis(),
                    ];
                    (faces, embs, decoded.width, decoded.height, seg_ms)
                })
                .map_err(|e| {
                    (
                        WorkerErrorCode::InternalError,
                        Some(format!("item {} 人脸推理失败:{e}", item.item_id)),
                    )
                })
        });
        for (item, out) in chunk.iter().zip(outs) {
            match out {
                Ok((faces, embs, width, height, seg_ms)) => {
                    sum_decode += seg_ms[0];
                    sum_detect += seg_ms[1];
                    sum_embed += seg_ms[2];
                    n_ok += 1;
                    n_faces += faces.len();
                    // 维度红线(terminal):系统性错误,整批 Failure(同 EmbedBatch)。
                    if embs.iter().any(|e| e.len() != fp.embed_dim) {
                        return batch_failure(
                            request_id,
                            WorkerErrorCode::EmbedDimMismatch,
                            format!("人脸嵌入维度与契约 {} 不符", fp.embed_dim),
                        );
                    }
                    for emb in &embs {
                        append_embedding(&mut blob, emb);
                    }
                    results.push(FaceItemResult::Ok {
                        item_id: item.item_id,
                        fingerprint: item.fingerprint.clone(),
                        // DetectedFace 与协议 FaceDet 字段同构,逐字段搬运(0 脸也是 Ok)。
                        faces: faces
                            .iter()
                            .map(|f| FaceDet {
                                bbox: f.bbox,
                                landmarks: f.landmarks,
                                score: f.score,
                            })
                            .collect(),
                        // 实际解码尺寸:几何是本图像素坐标,host 归一化/quality 派生依赖它。
                        width,
                        height,
                    });
                }
                Err((code, msg)) => {
                    if let Some(m) = msg {
                        log_warn(m);
                    }
                    results.push(FaceItemResult::Err {
                        item_id: item.item_id,
                        fingerprint: item.fingerprint.clone(),
                        code,
                    });
                }
            }
        }
    }
    // 批诊断一行汇总:三段均值为并行链内单项耗时(检测含 letterbox+池等待,嵌入含
    // 对齐+池等待),墙钟为整批实耗。解码均值远大于检测/嵌入 ⇒ 瓶颈在 CPU 解码
    // (源过大或未优化构建),与 GPU/provider 无关。
    if n_ok > 0 {
        log_info(format!(
            "FaceDetectEmbed 批诊断:{}/{} 项 {} 脸,墙钟 {}ms(并行 {});单项均值 解码 {}ms / 检测 {}ms / 嵌入 {}ms",
            n_ok,
            items.len(),
            n_faces,
            batch_t0.elapsed().as_millis(),
            chunk_size,
            sum_decode / n_ok as u128,
            sum_detect / n_ok as u128,
            sum_embed / n_ok as u128,
        ));
    }
    if blob.len() > MAX_BLOB_LEN as usize {
        return batch_failure(
            request_id,
            WorkerErrorCode::ResourceLimit,
            format!("人脸嵌入 blob {} 字节超协议上限", blob.len()),
        );
    }

    let body = SuccessBody {
        face: Some(FaceBatchSuccess { results }),
        ..Default::default()
    };
    Frame::with_blob(FrameType::Success, request_id, &body, blob).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_whitelist_blocks_traversal() {
        let root = Path::new("C:/cache/ai");
        assert!(cache_webp_path(root, "..").is_none(), "点号必须被拒");
        assert!(cache_webp_path(root, "../../etc").is_none());
        assert!(cache_webp_path(root, "ab/cd").is_none(), "分隔符必须被拒");
        assert!(cache_webp_path(root, "ab\\cd").is_none());
        assert!(cache_webp_path(root, "").is_none());
        assert!(
            cache_webp_path(root, "ab").is_none(),
            "短于前缀长度必须被拒"
        );
        assert!(cache_webp_path(root, &"a".repeat(65)).is_none());
    }

    #[test]
    fn cache_key_valid_hex_maps_to_prefixed_path() {
        let root = Path::new("C:/cache/ai");
        let p = cache_webp_path(root, "0badf00d1234abcd").unwrap();
        // 约定:{root}/{key[..2]}/{key}.webp(与 host thumbnail::cache::ai_cache_path 同构)。
        assert!(p.ends_with(Path::new("0b").join("0badf00d1234abcd.webp")));
        assert!(p.starts_with(root));
    }

    #[test]
    fn parallel_map_preserves_order() {
        // 保序是协议契约(results/blob 按 items 序);多线程领活后必须按索引落槽。
        let n = 100usize;
        let out = parallel_map_indexed(4, n, |i| i * 3);
        assert_eq!(out, (0..n).map(|i| i * 3).collect::<Vec<_>>());
        // 单线程退化路径同样保序。
        let out1 = parallel_map_indexed(1, 5, |i| i + 10);
        assert_eq!(out1, vec![10, 11, 12, 13, 14]);
        // 空批。
        assert!(parallel_map_indexed(4, 0, |i| i).is_empty());
    }

    #[test]
    fn append_embedding_is_f32_le_layout() {
        let mut blob = Vec::new();
        append_embedding(&mut blob, &[1.0f32, -2.5]);
        append_embedding(&mut blob, &[0.25]);
        assert_eq!(blob.len(), 12, "3 × f32 = 12 字节");
        assert_eq!(&blob[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&blob[4..8], &(-2.5f32).to_le_bytes());
        assert_eq!(&blob[8..12], &0.25f32.to_le_bytes());
    }
}
