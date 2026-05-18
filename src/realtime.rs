use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::models::{Identity, RoleMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    pub document_id: String,
    pub client_id: String,
    pub nickname: String,
    pub role_mode: RoleMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Presence {
        document_id: String,
        users: Vec<Presence>,
    },
    LinesInserted {
        document_id: String,
        line_ids: Vec<String>,
        after_line_id: Option<String>,
    },
    LinesReplaced {
        document_id: String,
        line_ids: Vec<String>,
    },
    LinesDeleted {
        document_id: String,
        line_ids: Vec<String>,
    },
    CommentCreated {
        document_id: String,
        comment_id: String,
    },
    CommentResolved {
        document_id: String,
        comment_id: String,
    },
    SuggestionCreated {
        document_id: String,
        suggestion_id: String,
    },
    SuggestionDecided {
        document_id: String,
        suggestion_id: String,
        accepted: bool,
    },
    DocumentTitleUpdated {
        document_id: String,
        title: String,
    },
    LocksChanged {
        document_id: String,
    },
    ContentChanged {
        document_id: String,
    },
}

#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<ServerEvent>,
    presence: Arc<Mutex<HashMap<String, HashMap<String, Presence>>>>,
}

impl EventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            tx,
            presence: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }

    pub fn lines_inserted(&self, document_id: &str, line_ids: &[String], after_line_id: Option<&str>) {
        let _ = self.tx.send(ServerEvent::LinesInserted {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
            after_line_id: after_line_id.map(str::to_string),
        });
    }

    pub fn lines_replaced(&self, document_id: &str, line_ids: &[String]) {
        let _ = self.tx.send(ServerEvent::LinesReplaced {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
        });
    }

    pub fn lines_deleted(&self, document_id: &str, line_ids: &[String]) {
        let _ = self.tx.send(ServerEvent::LinesDeleted {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
        });
    }

    pub fn comment_created(&self, document_id: &str, comment_id: &str) {
        let _ = self.tx.send(ServerEvent::CommentCreated {
            document_id: document_id.to_string(),
            comment_id: comment_id.to_string(),
        });
    }

    pub fn comment_resolved(&self, document_id: &str, comment_id: &str) {
        let _ = self.tx.send(ServerEvent::CommentResolved {
            document_id: document_id.to_string(),
            comment_id: comment_id.to_string(),
        });
    }

    pub fn suggestion_created(&self, document_id: &str, suggestion_id: &str) {
        let _ = self.tx.send(ServerEvent::SuggestionCreated {
            document_id: document_id.to_string(),
            suggestion_id: suggestion_id.to_string(),
        });
    }

    pub fn suggestion_decided(&self, document_id: &str, suggestion_id: &str, accepted: bool) {
        let _ = self.tx.send(ServerEvent::SuggestionDecided {
            document_id: document_id.to_string(),
            suggestion_id: suggestion_id.to_string(),
            accepted,
        });
    }

    pub fn title_updated(&self, document_id: &str, title: &str) {
        let _ = self.tx.send(ServerEvent::DocumentTitleUpdated {
            document_id: document_id.to_string(),
            title: title.to_string(),
        });
    }

    pub fn locks_changed(&self, document_id: &str) {
        let _ = self.tx.send(ServerEvent::LocksChanged {
            document_id: document_id.to_string(),
        });
    }

    pub fn content_changed(&self, document_id: &str) {
        let _ = self.tx.send(ServerEvent::ContentChanged {
            document_id: document_id.to_string(),
        });
    }

    pub fn join(&self, document_id: &str, identity: Identity) {
        let users = {
            let mut guard = self.presence.lock().expect("presence mutex poisoned");
            let doc = guard.entry(document_id.to_string()).or_default();
            doc.insert(
                identity.client_id.clone(),
                Presence {
                    document_id: document_id.to_string(),
                    client_id: identity.client_id,
                    nickname: identity.nickname,
                    role_mode: identity.role_mode,
                },
            );
            doc.values().cloned().collect::<Vec<_>>()
        };
        let _ = self.tx.send(ServerEvent::Presence {
            document_id: document_id.to_string(),
            users,
        });
    }

    pub fn leave(&self, document_id: &str, client_id: &str) {
        let users = {
            let mut guard = self.presence.lock().expect("presence mutex poisoned");
            if let Some(doc) = guard.get_mut(document_id) {
                doc.remove(client_id);
                doc.values().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        let _ = self.tx.send(ServerEvent::Presence {
            document_id: document_id.to_string(),
            users,
        });
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn websocket(socket: WebSocket, hub: EventHub, document_id: String, identity: Identity) {
    hub.join(&document_id, identity.clone());
    let mut rx = hub.subscribe();
    let (mut sender, mut receiver) = socket.split();
    let writer_doc = document_id.clone();
    let write_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let event_doc = match &event {
                ServerEvent::Presence { document_id, .. }
                | ServerEvent::LinesInserted { document_id, .. }
                | ServerEvent::LinesReplaced { document_id, .. }
                | ServerEvent::LinesDeleted { document_id, .. }
                | ServerEvent::CommentCreated { document_id, .. }
                | ServerEvent::CommentResolved { document_id, .. }
                | ServerEvent::SuggestionCreated { document_id, .. }
                | ServerEvent::SuggestionDecided { document_id, .. }
                | ServerEvent::DocumentTitleUpdated { document_id, .. }
                | ServerEvent::LocksChanged { document_id }
                | ServerEvent::ContentChanged { document_id } => document_id,
            };
            if event_doc == &writer_doc
                && let Ok(text) = serde_json::to_string(&event)
                && sender.send(Message::Text(text.into())).await.is_err()
            {
                break;
            }
        }
    });
    while let Some(Ok(message)) = receiver.next().await {
        if matches!(message, Message::Close(_)) {
            break;
        }
    }
    write_task.abort();
    hub.leave(&document_id, &identity.client_id);
}
