use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::TelegramAccountId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramAuditStatus {
    Success,
    Failed,
}

impl TelegramAuditStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "success" => Self::Success,
            _ => Self::Failed,
        }
    }
}

/// Automation / action audit record. Never stores secrets or auth credentials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramAuditEvent {
    pub id: Uuid,
    pub account_id: TelegramAccountId,
    pub scenario_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub event_type: String,
    pub action: String,
    pub target: Option<String>,
    pub status: TelegramAuditStatus,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramScenarioExecution {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub account_id: TelegramAccountId,
    pub event_key: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}
