// src-tauri/benches/logging.rs
//! 日志内核性能基准(日志能力重构 S1,方案 §3.2/§9.4)。数字回写方案 §3.2 表。
//!
//! 两组基准:
//! 1. `logging_per_event_cost` —— 每事件成本三态:off 档(interest 缓存 never)/ target 被
//!    directive 压掉 / 通过 filter 全链路(JSONL 序列化+入队,写向 io::sink 隔离磁盘 IO 噪声)。
//! 2. `logging_pipeline_thumbnail_batch` —— 真实缩略图流水线(CPU 解码+编码)批量 N 张,
//!    off/info/debug 三档吞吐对比;off 态相对 info/debug 的差值即日志系统本身开销占比,
//!    不达标(>1%)视为未完成(方案 §3.2 验收基准)。

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use scrollery_lib::db::models::MediaItem;
use scrollery_lib::engine::EngineArena;
use scrollery_lib::logging::EnvelopeFormat;
use scrollery_lib::thumbnail::{
    decode_media_step, encode_media_step_with_snapshot, process_deferred_cpu, DecodeResult,
    ThumbConfig,
};

/// 全进程只 init 一次(tracing 全局默认 subscriber 只能设一次);用 reload::Handle 在各基准间切档。
/// writer 套 `tracing_appender::non_blocking` 而非裸 `io::sink`——生产路径的「序列化+入队」
/// 成本包含 NonBlocking::write 的 crossbeam try_send + 整行拷贝,裸 sink 会跳过这段,低估真实
/// 每事件成本(reviewer 审出的方法论问题)。guard 需存活到基准全部跑完,由调用方持有。
fn init_subscriber() -> (
    tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>,
    tracing_appender::non_blocking::WorkerGuard,
) {
    let env_filter = EnvFilter::new("info");
    let (filter, handle) = tracing_subscriber::reload::Layer::new(env_filter);
    let session_id: Arc<str> = Arc::from("s-benchmark");
    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::sink());
    let file_layer = tracing_subscriber::fmt::layer()
        .event_format(EnvelopeFormat { session_id })
        .with_ansi(false)
        .with_writer(non_blocking);
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .init();
    (handle, guard)
}

fn synthetic_rgba(width: u32, height: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
    })
}

fn make_item(id: i64, cache_key: i64, file_size: i64) -> MediaItem {
    MediaItem {
        id,
        directory_id: 1,
        file_name: "sample.png".to_string(),
        file_size,
        file_mtime: 0,
        file_format: "png".to_string(),
        media_type: "image".to_string(),
        width: 256,
        height: 256,
        duration_ms: None,
        sort_datetime: 0,
        cache_key,
        thumb_status: 0,
        thumb_path: None,
        thumbhash: None,
        is_favorited: false,
        is_deleted: false,
        deleted_at: None,
        rating: 0,
        color_label: 0,
        view_rotation: 0,
        playback_position_ms: 0,
        is_live_photo: false,
        has_embedded_video: false,
        companion_of: None,
        content_hash: None,
        source_revision: 1,
        created_at: 0,
        updated_at: 0,
    }
}

fn bench_per_event_cost(
    c: &mut Criterion,
    handle: &tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>,
) {
    let mut group = c.benchmark_group("logging_per_event_cost");

    // A) 完全关闭(off 档):callsite Interest::never 缓存,字段表达式不求值。
    handle
        .modify(|f| *f = EnvFilter::new("off"))
        .expect("reload to off");
    group.bench_function("off", |b| {
        b.iter(|| {
            tracing::info!(path = %"scan/root/a.jpg", size = 12345u64, "thumbnail generated");
        });
    });

    // B) target 被 directive 压掉(流水线默认降噪场景):info 档下 pipeline target 的 debug! 不通过。
    handle
        .modify(|f| *f = EnvFilter::new("info,bench::pipeline=warn"))
        .expect("reload to filtered");
    group.bench_function("filtered_by_target", |b| {
        b.iter(|| {
            tracing::debug!(target: "bench::pipeline", path = %"scan/root/a.jpg", "cache hit");
        });
    });

    // C) 通过 filter,JSONL 序列化 + 入队(serde_json 序列化在业务线程,写盘在后台线程)。
    handle
        .modify(|f| *f = EnvFilter::new("info"))
        .expect("reload to info");
    group.bench_function("enabled_full_path", |b| {
        b.iter(|| {
            tracing::info!(path = %"scan/root/a.jpg", size = 12345u64, "thumbnail generated");
        });
    });

    group.finish();
}

/// 真实缩略图流水线宏基准(方案 §3.2 验收基准):批量 N 张走 CPU 解码+编码全链路,
/// 三档(off/info/debug)吞吐对比。每张图 cache_key 递增,强制走 CACHE_MISS → 全解码路径
/// (thumb_status=0、skip_max_bytes=0、strategy=cpu),覆盖 decode_media_step_inner 的
/// trace!/info! 调用点。
fn bench_pipeline_thumbnail_batch(
    c: &mut Criterion,
    handle: &tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>,
) {
    const BATCH_N: i64 = 30;

    let temp = tempfile::tempdir().expect("create tempdir");
    let img_path = temp.path().join("sample.png");
    synthetic_rgba(256, 256)
        .save(&img_path)
        .expect("write synthetic png");
    let file_size = std::fs::metadata(&img_path)
        .map(|m| m.len() as i64)
        .unwrap_or(1);

    let cache_dir = temp.path().join("cache");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");

    let arena = EngineArena::phase1();
    let config = ThumbConfig {
        cache_dir,
        size: 256,
        skip_max_bytes: 0,
        strategy: "cpu".to_string(),
        gpu_engine: "wic".to_string(),
        ai_hq_cache: false,
        webp_quality: 80,
        ai_cache_short_edge: 336,
    };

    let mut group = c.benchmark_group("logging_pipeline_thumbnail_batch");
    // 每样本已含 BATCH_N 张真实解码+编码,采样数降低以控制总耗时。
    group.sample_size(20);

    for directive in ["off", "info", "debug"] {
        handle
            .modify(|f| *f = EnvFilter::new(directive))
            .expect("reload directive");
        group.bench_function(directive, |b| {
            b.iter(|| {
                for i in 0..BATCH_N {
                    let item = make_item(i, i, file_size);
                    // 与生产调用点同款两步:decode_media_step 判缓存/直显/分派,编码与
                    // 延迟 CPU 收尾各自走既有入口(镜像 ipc/thumbnail_full_gen.rs)。
                    match decode_media_step(&item, &img_path, &arena, &config) {
                        Ok(DecodeResult::ToEncode {
                            item_id,
                            source_revision,
                            cache_key,
                            decoded,
                        }) => {
                            let _ = encode_media_step_with_snapshot(
                                item_id,
                                source_revision,
                                cache_key,
                                decoded,
                                &config,
                            );
                        }
                        Ok(DecodeResult::DeferredToCpu { item, abs_path }) => {
                            let _ = process_deferred_cpu(&item, &abs_path, &arena, &config);
                        }
                        // Ready(缓存命中/直显/非图像)与 Err 均无后续编解码步骤。
                        _ => {}
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_all(c: &mut Criterion) {
    let (handle, _guard) = init_subscriber();
    bench_per_event_cost(c, &handle);
    bench_pipeline_thumbnail_batch(c, &handle);
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
