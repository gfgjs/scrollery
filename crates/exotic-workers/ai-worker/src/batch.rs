// crates/exotic-workers/ai-worker/src/batch.rs
//! EmbedBatch / FaceDetectEmbed 批处理；单项失败不连坐整批。
//!
//! 载运契约(与 message.rs 文档一致,host 侧按此校验):
//!   - 嵌入本体不进 JSON,走 Success 帧同帧 blob(f32 LE);
//!   - EmbedBatch blob 按 results 中 **Ok 项顺序**连续排布,每项 `embed_dim × 4` 字节;
//!   - FaceDetectEmbed blob 按 Ok 项顺序、项内按 faces 顺序,每脸 `face_embed_dim × 4` 字节;
//!   - 维度不符 = EmbedDimMismatch(terminal,系统性错误必然全批皆错 → 整批 Failure)。
//!
//! 路径安全:EmbedBatch 可消费宿主指定的已提交 v2 产物；严格校验组件与缓存键，
//! 通过共用授权打开固定句柄后读取。None 保留旧 cache_key 路径。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use exotic_protocol::{
    EmbedBatchSuccess, EmbedItem, EmbedResult, FaceBatchSuccess, FaceDet, FaceItem, FaceItemResult,
    FailureBody, Frame, FrameType, SuccessBody, TextEmbedSuccess, WorkerErrorCode, MAX_BLOB_LEN,
};
use ndarray::Array4;
use scrollery_ai_core::{clip, face, DecodedImage};

use crate::session::SessionState;

#[path = "../../../shared/authorized_file.rs"]
mod authorized_file;

/// source_path 回退解码的源文件字节上限(与 psd-worker 同值;拦巨文件吃满内存)。
const MAX_SOURCE_FILE_BYTES: u64 = 512 << 20;

/// source_path 回退解码的声明像素上限(100 兆像素 ≈ 10000×10000)。**与
/// ocr.rs::MAX_SOURCE_PIXELS 同值同理由,勿双向漂移**:`MAX_SOURCE_FILE_BYTES`
/// 的 stat 只拦压缩字节数——一张 30MB PNG 可解出 400MB+ RGBA。先用
/// `ImageReader::into_dimensions` 只解头拿声明尺寸(不解像素数据),超限即该项
/// `ResourceLimit`,不进入真正解码分配。
const MAX_SOURCE_PIXELS: u64 = 100_000_000;

/// 动态模型的推理子批上限；固定模型按真实 batch 收集，仅尾批补齐。
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

// WorkerLogLine 行协议输出；裸 eprintln 会被 supervisor 按未解析行告警转发。
// 定级同 main.rs 原则:批诊断=info、推理/单项失败(worker 继续服务)=warn。
fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}

/// EmbedBatch 性能汇总的 opt-in 开关:环境变量 `SCROLLERY_AI_WORKER_PERF` 取 1/true/yes 时
/// 每请求输出一行 `EMBED_PERF` 分段汇总(每请求一条,不逐项刷日志);未开启时零日志。
/// 计时本身恒开(Instant::now 级开销,见 [`EmbedTiming`]),只有输出被门控。
fn perf_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("SCROLLERY_AI_WORKER_PERF").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        )
    })
}

/// EmbedBatch 分段计时累加(µs)。decode/preprocess 在解码线程内各自累加(线程并行,求和值
/// 可超墙钟,仅表单项成本);供料等待/攒批/组批/编码在主线程串行累加,四项之和应≈墙钟。
#[derive(Default)]
struct EmbedTiming {
    /// 主线程阻塞等首个可推理项(`rx.recv`)的累计时间 = 供料跟不上推理的空洞。
    wait_first_us: u64,
    /// 动态模型非阻塞攒子批的累计时间；固定模型的组批等待计入 wait_first_us。
    drain_us: u64,
    /// 组批复制:新建 [n,3,S,S] 张量 + 逐项平坦 memcpy。
    assemble_us: u64,
    /// 提交给编码的子批数。
    groups: u32,
}

