param(
    [string]$DependencyDirectory = 'C:/workspace/scrollery-private/target/debug/deps'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path.Replace('\', '/')
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('scrollery-portable-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($temporary) | Out-Null
# 只复用已编译依赖；真实 portable_execution/ByteBudget/metrics 原样纳入 rustc。
# 宿主、ImageRs 和编码器使用契约替身，因此本脚本不证明真实 codec 或非 Windows 链接可用。
$source = @'
#![allow(dead_code)]
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
static CODEC_ENTERED: AtomicBool = AtomicBool::new(false);
static CODEC_RELEASE: AtomicBool = AtomicBool::new(false);
mod error {
    #[derive(Debug)]
    pub enum AppError { Io(std::io::Error), Engine(image::ImageError), UnsupportedFormat(String) }
}
mod state {
    use super::*;
    pub struct AppState { pub thumb_config: RwLock<crate::thumbnail::generator::ThumbConfig>, pub epoch: AtomicU64 }
    impl AppState {
        pub fn with_database_lifecycle_read<T>(&self, epoch: u64, action: impl FnOnce() -> T) -> Option<T> {
            (epoch == self.epoch.load(Ordering::Acquire)).then(action)
        }
    }
}
mod export {
    pub fn open_authorized_source(root: &str, _: &str, name: &str) -> std::io::Result<std::fs::File> {
        std::fs::File::open(std::path::Path::new(root).join(name))
    }
}
mod engine {
    pub mod traits {
        #[derive(Clone, Copy)] pub enum ResizeHint { LongEdge(u32), ShortEdge(u32) }
        pub struct DecodedImage { pub width: u32, pub height: u32, pub pixels: Vec<u8>, pub icc: Option<Vec<u8>> }
    }
    pub mod image_rs {
        use super::traits::*;
        use crate::error::AppError;
        use std::io::{Read, Seek};
        pub struct ImageRsEngine;
        pub struct ImageWorkingSet { pub width: u32, pub height: u32 }
        impl ImageWorkingSet { pub fn bytes_for_resize(&self, _: Option<ResizeHint>) -> Result<u64, AppError> { Ok(32) } }
        pub fn resize_dimensions(w: u32, h: u32, _: Option<ResizeHint>) -> Result<(u32,u32), AppError> { Ok((w,h)) }
        impl ImageRsEngine {
            pub fn working_set_bytes(_: &mut std::fs::File, _: u64) -> Result<ImageWorkingSet, AppError> {
                Ok(ImageWorkingSet { width: 2, height: 2 })
            }
            pub fn decode_open_file_bounded(file: &mut std::fs::File, _: &str, _: Option<ResizeHint>, _: u64) -> Result<DecodedImage, AppError> {
                file.rewind().unwrap();
                let mut byte = [0]; file.read_exact(&mut byte).unwrap();
                match byte[0] {
                    0 => panic!("injected decoder panic"),
                    1 => return Err(AppError::Engine(image::ImageError::IoError(std::io::ErrorKind::UnexpectedEof.into()))),
                    4 => {
                        crate::CODEC_ENTERED.store(true, std::sync::atomic::Ordering::Release);
                        while !crate::CODEC_RELEASE.load(std::sync::atomic::Ordering::Acquire) { std::thread::yield_now(); }
                    }
                    _ => (),
                }
                Ok(DecodedImage { width: 2, height: 2, pixels: vec![byte[0];16], icc: None })
            }
        }
    }
}
mod thumbnail {
    use super::*;
    #[path = "__ROOT__/src-tauri/src/thumbnail/byte_budget.rs"] pub mod byte_budget;
    #[path = "__ROOT__/src-tauri/src/thumbnail/native_protocol.rs"] pub mod native_protocol;
    #[path = "__ROOT__/src-tauri/src/thumbnail/benchmark.rs"] pub mod benchmark;
    #[path = "__ROOT__/src-tauri/src/thumbnail/portable_execution.rs"] pub mod portable_execution;
    pub mod qos { pub fn refresh_worker_qos(_: &mut Option<bool>) {} }
    pub mod generator {
        use super::*;
        #[derive(Clone, Default, PartialEq)]
        pub struct ThumbConfig { pub size: u32, pub ai_cache_short_edge: u32 }
        impl ThumbConfig { pub fn same_policy(&self, other: &Self) -> bool { self == other } }
        pub struct EncodedImage { pub display: Vec<u8>, pub ai: Option<Vec<u8>>, pub thumbhash: Option<Vec<u8>> }
        pub fn encode_image_payload(image: crate::engine::traits::DecodedImage, _: Option<&[u8]>, _: &ThumbConfig, _: bool, _: bool, _: Option<&mut benchmark::ItemMetrics>) -> Result<EncodedImage, crate::error::AppError> {
            if image.pixels[0] == 2 { panic!("injected encoder panic"); }
            Ok(EncodedImage { display: image.pixels, ai: None, thumbhash: None })
        }
    }
    pub mod service {
        use super::*;
        pub type TaskKey = u64;
        pub struct Item { pub file_name: String, pub file_format: String }
        pub struct ImageRequest { pub root_path: String, pub relative_dir: String, pub item: Item, pub config: generator::ThumbConfig, pub epoch: u64, pub measured: bool, pub cached_result: Option<u8> }
        pub struct Job { pub key: TaskKey, pub id: u64, pub request: ImageRequest, pub metrics: benchmark::WorkMetrics, pub permits: Option<Arc<AtomicU64>> }
        impl Job { pub fn decoded(&mut self) { if let Some(permits) = self.permits.take() { permits.fetch_sub(1, Ordering::AcqRel); } } }
        pub struct Shared {
            pub queue: Mutex<std::collections::VecDeque<Job>>,
            pub stopped: AtomicBool,
            pub cancelled: Mutex<std::collections::HashSet<u64>>,
            pub results: crossbeam_channel::Sender<(u64, Option<u8>)>,
        }
        impl Shared {
            pub fn interested(&self, _: &TaskKey, id: u64) -> bool { !self.stopped() && !self.cancelled.lock().unwrap().contains(&id) }
            pub fn stopped(&self) -> bool { self.stopped.load(Ordering::Acquire) }
            pub fn try_take(&self) -> Option<Job> { self.queue.lock().unwrap().pop_front() }
            pub fn wait(&self) { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        pub fn failed_result(_: &Item) -> u8 { 2 }
        pub fn finish_with_ai(shared: &Shared, mut job: Job, result: Option<u8>, _: Option<String>) {
            job.decoded();
            shared.results.send((job.id,result)).unwrap();
        }
    }
    pub mod execution {
        use super::*;
        pub struct WriteWork { pub job: service::Job, pub payload: generator::EncodedImage, pub bytes: byte_budget::BytePermit }
        pub fn direct(_: &service::ImageRequest) -> bool { false }
        pub fn emit_ai(_: &service::ImageRequest) -> bool { false }
        pub fn needs_pixels(_: &service::ImageRequest) -> bool { true }
        pub fn valid(shared: &service::Shared, state: &std::sync::Weak<crate::state::AppState>, job: &service::Job) -> bool {
            shared.interested(&job.key, job.id) && state.upgrade().is_some_and(|state| state.with_database_lifecycle_read(job.request.epoch, || job.request.config.same_policy(&state.thumb_config.read().unwrap())).unwrap_or(false))
        }
        pub fn finish_ready(shared: &service::Shared, job: service::Job) { service::finish_with_ai(shared, job, Some(1), None); }
    }
    #[test]
    fn portable_pools_continue_after_decode_and_encode_panics() {
        let root = std::env::var("PORTABLE_TEST_TEMP").unwrap();
        let config = generator::ThumbConfig { size: 64, ai_cache_short_edge: 336 };
        let state = Arc::new(crate::state::AppState { thumb_config: RwLock::new(config.clone()), epoch: AtomicU64::new(1) });
        let (results, finished) = crossbeam_channel::unbounded();
        let permits = Arc::new(AtomicU64::new(4));
        let queue = (0..4).map(|id| {
            let name = format!("{id}.png");
            std::fs::write(std::path::Path::new(&root).join(&name), [id as u8]).unwrap();
            service::Job { key: id, id, request: service::ImageRequest { root_path: root.clone(), relative_dir: String::new(), item: service::Item { file_name: name, file_format: "png".into() }, config: config.clone(), epoch: 1, measured: true, cached_result: None }, metrics: benchmark::WorkMetrics::default(), permits: Some(Arc::clone(&permits)) }
        }).collect();
        let shared = Arc::new(service::Shared { queue: Mutex::new(queue), stopped: AtomicBool::new(false), cancelled: Mutex::default(), results });
        let (sender, writer) = crossbeam_channel::bounded(1);
        portable_execution::start(&state, Arc::clone(&shared), sender, Arc::default(), 1);
        let work = writer.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        assert_eq!(work.job.id, 3);
        assert_eq!(work.payload.display, vec![3;16]);
        assert_eq!(permits.load(Ordering::Acquire), 0, "源许可必须在写入前已释放");
        let mut completions = (0..3).map(|_| finished.recv_timeout(std::time::Duration::from_secs(1)).unwrap()).collect::<Vec<_>>();
        completions.sort_by_key(|entry| entry.0);
        assert_eq!(completions, [(0,None),(1,Some(2)),(2,None)]);
        shared.stopped.store(true, Ordering::Release);
        drop(work);
    }

    #[test]
    fn epoch_change_during_codec_discards_old_work_and_keeps_pool_alive() {
        let root = std::env::var("PORTABLE_TEST_TEMP").unwrap();
        std::fs::write(std::path::Path::new(&root).join("epoch.png"), [4]).unwrap();
        let config = generator::ThumbConfig { size: 64, ai_cache_short_edge: 336 };
        let state = Arc::new(crate::state::AppState { thumb_config: RwLock::new(config.clone()), epoch: AtomicU64::new(1) });
        let (results, finished) = crossbeam_channel::unbounded();
        let permits = Arc::new(AtomicU64::new(1));
        let job = service::Job { key: 4, id: 4, request: service::ImageRequest { root_path: root.clone(), relative_dir: String::new(), item: service::Item { file_name: "epoch.png".into(), file_format: "png".into() }, config, epoch: 1, measured: false, cached_result: None }, metrics: benchmark::WorkMetrics::default(), permits: Some(Arc::clone(&permits)) };
        let shared = Arc::new(service::Shared { queue: Mutex::new([job].into()), stopped: AtomicBool::new(false), cancelled: Mutex::default(), results });
        let (sender, writer) = crossbeam_channel::bounded(1);
        portable_execution::start(&state, Arc::clone(&shared), sender, Arc::default(), 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !CODEC_ENTERED.load(Ordering::Acquire) && std::time::Instant::now() < deadline { std::thread::yield_now(); }
        let entered = CODEC_ENTERED.load(Ordering::Acquire);
        assert_eq!(permits.load(Ordering::Acquire), 1, "codec 还在执行时保留源许可");
        state.epoch.store(2, Ordering::Release);
        CODEC_RELEASE.store(true, Ordering::Release);
        assert!(entered);
        assert_eq!(finished.recv_timeout(std::time::Duration::from_secs(3)).unwrap(), (4,None));
        assert_eq!(permits.load(Ordering::Acquire), 0);
        assert!(writer.try_recv().is_err());
        shared.stopped.store(true, Ordering::Release);
    }
}
'@
$source = $source.Replace('__ROOT__', $repo)
$harness = Join-Path $temporary 'portable_harness.rs'
[IO.File]::WriteAllText($harness, $source)
$imageLibrary = Get-ChildItem $DependencyDirectory -Filter 'libimage-*.rlib' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$channelLibrary = Get-ChildItem $DependencyDirectory -Filter 'libcrossbeam_channel-*.rlib' | Select-Object -First 1
$serdeLibrary = Get-ChildItem $DependencyDirectory -Filter 'libserde-*.rlib' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$jsonLibrary = Get-ChildItem $DependencyDirectory -Filter 'libserde_json-*.rlib' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (!$imageLibrary -or !$channelLibrary -or !$serdeLibrary -or !$jsonLibrary) { throw '需要现有 image/crossbeam-channel/serde/serde_json rlib；不会安装或构建依赖。' }
$executable = Join-Path $temporary 'portable_harness.exe'
$env:PORTABLE_TEST_TEMP = $temporary
try {
    & rustc --edition 2021 --test $harness --crate-name portable_harness -L "dependency=$DependencyDirectory" --extern "image=$($imageLibrary.FullName)" --extern "crossbeam_channel=$($channelLibrary.FullName)" --extern "serde=$($serdeLibrary.FullName)" --extern "serde_json=$($jsonLibrary.FullName)" -o $executable
    if ($LASTEXITCODE -ne 0) { throw '局部 Rust 编译失败' }
    & $executable
    if ($LASTEXITCODE -ne 0) { throw '局部 Rust 测试失败' }
} finally {
    Remove-Item Env:PORTABLE_TEST_TEMP -ErrorAction SilentlyContinue
    $resolved = [IO.Path]::GetFullPath($temporary)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($resolved).StartsWith('scrollery-portable-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
