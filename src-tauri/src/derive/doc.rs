// src-tauri/src/derive/doc.rs
//! 文档缩略图派生（§3.4）。
//!
//! P4 落点（Lite 路径）——文档缩略图按子类型分两条路：
//!  - **epub**：后端用 `zip` crate 解析容器，按 OPF 取封面图字节 → 复用图像编码器写入缩略图缓存。
//!    纯后端、可走派生流水线，故由本文件 `run_thumb` 处理。
//!  - **pdf / svg**：Lite 无 native 栅格化器 → 由前端 `DocThumbRenderer.vue` 离屏渲染截图，
//!    经 `store_doc_thumbnail` IPC 回传字节落盘。这两类在 `get_pending_derivations` 中被排除，
//!    永不进入后端消费者（见该查询注释），故 `run_thumb` 只会收到 epub。
//!  - **txt / md / office**：不在 `DOC_THUMB_FORMATS`，无派生行；前端用 CSS「文本卡」呈现。
//!
//! Perf 路径走 pdfium 并行栅格化，产物/缓存路径一致，前端 `MediaThumb` 无感。

use std::io::Read;
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::derive::kind::{DerivationContext, DerivationOutput};
use crate::error::{AppError, Result};
use crate::thumbnail::generator::{encode_media_step_with_snapshot, snap_to_tier, ThumbConfig};

/// 为文档生成首页/封面缩略图（P4）。
///
/// 仅处理 epub（后端 zip 取封面）；pdf/svg 由前端离屏渲染并经 `store_doc_thumbnail` 回传，
/// 不应到达此处（已在 `get_pending_derivations` 排除）。
pub fn run_thumb(ctx: &DerivationContext) -> Result<DerivationOutput> {
    if ctx.file_format != "epub" {
        return Err(AppError::Internal(format!(
            "doc_thumb backend handles epub only; '{}' is frontend-driven | 后端仅处理 epub 文档缩略图，其余前端渲染",
            ctx.file_format
        )));
    }

    // 1) 取 epub 封面原始图字节 + spine 页数（document_meta 回填，§3.8.2 / T10）。
    let (img_bytes, page_count) = extract_epub_cover_and_pages(&ctx.abs_path)?;

    // 2) 解码为 RGBA → 复用缩略图编码器（缩放 → WebP → 写入缓存 by cache_key → thumbhash），
    //    与视频封面（derive/video.rs::run_cover）完全同构，使 MediaThumb 零改动显示。
    let dynimg = image::load_from_memory(&img_bytes).map_err(|e| {
        AppError::Internal(format!("epub cover decode failed | epub 封面解码失败: {e}"))
    })?;
    let rgba = dynimg.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let decoded = crate::engine::traits::DecodedImage {
        pixels: rgba.into_raw(),
        width: w,
        height: h,
        icc: None, // epub 封面无 ICC 来源,按 sRGB 假定
    };

    let cfg = ThumbConfig {
        cache_dir: ctx.cache_dir.clone(),
        size: snap_to_tier(ctx.thumb_size),
        skip_max_bytes: 0,
        strategy: String::new(),
        gpu_engine: String::new(),
        ai_hq_cache: false, // 文档封面非 CLIP 分析对象，不产 AI 缓存
        webp_quality: ctx.webp_quality,
        ai_cache_short_edge: crate::thumbnail::cache::AI_CACHE_SHORT_EDGE, // 未用(ai_hq_cache=false)
    };
    let res = encode_media_step_with_snapshot(
        ctx.item_id,
        ctx.source_revision,
        ctx.cache_key,
        decoded,
        &cfg,
    )?;

    Ok(DerivationOutput {
        payload_path: res.thumb_path,
        thumbhash: res.thumbhash,
        page_count,
    })
}

/// Extract the cover image bytes AND the spine page count from an EPUB (a zip container):
///   container.xml → OPF rootfile → cover href → read entry bytes; spine `<itemref>` count → pages.
/// 从 EPUB（zip 容器）中抽取封面图字节 + spine 页数：container.xml → OPF → 封面 href → 读取条目字节；
/// 同一份 OPF 顺带数 spine 的 `<itemref>` 作为页数近似（§3.8.2 / T10），避免二次开包。
fn extract_epub_cover_and_pages(path: &Path) -> Result<(Vec<u8>, Option<i64>)> {
    let file = std::fs::File::open(path).map_err(AppError::from)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Internal(format!("open epub failed | 打开 epub 失败: {e}")))?;

    // 1) META-INF/container.xml → OPF 路径（相对 zip 根）。
    let opf_path = {
        let mut f = zip.by_name("META-INF/container.xml").map_err(|_| {
            AppError::Internal("epub missing container.xml | 缺少 container.xml".into())
        })?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(AppError::from)?;
        find_opf_path(&s)
            .ok_or_else(|| AppError::Internal("epub rootfile not found | 未找到 OPF".into()))?
    };

    // 2) 解析 OPF → 封面 href（相对 OPF 所在目录）。
    let opf_xml = {
        let mut f = zip.by_name(&opf_path).map_err(|_| {
            AppError::Internal(format!("epub OPF not found: {opf_path} | OPF 缺失"))
        })?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(AppError::from)?;
        s
    };
    let cover_href = find_cover_href(&opf_xml)
        .ok_or_else(|| AppError::Internal("epub cover not found | 未找到 epub 封面".into()))?;
    // 顺带数 spine 页数（同一份 OPF，避免二次开包）。
    let page_count = count_epub_spine(&opf_xml);

    // 3) href 相对 OPF 目录解析 + 读取封面字节。
    let opf_dir = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let cover_path = normalize_zip_path(opf_dir, &percent_decode(&cover_href));
    let mut f = zip.by_name(&cover_path).map_err(|_| {
        AppError::Internal(format!(
            "epub cover entry not found: {cover_path} | 封面条目缺失"
        ))
    })?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(AppError::from)?;
    Ok((bytes, page_count))
}