/// 固定模型等齐真实 batch，动态模型只收已就绪张量；错误不占有效容量。
fn collect_infer_group<T>(
    rx: &std::sync::mpsc::Receiver<(usize, Result<T, WorkerErrorCode>)>,
    errors: &mut [Option<WorkerErrorCode>],
    clock: &mut clip::StageClock<'_, EmbedTiming>,
    fixed_batch: Option<usize>,
) -> Vec<(usize, T)> {
    let cap = fixed_batch.unwrap_or(INFER_SUB_BATCH).max(1);
    let mut pending = Vec::with_capacity(cap.min(errors.len()));
    clock.begin();
    loop {
        match rx.recv() {
            Ok((i, Ok(t))) => {
                pending.push((i, t));
                break;
            }
            Ok((i, Err(code))) => errors[i] = Some(code),
            Err(_) => break,
        }
    }
    clock.end(|t| &mut t.wait_first_us);
    clock.begin();
    while pending.len() < cap {
        // 固定 batch 的提前推理会把暂未到达的有效项替换成填充，浪费设备计算。
        if fixed_batch.is_some() {
            match rx.recv() {
                Ok((i, Ok(t))) => pending.push((i, t)),
                Ok((i, Err(code))) => errors[i] = Some(code),
                Err(_) => break,
            }
        } else {
            match rx.try_recv() {
                Ok((i, Ok(t))) => pending.push((i, t)),
                Ok((i, Err(code))) => errors[i] = Some(code),
                Err(_) => break,
            }
        }
    }
    if fixed_batch.is_some() {
        clock.end(|t| &mut t.wait_first_us);
    } else {
        clock.end(|t| &mut t.drain_us);
    }
    pending
}

/// 单张直接交出张量；多张才复制到连续批张量，并保留原请求索引。
fn assemble_infer_group(
    mut pending: Vec<(usize, Array4<f32>)>,
    side: usize,
) -> (Vec<usize>, Array4<f32>) {
    if pending.len() == 1 {
        let (i, tensor) = pending.pop().expect("已确认单张");
        return (vec![i], tensor);
    }
    let mut batch = Array4::<f32>::zeros((pending.len(), 3, side, side));
    let dst = batch.as_slice_mut().expect("零初始化张量必为标准布局");
    let stride = 3 * side * side;
    let mut indices = Vec::with_capacity(pending.len());
    for (bi, (i, tensor)) in pending.into_iter().enumerate() {
        dst[bi * stride..(bi + 1) * stride]
            .copy_from_slice(tensor.as_slice().expect("preprocess 输出为标准布局"));
        indices.push(i);
    }
    (indices, batch)
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

/// 解析 EmbedBatch 缓存路径；v2 只接受规范 short/revision 与对应的 16 位小写缓存键。
fn embed_cache_path(ai_cache_dir: &Path, item: &EmbedItem) -> Option<PathBuf> {
    let Some(relative) = item.cache_path.as_deref() else {
        return cache_webp_path(ai_cache_dir, &item.cache_key);
    };
    let parts: Vec<_> = relative.split('/').collect();
    if parts.len() != 5
        || parts[0] != "v2"
        || item.cache_key.len() != 16
        || !item
            .cache_key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || parts[2] != &item.cache_key[..2]
        || parts[4] != format!("{}.webp", item.cache_key)
    {
        return None;
    }
    let short = parts[1].parse::<u32>().ok()?;
    let revision = parts[3].strip_prefix('r')?.parse::<i64>().ok()?;
    if short == 0
        || parts[1] != short.to_string()
        || revision <= 0
        || parts[3] != format!("r{revision}")
    {
        return None;
    }
    Some(ai_cache_dir.join(relative))
}

/// 从授权缓存句柄解码，不回退到另一条路径，也不重新按路径读文件。
fn load_embed_cache_image(
    ai_cache_dir: &Path,
    item: &EmbedItem,
) -> Result<image::DynamicImage, WorkerErrorCode> {
    use std::io::Read as _;
    let path = embed_cache_path(ai_cache_dir, item).ok_or(WorkerErrorCode::MalformedInput)?;
    let relative = path
        .strip_prefix(ai_cache_dir)
        .map_err(|_| WorkerErrorCode::MalformedInput)?;
    let relative = relative
        .to_str()
        .ok_or(WorkerErrorCode::MalformedInput)?
        .replace('\\', "/");
    let mut file = authorized_file::open_relative(ai_cache_dir, &relative).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            WorkerErrorCode::MalformedInput
        } else {
            WorkerErrorCode::IoError
        }
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| WorkerErrorCode::IoError)?;
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

