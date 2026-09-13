# OCR golden fixtures(待落位)

`tests/ocr_golden.rs` 的 `ocr_golden`(`#[ignore]`)用两张检入 PNG 做通道序/归一化定案对拍。
**这两张图无法程序生成**(image crate 无字体渲染能力),须由人工准备后放入本目录,归**后续手测批**。
缺失时 golden 测试以 `eprintln` 提前 return,不 panic、不误伤 CI。

## 需要的两张图(各 ≤ 100KB)

| 文件名 | 内容要求 | 断言子串(`GOLDEN` 表) |
|---|---|---|
| `mixed_zh_en.png` | 中英混排**印刷体**一张:清晰黑字白底,含中文词与英文/数字。 | `"Scrollery"`、`"文字"` |
| `inverted_line.png` | 含**至少一行 180° 倒置**的印刷体行(测 cls 方向摆正)。 | `"倒置"` |

## 约定

- 子串来自 `ocr_golden.rs` 的 `GOLDEN` 常量;**落位后按图内实际文本同步微调该表与本表**(单源:`GOLDEN`)。
- 图内文字应为通用中英,避免生僻字(dict 覆盖外会静默丢字)。
- 印刷体、正对、无明显透视/模糊 —— golden 是「管线通道序是否正确」的判据,非鲁棒性压测。
- 若 golden 断言不过:先翻转 `ocr_profile.rs` 的 `swap_rb` 重跑,以过者为准回写默认值并记录定案依据
  (见 `construction-plan.md` 边界1)。
- 若 golden **全丢行 / conf 普遍异常**:先查「导出图末层含 softmax」假设(rec.rs/cls.rs 注释)——
  若模型输出的是未归一 logits,`rec_min_conf` / `cls_thresh` 会系统性失准,须在解码前补 softmax。
- bench 须记录各档 `OcrSessionInit` 冷载耗时,回填 coordinator `OCR_SESSION_INIT` 预算依据
  (现值 180s flat 为无实测的保守估计;慢机 server 档若撞冷载墙须按档位分预算)。
