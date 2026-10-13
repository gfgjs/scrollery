//! 日志内核性能基准。
//!
//! `logging_per_event_cost` —— 每事件成本两态:off 档(interest 缓存 never)与通过 filter
//!    全链路(JSONL 序列化+入队,写向 io::sink 隔离磁盘 IO 噪声)。

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use scrollery_lib::logging::EnvelopeFormat;

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

    // B) 通过 filter,JSONL 序列化 + 入队(serde_json 序列化在业务线程,写盘在后台线程)。
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

fn bench_all(c: &mut Criterion) {
    let (handle, _guard) = init_subscriber();
    bench_per_event_cost(c, &handle);
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