/// 处理 EmbedBatch：逐项并行解码，组批推理，结果与 blob 按请求序装配。
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

    // 解码/预处理与推理流水重叠：解码线程池喂有界 channel，本线程
    //      攒子批边收边推——CPU(解码)与 GPU(推理)同时有活。此前「先全解完再推」
    //      的相位交替使 GPU/CPU 利用率互为镜像(实测各仅 ~35%/50%),批耗时=两段之和;
    //      重叠后=两段取大。有界容量封顶在飞张量内存(600KB/张 × 2×threads)。
    //      失败仍逐项不连坐;结果按 items 原序落槽(与 blob 布局对齐)。
    let side = sess.profile.image_size as usize;
    let threads = decode_threads(items.len());
    let pool = match sess.pool.clip_image_session.as_ref() {
        Some(p) => p,
        // 按需会话可不含图像塔，交由 host 补载。
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
    // 计时(read-only 采样,不改流水线行为):decode/preprocess 由解码线程用原子累加,
    // 其余段在主线程串行累加;输出仅在 opt-in 时每请求一行([`perf_enabled`])。
    let perf = perf_enabled();
    let req_t0 = perf.then(std::time::Instant::now);
    let mut timing = EmbedTiming::default();
    let mut enc_timing = clip::ImageBatchTiming::default();
    // 关闭时零采集:主线程各测量点经 StageClock(None 即空操作),解码线程连时钟都不取。
    let decode_us = if perf { Some(AtomicU64::new(0)) } else { None };
    let preprocess_us = if perf { Some(AtomicU64::new(0)) } else { None };
    let mut clock = clip::StageClock::new(if perf { Some(&mut timing) } else { None });
    std::thread::scope(|s| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, Result<Array4<f32>, WorkerErrorCode>)>(
            threads * 2,
        );
        for _ in 0..threads {
            let tx = tx.clone();
            let cursor = &cursor;
            let decode_us = &decode_us;
            let preprocess_us = &preprocess_us;
            s.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                let t_decode = perf.then(std::time::Instant::now);
                let loaded = load_embed_cache_image(&sess.ai_cache_dir, &items[i]);
                if let (Some(t0), Some(acc)) = (t_decode, decode_us.as_ref()) {
                    acc.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
                let r = loaded.map(|img| {
                    let t_preprocess = perf.then(std::time::Instant::now);
                    let t = clip::preprocess_image(&img, &sess.profile);
                    if let (Some(t0), Some(acc)) = (t_preprocess, preprocess_us.as_ref()) {
                        acc.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
                    }
                    t
                });
                if tx.send((i, r)).is_err() {
                    break; // 推理侧已终止(批级失败提前收摊)
                }
            });
        }
        drop(tx); // 主线程不发;解码线程全退后 recv 得 Disconnected 即收尾

        loop {
            let pending =
                collect_infer_group(&rx, &mut item_err, &mut clock, sess.image_fixed_batch);
            if pending.is_empty() {
                break; // 所有生产线程结束且没有残余有效项。
            }

            clock.begin();
            let (indices, batch) = assemble_infer_group(pending, side);
            clock.end(|t| &mut t.assemble_us);
            clock.count(|t| &mut t.groups);
            let enc_out = if perf { Some(&mut enc_timing) } else { None };
            match clip::encode_image_batch_timed(pool, batch, &sess.profile, enc_out) {
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
                    for (i, v) in indices.into_iter().zip(vecs) {
                        embeds[i] = Some(v);
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
    drop(clock); // 结束对 timing 的可变借用,下方汇总才可读
                 // 分段汇总(每请求一条)。读法:`wait_input_us` 与 `enc_*` 相加≈`wall_us`(供料与
                 // 推理串行);`wall_us - wait_input_us - enc_run_us` 即主线程固定开销(组批复制等)。
    if perf && !items.is_empty() {
        let wall_us = req_t0
            .map(|t0| t0.elapsed().as_micros() as u64)
            .unwrap_or(0);
        let decode_us = decode_us
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(0);
        let preprocess_us = preprocess_us
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(0);
        let n_ok = embeds.iter().filter(|e| e.is_some()).count();
        log_info(format!(
            "EMBED_PERF req={request_id} items={} ok={n_ok} err={} fail={} groups={} threads={threads} \
             wall_us={wall_us} decode_us={} preprocess_us={} wait_input_us={} drain_us={} \
             assemble_us={} enc_pool_us={} enc_prep_us={} enc_run_us={} enc_extract_us={} \
             enc_runs={} fixed_batch={:?} ms_per_item={:.3}",
            items.len(),
            items.len() - n_ok,
            u8::from(batch_fail.is_some()),
            timing.groups,
            decode_us,
            preprocess_us,
            timing.wait_first_us,
            timing.drain_us,
            timing.assemble_us,
            enc_timing.pool_wait_us,
            enc_timing.prep_us,
            enc_timing.run_us,
            enc_timing.extract_us,
            enc_timing.runs,
            sess.image_fixed_batch,
            wall_us as f64 / 1000.0 / items.len() as f64,
        ));
    }
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

/// 处理 EncodeText：逐条编码，全批原子（文本编码无逐项 IO 失败模式，任一失败
/// 即整批 Failure)。blob 按 texts 顺序连续排布,每项 `embed_dim × f32(LE)`;文本塔 EP
/// 恒为 CPU（由 AiEnginePool 构建时确定）。
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
        // 按需会话可不含文本塔，交由 host 补载。
        None => {
            return batch_failure(
                request_id,
                WorkerErrorCode::SessionExpired,
                "CLIP 文本编码器不在会话中".into(),
            )
        }
    };

    let Some(tokenizer) = sess.tokenizer.as_ref() else {
        return batch_failure(
            request_id,
            WorkerErrorCode::SessionExpired,
            "CLIP 分词器不在会话中".into(),
        );
    };

    let mut blob: Vec<u8> = Vec::with_capacity(texts.len() * sess.profile.embed_dim * 4);
    for text in texts {
        match clip::encode_text(pool, tokenizer, text, &sess.profile) {
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
    let img = if let Some(relative) = &item.cache_path {
        use std::io::Read as _;
        if item.source_path.is_some()
            || item.cache_key.is_some()
            || !(relative.starts_with("thumbnails/") || relative.starts_with("face_thumbs/v2/"))
        {
            return Err(WorkerErrorCode::MalformedInput);
        }
        let root = ai_cache_dir
            .parent()
            .ok_or(WorkerErrorCode::MalformedInput)?;
        let file = authorized_file::open_relative(root, relative).map_err(|error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                WorkerErrorCode::MalformedInput
            } else {
                WorkerErrorCode::IoError
            }
        })?;
        if file.metadata().map_err(|_| WorkerErrorCode::IoError)?.len() > MAX_SOURCE_FILE_BYTES {
            return Err(WorkerErrorCode::ResourceLimit);
        }
        let mut bytes = Vec::new();
        file.take(MAX_SOURCE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| WorkerErrorCode::IoError)?;
        if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
            return Err(WorkerErrorCode::ResourceLimit);
        }
        decode_face_bytes(&bytes)?
    } else if let Some(key) = &item.cache_key {
        load_cache_image(ai_cache_dir, key)?
    } else if let Some(src) = &item.source_path {
        // 信任语义同 Thumbnail.source_path(host 提供绝对路径);读盘前 stat 拦巨文件,
        // 解码前按声明尺寸拦像素(镜像 ocr.rs::load_ocr_image 的像素级设防)。
        let meta = std::fs::metadata(src).map_err(|_| WorkerErrorCode::IoError)?;
        if meta.len() > MAX_SOURCE_FILE_BYTES {
            return Err(WorkerErrorCode::ResourceLimit);
        }
        let bytes = std::fs::read(src).map_err(|_| WorkerErrorCode::IoError)?;
        decode_face_bytes(&bytes)?
    } else {
        return Err(WorkerErrorCode::MalformedInput);
    };
    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok(DecodedImage {
        pixels: rgba.into_raw(),
        width,
        height,
        icc: None,
    })
}

