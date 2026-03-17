//! 计费事件
//!
//! 处理 meteringEvent 类型的事件

use serde::{Deserialize, Serialize};

use crate::kiro::parser::error::ParseResult;
use crate::kiro::parser::frame::Frame;

use super::base::EventPayload;

/// 计费事件
///
/// `usage` 表示本次请求消耗的 credit 数量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeteringEvent {
    /// 计费单位（通常为 "credit"）
    #[serde(default)]
    pub unit: Option<String>,
    /// 计费单位复数形式（通常为 "credits"）
    #[serde(default)]
    pub unit_plural: Option<String>,
    /// 本次事件上报的 usage（credits）
    #[serde(default)]
    pub usage: f64,
    /// 捕获未使用字段，增强兼容性
    #[serde(flatten)]
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    extra: serde_json::Value,
}

impl EventPayload for MeteringEvent {
    fn from_frame(frame: &Frame) -> ParseResult<Self> {
        frame.payload_as_json()
    }
}

impl std::fmt::Display for MeteringEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(unit) = &self.unit_plural {
            write!(f, "{:.4} {}", self.usage, unit)
        } else if let Some(unit) = &self.unit {
            write!(f, "{:.4} {}", self.usage, unit)
        } else {
            write!(f, "{:.4}", self.usage)
        }
    }
}
