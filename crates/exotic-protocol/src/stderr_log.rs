// crates/exotic-protocol/src/stderr_log.rs
//! Worker stderr 行日志协议(日志能力重构线 阶段 3 · W3,D-313/D-314)。
//!
//! **不属帧协议**:stdout 才是唯一的帧通道(magic/版本/长度上限校验),stderr 一直是自由文本
//! 诊断区。本模块只是把这片自由文本的形状定成单行 JSON,供 supervisor 侧统一解析转发进主
//! tracing/JSONL 体系(§3.4/D-310)。因此**不 bump [`crate::PROTOCOL_VERSION`]**——非 JSON 行
//! supervisor 侧有 WARN + `unparsed=true` 兜底,不构成协议破坏。

use serde::{Deserialize, Serialize};

/// 单行 JSON 写 stderr 的 worker 日志行(host 侧 supervisor 按行扫描 + 反序列化)。
///
/// `lvl` 取值约定为 tracing 五档小写字符串之一(`"trace"|"debug"|"info"|"warn"|"error"`)；
/// supervisor 转发时按此值多臂 match 选级别,未知值按 warn 转发并原样保留(schema 漂移可见)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerLogLine {
    pub lvl: String,
    pub msg: String,
    /// 附加结构化上下文;省略即空对象(旧 worker/最小调用点不必显式传空 map)。
    #[serde(default)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl WorkerLogLine {
    /// 序列化为单行 JSON 并写 stderr。序列化失败(理论上不会发生——字段均为已校验的 JSON 安全类型)
    /// 时静默降级为原样 `eprintln!(msg)`:日志辅助本身绝不允许 panic 或吞掉消息(方案 §9.1 精神)。
    pub fn emit(&self) {
        match serde_json::to_string(self) {
            Ok(line) => eprintln!("{line}"),
            Err(_) => eprintln!("{}", self.msg),
        }
    }
}

/// 便捷封装:免各调用点手写 `WorkerLogLine { .. }.emit()`,两 worker 共用同一份序列化代码
/// (D-313 施工指引:避免各写一份)。
pub fn emit_stderr_log(
    lvl: &str,
    msg: impl Into<String>,
    fields: serde_json::Map<String, serde_json::Value>,
) {
    WorkerLogLine {
        lvl: lvl.to_string(),
        msg: msg.into(),
        fields,
    }
    .emit();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut fields = serde_json::Map::new();
        fields.insert("req_id".to_string(), serde_json::Value::from(42));
        let line = WorkerLogLine {
            lvl: "info".to_string(),
            msg: "会话就绪".to_string(),
            fields,
        };
        let json = serde_json::to_string(&line).expect("serialize");
        let back: WorkerLogLine = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.lvl, "info");
        assert_eq!(back.msg, "会话就绪");
        assert_eq!(back.fields.get("req_id").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn fields_defaults_to_empty_map_when_absent() {
        let json = r#"{"lvl":"warn","msg":"无字段行"}"#;
        let line: WorkerLogLine = serde_json::from_str(json).expect("deserialize");
        assert_eq!(line.lvl, "warn");
        assert_eq!(line.msg, "无字段行");
        assert!(line.fields.is_empty());
    }
}
