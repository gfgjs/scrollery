# Architecture Notes | 架构笔记

> 📦 **已归档(2026-07-10 文档治理补标)**:Part0 §0.3 降历史参考;进程/线程/数据流不变量部分仍可引用,但整体以 refactor_2026 正文与代码为准。

Living summary of how Picasa Next is put together. For the full original design see
`implementation_plan_v1.2.md`; for the million-scale hardening work + progress see
`perf_hardening_plan_v2.md`.

当前架构的活文档。完整原始设计见 `implementation_plan_v1.2.md`；百万级性能加固与进度见
`perf_hardening_plan_v2.md`。

## Process & threading | 进程与线程

- **tokio** handles async IPC commands, channels, timers.
- **rayon** + `spawn_blocking` handle CPU-bound work (scan dimension extraction, thumbnail
  decode/encode, justified layout, cosine similarity). Never `.await` inside rayon.
- **DB access**: a single write `Mutex<Connection>` serialises writes; a `r2d2` read pool
  (4 connections) serves concurrent reads under WAL. Lock order is always **db_writer →
  layout_cache** (or independent); never the reverse, to avoid deadlock.

## Data flow: browsing | 数据流：浏览

1. `compute_layout` (IPC) → `query_layout_geometry`-style query (id/w/h/sort_datetime/
   grouping + render-essential fields only) → `compute_justified_layout` (rayon, in
   `spawn_blocking`) → stored in the in-memory `LayoutCacheData` with an `id → (row,col)`
   index + flat id list. Returns a summary (rows/height/version/separators).
2. Frontend virtual scroll → `get_layout_rows_by_y(topY, bottomY)` returns only the
   visible rows from the cache.
3. Heavy per-item metadata (EXIF/GPS/filename/dir path) is NOT in the cache — the grid
   lazily fetches it for the visible window via `get_meta_for_viewport(ids)` only when the
   card info overlay is enabled.
4. Thumbnails: the frontend batches `item_id`s → `batch_request_thumbnails` → rayon
   decode/encode → results written to DB **and** patched into the layout cache in
   **O(batch)** via the index (`apply_thumb_results`).

## Key invariants | 关键不变量

- `scrollHeight == totalHeight` (normal mode) — otherwise the browser clamps scrollTop and
  triggers an endless fetch loop. Large libraries use coordinate translation instead.
- All user-facing queries append `is_deleted = 0 AND companion_of IS NULL`.
- `cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`; thumbnails live under
  `cache/thumbnails/{size}/{2-hex-prefix}/{cache_key_hex}.webp`.
- Layout-cache mutations: thumbnail status + `is_favorited` are synced in-place by id
  (O(1)); set changes (soft-delete/restore) trigger a full `compute_layout` from the
  frontend (positions reflow).

## AI semantic search | AI 语义搜索

- Chinese-CLIP via ONNX Runtime (`ort`, DirectML). Image + text encoders loaded lazily
  (`AiEnginePool`); tokenizer (bert-base-chinese vocab) cached in the pool.
- Embeddings stored in SQLite as little-endian f32 BLOBs (512-d), and kept **resident** in
  an f16 contiguous cache (`AppState.ai_embedding_cache`) for search. Cosine similarity
  runs with rayon; results persisted to `ai_search_results`, which the layout query JOINs
  when `aiSearch` is on. The cache is invalidated on every embedding batch write / reset.
- See the extensive "踩坑记录" at the top of `ai/engine.rs` and `ai/clip.rs` (ORT version
  pitfalls, FP16 external-data format, vocab.txt mismatch).

## Security notes | 安全笔记

- `assetProtocol.scope` (tauri.conf.json) grants the asset protocol read access. Source
  images live under arbitrary scan-root paths, so scan roots + the thumbnail cache dir are
  granted at **runtime** (`app.asset_protocol_scope().allow_directory`) on startup and on
  `add_scan_root`; the static config keeps only the common user-media globs (`$PICTURE`,
  `$APPDATA`, …). Blanket full-drive globs (`C:/**` … `G:/**`) were removed — see
  `perf_hardening_plan_v2.md` §E1.
- CSP keeps `script-src 'unsafe-eval'` because **vue-i18n compiles messages at runtime via
  `new Function`**. Removing it requires precompiling locale messages
  (`@intlify/unplugin-vue-i18n`); left as-is for now.

## Known issues | 已知问题

- **Coordinate translation (B1)**: translated mode (logical height > `SAFE_MAX`, i.e.
  >~250k items) has a scroll-misalignment bug (scrollbar jumps, content offset wrong,
  jumps to bottom). Dormant at the default `SAFE_MAX = 10_000_000`. Shelved — see
  `perf_hardening_plan_v2.md`.
