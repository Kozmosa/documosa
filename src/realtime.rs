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
    DocumentChanged {
        document_id: String,
        topic: String,
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

    pub fn document_changed(&self, document_id: &str, topic: &str) {
        let _ = self.tx.send(ServerEvent::DocumentChanged {
            document_id: document_id.to_string(),
            topic: topic.to_string(),
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
                ServerEvent::Presence { document_id, .. } => document_id,
                ServerEvent::DocumentChanged { document_id, .. } => document_id,
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
