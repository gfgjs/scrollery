// src-tauri/src/ai/worker_client/dispatch.rs
//! C 组:批请求重试分发(MAX_ATTEMPTS 硬止损 attempt 圈)。

use super::*;

impl AiWorkerClient {
    /// 跑一个批请求并按 op 校验输出。进程级异常/输出违例 → 重建重发一次(硬止损)。
    fn run_validated<T>(
        &mut self,
        spec: &SessionSpec,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
        validate: impl Fn(
            &SessionDescriptor,
            &exotic_protocol::SuccessBody,
            &[u8],
        ) -> std::result::Result<T, String>,
    ) -> Result<T> {
        // 记录最后一次失败原因,硬止损时带出(否则 TimedOut 等只剩笼统文案)。
        let mut last_err: Option<String> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            // 会话就绪失败按二分处理(2026-07-10 审查 W1):进程级弃实例后计入本次
            // attempt 重试(旧实现 `?` 直接冒泡,MAX_ATTEMPTS 对这条路径形同虚设);
            // 数据级直接冒泡。
            let desc = match self.ensure_session(spec, cancelled) {
                Ok(d) => d,
                Err(EnsureError::Terminal(e)) => return Err(e),
                Err(EnsureError::Process(e)) => {
                    warn!("AI worker 会话就绪失败(第 {attempt} 次,进程级):{e} → 弃用实例重试");
                    last_err = Some(e.to_string());
                    self.drop_worker();
                    if cancelled() {
                        return Err(AppError::System("AI worker 批请求已取消".into()));
                    }
                    continue;
                }
            };
            let w = self.worker.as_mut().expect("ensure_session 后必有实例");
            match w.run_batch(req, timeout, cancelled) {
                RawOutcome::Success { body, blob } => match validate(&desc, &body, &blob) {
                    Ok(out) => return Ok(out),
                    Err(msg) => {
                        // 输出校验失败 = 协议违例:弃用实例,重建后重发一次。
                        warn!("AI worker 输出校验失败(第 {attempt} 次):{msg} → 弃用实例");
                        last_err = Some(msg);
                        self.drop_worker();
                    }
                },
                RawOutcome::Failure(fb)
                    if fb.code == exotic_protocol::WorkerErrorCode::SessionExpired =>
                {
                    // worker 端会话丢失:close 清 host 快照(幂等)后由下轮 ensure 重建。
                    warn!("AI worker 报会话失效(第 {attempt} 次)→ 重建会话");
                    last_err = Some(format!("[{}]{}", fb.code.as_str(), fb.message));
                    if let Some(w) = self.worker.as_mut() {
                        let _ = w.close_session(op_timeouts::SESSION_CLOSE, cancelled);
                    }
                }
                RawOutcome::Failure(fb) if fb.retryable => {
                    // 尊重协议 retryable 位(2026-07-10 审查 W2):InternalError/
                    // GpuUnavailable/IoError 等瞬态失败(DirectML 偶发内核错误、显存
                    // 瞬时紧张)重发一次,而非把一切 Failure 按 terminal 放大成整轮
                    // 终止+清续传标志。实例与会话仍存活:不弃实例,退避后直接重发。
                    warn!(
                        "AI worker 批请求失败[{}](第 {attempt} 次,retryable):{} → 退避重发",
                        fb.code.as_str(),
                        fb.message
                    );
                    last_err = Some(format!("[{}]{}", fb.code.as_str(), fb.message));
                    if attempt < MAX_ATTEMPTS {
                        std::thread::sleep(RETRY_BACKOFF);
                    }
                }
                RawOutcome::Failure(fb) => {
                    return Err(AppError::System(format!(
                        "AI worker 批请求失败[{}]:{}",
                        fb.code.as_str(),
                        fb.message
                    )))
                }
                RawOutcome::TimedOut => {
                    warn!("AI worker 批请求超时(第 {attempt} 次,实例已回收)");
                    last_err = Some("批请求超时".into());
                }
                RawOutcome::Disconnected => {
                    warn!("AI worker 断开(第 {attempt} 次)");
                    last_err = Some("worker 断开".into());
                }
                RawOutcome::Protocol(msg) => {
                    warn!("AI worker 协议违例(第 {attempt} 次):{msg}");
                    last_err = Some(msg);
                }
            }
            if cancelled() {
                return Err(AppError::System("AI worker 批请求已取消".into()));
            }
        }
        Err(AppError::System(format!(
            "AI worker 批请求 {MAX_ATTEMPTS} 次尝试均失败(硬止损){}",
            last_err
                .map(|e| format!(";最后错误:{e}"))
                .unwrap_or_default()
        )))
    }

    /// CLIP 图像批嵌入:结果与 `items` 同序对齐(校验见 `validate_embed_batch_output`)。
    pub fn embed_batch(
        &mut self,
        spec: &SessionSpec,
        items: &[EmbedItem],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<EmbedItemOutcome>> {
        let req = RequestBody::EmbedBatch {
            items: items.to_vec(),
        };
        self.run_validated(
            spec,
            &req,
            op_timeouts::EMBED_BATCH,
            cancelled,
            |d, b, bl| validate_embed_batch_output(items, b, bl, d.embed_dim as usize),
        )
    }

    /// CLIP 文本编码(语义搜索查询向量;T17 补的 EncodeText op)。
    pub fn encode_text(
        &mut self,
        spec: &SessionSpec,
        texts: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Vec<f32>>> {
        let req = RequestBody::EncodeText {
            texts: texts.to_vec(),
        };
        self.run_validated(
            spec,
            &req,
            op_timeouts::ENCODE_TEXT,
            cancelled,
            |d, b, bl| validate_encode_text_output(texts.len(), b, bl, d.embed_dim as usize),
        )
    }

    /// 人脸检测+嵌入批(face 接线波):结果与 `items` 同序对齐。要求
    /// `spec.face_profile = Some`(ensure_session 据此载入合并会话);校验维度取
    /// SessionReady 回报的 `face_embed_dim`,缺失即协议违例(触发硬止损重建)。
    pub fn face_detect_embed(
        &mut self,
        spec: &SessionSpec,
        items: &[FaceItem],
        det_score_thresh: f32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<FaceItemOutcome>> {
        let req = RequestBody::FaceDetectEmbed {
            items: items.to_vec(),
            det_score_thresh,
        };
        self.run_validated(
            spec,
            &req,
            // 超时按批内项数缩放(2026-07-03 修复:固定 120s 对全尺寸原图批必然误杀)。
            op_timeouts::face_detect_embed(items.len()),
            cancelled,
            |d, b, bl| {
                let dim = d
                    .face_embed_dim
                    .ok_or_else(|| "会话未声明 face_embed_dim(未载人脸角色)".to_string())?;
                validate_face_batch_output(items, b, bl, dim as usize)
            },
        )
    }

    /// OCR 批(交互恒单图;批口面向未来):结果与 `items` 同序对齐(校验见
    /// [`validate_ocr_batch_output`])。
    ///
    /// **不复用 [`run_validated`](Self::run_validated)**——它的 ensure 是 CLIP 会话;本圈
    /// 与之逐臂同构,只把 ensure 换成 [`ensure_ocr_session`](Self::ensure_ocr_session):
    ///   - 输出校验违例 → 弃实例重建重发(协议违例);
    ///   - `SessionExpired` → 清 `ocr_loaded` 后由下轮 ensure 重 init(worker 自杀/换代场景);
    ///   - retryable Failure → 退避原实例重发;terminal Failure → 直返;
    ///   - 进程级三态(超时/断开/协议)→ worker 已死,下轮 ensure_worker 换代重 init;
    ///   - `MAX_ATTEMPTS` 硬止损。
    pub fn ocr_batch(
        &mut self,
        spec: &OcrSessionSpec,
        items: &[OcrItem],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<OcrItemOutcome>> {
        let req = RequestBody::OcrBatch {
            items: items.to_vec(),
        };
        let timeout = op_timeouts::ocr_batch(items.len());
        let mut last_err: Option<String> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            match self.ensure_ocr_session(spec, cancelled) {
                Ok(()) => {}
                Err(EnsureError::Terminal(e)) => return Err(e),
                Err(EnsureError::Process(e)) => {
                    warn!("OCR 会话就绪失败(第 {attempt} 次,进程级):{e} → 弃用实例重试");
                    last_err = Some(e.to_string());
                    self.drop_worker();
                    if cancelled() {
                        return Err(AppError::System("OCR 批请求已取消".into()));
                    }
                    continue;
                }
            }
            let w = self.worker.as_mut().expect("ensure_ocr_session 后必有实例");
            match w.run_batch(&req, timeout, cancelled) {
                RawOutcome::Success { body, blob } => {
                    match validate_ocr_batch_output(items, &body, &blob) {
                        Ok(out) => return Ok(out),
                        Err(msg) => {
                            // 输出校验失败 = 协议违例:弃用实例,重建后重发一次。
                            warn!("OCR 输出校验失败(第 {attempt} 次):{msg} → 弃用实例");
                            last_err = Some(msg);
                            self.drop_worker();
                        }
                    }
                }
                RawOutcome::Failure(fb)
                    if fb.code == exotic_protocol::WorkerErrorCode::SessionExpired =>
                {
                    // worker 端 OCR 会话丢失(空闲自杀/换代):清 host 快照,下轮 ensure 重 init。
                    warn!("OCR 报会话失效(第 {attempt} 次)→ 重建会话");
                    last_err = Some(format!("[{}]{}", fb.code.as_str(), fb.message));
                    self.ocr_loaded = None;
                }
                RawOutcome::Failure(fb) if fb.retryable => {
                    // 瞬态失败:实例与会话仍存活,不弃实例,退避后原会话重发。
                    warn!(
                        "OCR 批请求失败[{}](第 {attempt} 次,retryable):{} → 退避重发",
                        fb.code.as_str(),
                        fb.message
                    );
                    last_err = Some(format!("[{}]{}", fb.code.as_str(), fb.message));
                    if attempt < MAX_ATTEMPTS {
                        std::thread::sleep(RETRY_BACKOFF);
                    }
                }
                // 同上(见 ensure_ocr_session):批发送本身撞 ModelLoadFailed 的防御分支,
                // 理论上恒在 ensure_ocr_session 先行拦下,此处双保险同一映射。
                RawOutcome::Failure(fb)
                    if fb.code == exotic_protocol::WorkerErrorCode::ModelLoadFailed =>
                {
                    return Err(AppError::Ocr {
                        code: "ocr_model_missing",
                        message: "OCR 模型加载失败，请重新下载对应档位".into(),
                    })
                }
                RawOutcome::Failure(fb) => {
                    return Err(AppError::System(format!(
                        "OCR 批请求失败[{}]:{}",
                        fb.code.as_str(),
                        fb.message
                    )))
                }
                // 进程级三态:worker 已死,ocr_loaded 由下轮 ensure_worker 换代复位(同 run_validated)。
                RawOutcome::TimedOut => {
                    warn!("OCR 批请求超时(第 {attempt} 次,实例已回收)");
                    last_err = Some("OCR 批请求超时".into());
                }
                RawOutcome::Disconnected => {
                    warn!("OCR worker 断开(第 {attempt} 次)");
                    last_err = Some("worker 断开".into());
                }
                RawOutcome::Protocol(msg) => {
                    warn!("OCR 协议违例(第 {attempt} 次):{msg}");
                    last_err = Some(msg);
                }
            }
            if cancelled() {
                return Err(AppError::System("OCR 批请求已取消".into()));
            }
        }
        Err(AppError::System(format!(
            "OCR 批请求 {MAX_ATTEMPTS} 次尝试均失败(硬止损){}",
            last_err
                .map(|e| format!(";最后错误:{e}"))
                .unwrap_or_default()
        )))
    }

    /// 显式卸载会话(管线自然完成/停止时调用,对齐进程内路径「结束即卸引擎释放 VRAM」;
    /// worker 进程留存,空闲 300s 自杀兜底,D3 §4④)。幂等。
    pub fn close_session(&mut self) {
        if let Some(w) = self.worker.as_mut() {
            if w.is_alive() {
                let _ = w.close_session(op_timeouts::SESSION_CLOSE, &|| false);
            }
        }
    }
}