fn decode_face_bytes(bytes: &[u8]) -> Result<image::DynamicImage, WorkerErrorCode> {
    // 只解头拿声明尺寸：超限在真正解码分配之前拦下。
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| WorkerErrorCode::MalformedInput)?
        .into_dimensions()
        .map_err(|_| WorkerErrorCode::MalformedInput)?;
    if (w as u64) * (h as u64) > MAX_SOURCE_PIXELS {
        return Err(WorkerErrorCode::ResourceLimit);
    }
    image::load_from_memory(bytes).map_err(|_| WorkerErrorCode::MalformedInput)
}

/// 处理 FaceDetectEmbed:整批并行领活，每项在线程内跑完 解码→letterbox→检测→对齐→嵌入
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

    // 每项在线程内完成解码→letterbox→检测→嵌入，避免预处理滞留串行段；
    // GPU session 由池锁串行。人脸源已由 host 限为 640 级小图或小原图，
    // 因此按探测核数并行，不再用全尺寸原图的驻留上限约束批大小。
    let batch_t0 = std::time::Instant::now();
    let mut results = Vec::with_capacity(items.len());
    let mut blob: Vec<u8> = Vec::new();
    // 整批共用一次领活，避免每块最慢项阻止后继项启动。线程数与原来一致，解码图
    // 仍在项内释放；结果槽保留整批几何/向量，随后按输入顺序装配协议结果。
    let threads = decode_threads(items.len());
    // 分段耗时累计(ms,2026-07-03 性能取证):解码 / 检测(含 letterbox 与 session 池
    // 等待)/ 嵌入(含对齐与池等待)。线程内逐项测量,串行段汇总,批尾一行输出。
    let (mut sum_decode, mut sum_detect, mut sum_embed) = (0u128, 0u128, 0u128);
    let mut n_ok = 0usize;
    let mut n_faces = 0usize;
    // 线程内产出:Ok(检测+嵌入+实际解码尺寸+分段耗时) / Err((错误码, 可选日志));
    // 日志带回主线程串行输出,避免多线程交错 stderr。
    let outs = parallel_map_indexed(threads, items.len(), |i| {
        let item = &items[i];
        let t0 = std::time::Instant::now();
        let decoded = load_face_image(&sess.ai_cache_dir, item).map_err(|code| (code, None))?;
        let t1 = std::time::Instant::now();
        face::detect_faces(detect_pool, &decoded, &fp)
            .and_then(|faces| {
                let t2 = std::time::Instant::now();
                face::embed_faces(embed_pool, &decoded, &faces, &fp).map(|embs| (faces, embs, t2))
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
    for (item, out) in items.iter().zip(outs) {
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
            threads,
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
    fn assembled_inputs_keep_arrival_and_output_index_mapping() {
        let a = Array4::from_shape_vec((1, 3, 1, 1), vec![1., 2., 3.]).unwrap();
        let b = Array4::from_shape_vec((1, 3, 1, 1), vec![4., 5., 6.]).unwrap();
        let (indices, batch) = assemble_infer_group(vec![(7, a), (2, b)], 1);
        assert_eq!(indices, vec![7, 2]);
        assert_eq!(batch.dim(), (2, 3, 1, 1));
        assert_eq!(batch.as_slice().unwrap(), &[1., 2., 3., 4., 5., 6.]);
    }

    #[test]
    fn infer_group_returns_before_a_live_slow_producer() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send((0, Ok(7))).unwrap();
        let mut errors = vec![None];
        assert_eq!(
            collect_infer_group(&rx, &mut errors, &mut clip::StageClock::new(None), None),
            vec![(0, 7)]
        );
        drop(tx);
    }

    // 每例只清理自己以 create_new 创建的文件，不递归删除临时目录。

    fn face_item_source(path: &Path) -> FaceItem {
        FaceItem {
            item_id: 1,
            cache_key: None,
            cache_path: None,
            source_path: Some(path.to_string_lossy().into_owned()),
            fingerprint: "fp".into(),
        }
    }

    #[test]
    fn face_cached_path_uses_authorized_root_and_rejects_mixed_or_traversal_input() {
        let root = std::env::temp_dir().join(format!("ai-face-cache-{}", std::process::id()));
        let thumbs = root.join("thumbnails");
        std::fs::create_dir_all(&thumbs).unwrap();
        let path = thumbs.join("test.webp");
        image::RgbaImage::new(2, 3)
            .save_with_format(&path, image::ImageFormat::WebP)
            .unwrap();
        let mut item = face_item_source(&path);
        item.source_path = None;
        item.cache_path = Some("thumbnails/test.webp".into());
        let img = load_face_image(&root.join("ai_thumbs"), &item).unwrap();
        assert_eq!((img.width, img.height), (2, 3));
        item.source_path = Some(path.to_string_lossy().into_owned());
        assert_eq!(
            load_face_image(&root.join("ai_thumbs"), &item).unwrap_err(),
            WorkerErrorCode::MalformedInput
        );
        item.source_path = None;
        item.cache_path = Some("thumbnails/../test.webp".into());
        assert_eq!(
            load_face_image(&root.join("ai_thumbs"), &item).unwrap_err(),
            WorkerErrorCode::MalformedInput
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(thumbs).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
