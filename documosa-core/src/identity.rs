use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoleMode {
    Reviewer,
    Writer,
}

impl RoleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RoleMode::Reviewer => "reviewer",
            RoleMode::Writer => "writer",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ProtocolError> {
        match value {
            "reviewer" => Ok(RoleMode::Reviewer),
            "writer" => Ok(RoleMode::Writer),
            _ => Err(ProtocolError::BadRequest(
                "role mode must be reviewer or writer".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent {
        agent_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_ref: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_ref: Option<String>,
    },
}

impl Default for ActorKind {
    fn default() -> Self {
        Self::Human
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub client_id: String,
    pub nickname: String,
    pub role_mode: RoleMode,
    #[serde(default)]
    pub actor_kind: ActorKind,
}