/// epub 是 reflowable、无固定分页；以 OPF `<spine>` 中 `<itemref>` 数（阅读顺序章节数）作为
/// 页数近似（§3.8.2 / T10）。无 spine 或解析失败返回 None（`document_meta.page_count` 置空）。
fn count_epub_spine(opf: &str) -> Option<i64> {
    let mut reader = Reader::from_str(opf);
    let mut count = 0i64;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                // itemref 只出现在 spine 内，直接计数即可。
                if e.local_name().as_ref() == b"itemref" {
                    count += 1;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (count > 0).then_some(count)
}

/// 按（无前缀）键名读取起始/空标签的一个属性值（已反转义）。
fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Find the OPF rootfile `full-path` in `META-INF/container.xml`.
/// 在 `META-INF/container.xml` 中找到 OPF rootfile 的 `full-path`。
fn find_opf_path(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"rootfile" {
                    if let Some(p) = attr(&e, b"full-path") {
                        return Some(p);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

/// Resolve the cover image href from an OPF, by priority:
///   1. EPUB3: manifest `<item properties="cover-image">`
///   2. EPUB2: `<meta name="cover" content="ID">` → manifest item with that id
///   3. Heuristic: first image-typed manifest item whose id/href mentions "cover"
///
/// 按优先级从 OPF 解析封面图 href（EPUB3 properties → EPUB2 meta cover → 启发式）。
fn find_cover_href(opf: &str) -> Option<String> {
    let mut reader = Reader::from_str(opf);
    let mut cover_id: Option<String> = None; // <meta name="cover" content="..">
    let mut epub3_href: Option<String> = None; // <item properties="cover-image" ..>
    let mut manifest: Vec<(String, String, String)> = Vec::new(); // (id, href, media_type)

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"meta" => {
                    if attr(&e, b"name").as_deref() == Some("cover") {
                        cover_id = attr(&e, b"content");
                    }
                }
                b"item" => {
                    let id = attr(&e, b"id").unwrap_or_default();
                    let href = attr(&e, b"href").unwrap_or_default();
                    let mt = attr(&e, b"media-type").unwrap_or_default();
                    if let Some(props) = attr(&e, b"properties") {
                        if props.split_whitespace().any(|p| p == "cover-image") {
                            epub3_href = Some(href.clone());
                        }
                    }
                    if !href.is_empty() {
                        manifest.push((id, href, mt));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if let Some(h) = epub3_href {
        return Some(h);
    }
    if let Some(cid) = cover_id {
        if let Some((_, href, _)) = manifest.iter().find(|(id, _, _)| *id == cid) {
            return Some(href.clone());
        }
    }
    manifest
        .iter()
        .find(|(id, href, mt)| {
            mt.starts_with("image/")
                && (id.to_lowercase().contains("cover") || href.to_lowercase().contains("cover"))
        })
        .map(|(_, href, _)| href.clone())
}

/// Join an OPF-relative href onto its directory and normalise `.`/`..`/empty segments
/// into a clean zip entry path (zip always uses `/`).
/// 把 OPF 相对 href 拼到其目录并规整 `.`/`..`/空段为干净的 zip 条目路径（zip 恒用 `/`）。
fn normalize_zip_path(dir: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or(href); // 去掉锚点/查询
    let combined = if dir.is_empty() {
        href.to_string()
    } else {
        format!("{dir}/{href}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// href 的最小化百分号解码（如 `%20` → 空格）；epub href 可能被 URL 编码。
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::count_epub_spine;

    /// spine 的 itemref 计数 = 页数近似（含自闭合 `<itemref/>` 与带属性形式）。
    #[test]
    fn counts_spine_itemrefs() {
        let opf = r#"<package><spine toc="ncx">
            <itemref idref="c1"/>
            <itemref idref="c2"/>
            <itemref idref="c3" linear="yes"/>
        </spine></package>"#;
        assert_eq!(count_epub_spine(opf), Some(3));
    }

    /// 无 spine/itemref → None（document_meta.page_count 置空，而非 0）。
    #[test]
    fn no_spine_returns_none() {
        let opf = r#"<package><manifest><item id="x" href="x.xhtml"/></manifest></package>"#;
        assert_eq!(count_epub_spine(opf), None);
        assert_eq!(count_epub_spine("not xml at all"), None);
    }
}
