// LAN collaboration module (feature-gated behind `collab`).
//
// Provides a simple server-authoritative operational transform (OT)
// system for real-time collaborative editing over TCP on a local
// network.

#[cfg(feature = "collab")]
pub mod client;
#[cfg(feature = "collab")]
pub mod server;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

// ── Protocol messages ─────────────────────────────────────────────────────────

/// Message sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Join { username: String },
    Op { op: OpKind, pos: usize, text: String, rev: u64 },
    Cursor { pos: usize },
}

/// Message sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Sync { content: String, rev: u64, peers: Vec<String> },
    Op { op: OpKind, pos: usize, text: String, rev: u64, peer: String },
    PeerUpdate { peers: Vec<String>, event: PeerEvent, username: String },
    Cursor { peer: String, pos: usize },
}

/// The kind of text operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Insert,
    Delete,
}

/// Peer connection event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerEvent {
    Joined,
    Left,
}

// ── Handle types ──────────────────────────────────────────────────────────────

/// An event received from the collaboration layer, to be applied to the local buffer.
#[derive(Debug, Clone)]
pub enum CollabEvent {
    /// A remote edit operation.
    Edit { kind: OpKind, pos: usize, text: String, peer: String, rev: u64 },
    /// Full document sync (on connect / reconnect).
    FullSync { content: String, rev: u64 },
    /// Peer list updated.
    PeersChanged { peers: Vec<String> },
    /// A remote peer's cursor moved.
    PeerCursor { peer: String, pos: usize },
    /// Connection state changed.
    ConnectionStatus { connected: bool },
    /// Local op confirmed by server — update revision only, do NOT re-apply.
    LocalConfirm { rev: u64 },
}

/// Shared state visible to the editor (peer info, cursor positions).
#[derive(Default)]
pub struct CollabState {
    pub peers: Vec<String>,
    pub peer_cursors: HashMap<String, usize>,
    pub connected: bool,
}

/// Whether this editor instance is acting as host or guest.
#[derive(Debug, Clone)]
pub enum CollabRole {
    Host { port: u16 },
    Guest { host: String, port: u16 },
}

/// Handle the editor uses to interact with the collaboration layer.
pub struct CollabHandle {
    pub role: CollabRole,
    /// Channel to send local ops (kind, char_offset, text) to the network layer.
    pub op_tx: std::sync::mpsc::SyncSender<(OpKind, usize, String)>,
    /// Channel to send cursor position updates.
    pub cursor_tx: std::sync::mpsc::SyncSender<usize>,
    /// Channel to receive collaboration events from the network layer.
    pub event_rx: std::sync::mpsc::Receiver<CollabEvent>,
    /// Shared peer state (updated by the background thread).
    pub state: Arc<Mutex<CollabState>>,
    /// Local copy of the current confirmed server revision.
    pub revision: u64,
}

impl CollabHandle {
    /// Try to receive a pending collab event (non-blocking).
    pub fn try_recv(&mut self) -> Option<CollabEvent> {
        match self.event_rx.try_recv() {
            Ok(ev) => {
                match &ev {
                    CollabEvent::Edit { rev, .. }
                    | CollabEvent::FullSync { rev, .. }
                    | CollabEvent::LocalConfirm { rev } => {
                        self.revision = *rev;
                    }
                    _ => {}
                }
                Some(ev)
            }
            Err(_) => None,
        }
    }

    /// Send a local insert operation.
    pub fn send_insert(&self, pos: usize, text: String) {
        let _ = self.op_tx.try_send((OpKind::Insert, pos, text));
    }

    /// Send a local delete operation.
    pub fn send_delete(&self, pos: usize, text: String) {
        let _ = self.op_tx.try_send((OpKind::Delete, pos, text));
    }

    /// Send the current cursor position.
    pub fn send_cursor(&self, pos: usize) {
        let _ = self.cursor_tx.try_send(pos);
    }

    /// Number of connected peers (excluding self).
    pub fn peer_count(&self) -> usize {
        self.state.lock().map(|s| s.peers.len()).unwrap_or(0)
    }

    /// Whether the network connection is currently active.
    pub fn is_connected(&self) -> bool {
        self.state.lock().map(|s| s.connected).unwrap_or(false)
    }

    /// A snapshot of all peer cursor positions (peer_username → char_offset).
    pub fn peer_cursors(&self) -> HashMap<String, usize> {
        self.state.lock().map(|s| s.peer_cursors.clone()).unwrap_or_default()
    }

    /// A short string describing the collab status for the status bar.
    pub fn status_str(&self) -> String {
        let connected = self.is_connected();
        let peers = self.peer_count();
        let role_str = match &self.role {
            CollabRole::Host { port } => format!("[Host: {}]", port),
            CollabRole::Guest { host, port } => format!("[Guest: {}:{}]", host, port),
        };
        if !connected {
            format!("{} [disconnected]", role_str)
        } else if peers > 0 {
            format!("{} [{} peer{}]", role_str, peers, if peers == 1 { "" } else { "s" })
        } else {
            role_str
        }
    }
}

// ── OT transform ──────────────────────────────────────────────────────────────

/// Transform a client operation's position against a server operation that
/// happened concurrently.  Returns the new position for the client op.
///
/// The server wins on position ties for insert-vs-insert.
pub fn transform_pos(
    client_kind: &OpKind,
    client_pos: usize,
    client_text: &str,
    server_kind: &OpKind,
    server_pos: usize,
    server_text: &str,
) -> usize {
    let server_len = server_text.chars().count();
    let client_len = client_text.chars().count();

    match (client_kind, server_kind) {
        // Insert vs Insert: server wins at same position → client shifts right.
        (OpKind::Insert, OpKind::Insert) => {
            if client_pos >= server_pos {
                client_pos + server_len
            } else {
                client_pos
            }
        }
        // Client inserts, server deleted: adjust for removed chars.
        (OpKind::Insert, OpKind::Delete) => {
            if client_pos <= server_pos {
                client_pos
            } else if client_pos <= server_pos + server_len {
                // Insertion point was inside the deleted range.
                server_pos
            } else {
                client_pos - server_len
            }
        }
        // Client deletes, server inserted: adjust for added chars.
        (OpKind::Delete, OpKind::Insert) => {
            if client_pos >= server_pos {
                client_pos + server_len
            } else {
                client_pos
            }
        }
        // Delete vs Delete: handle overlapping ranges.
        (OpKind::Delete, OpKind::Delete) => {
            if client_pos >= server_pos + server_len {
                // Client delete is entirely after server delete.
                client_pos - server_len
            } else if client_pos >= server_pos {
                // Client delete starts inside server's deleted range.
                server_pos
            } else if client_pos + client_len > server_pos {
                // Partial overlap: client delete extends into server's range.
                client_pos
            } else {
                client_pos
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── OT transform tests ────────────────────────────────────────────────────

    #[test]
    fn insert_vs_insert_client_after_server() {
        // Server inserted 3 chars at pos 2; client wants to insert at pos 5.
        // Client pos should shift right by 3.
        let new_pos = transform_pos(
            &OpKind::Insert, 5, "x",
            &OpKind::Insert, 2, "abc",
        );
        assert_eq!(new_pos, 8);
    }

    #[test]
    fn insert_vs_insert_client_before_server() {
        // Server inserted at pos 5; client wants to insert at pos 2.
        // Client pos unchanged.
        let new_pos = transform_pos(
            &OpKind::Insert, 2, "x",
            &OpKind::Insert, 5, "abc",
        );
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn insert_vs_insert_same_position_server_wins() {
        // Both insert at pos 3; server wins → client shifts right.
        let new_pos = transform_pos(
            &OpKind::Insert, 3, "x",
            &OpKind::Insert, 3, "abc",
        );
        assert_eq!(new_pos, 6);
    }

    #[test]
    fn insert_vs_delete_client_before_deleted_range() {
        let new_pos = transform_pos(
            &OpKind::Insert, 1, "x",
            &OpKind::Delete, 5, "abc",
        );
        assert_eq!(new_pos, 1);
    }

    #[test]
    fn insert_vs_delete_client_inside_deleted_range() {
        // Server deleted chars 3-5; client wants to insert at 4 → collapses to 3.
        let new_pos = transform_pos(
            &OpKind::Insert, 4, "x",
            &OpKind::Delete, 3, "ab",
        );
        assert_eq!(new_pos, 3);
    }

    #[test]
    fn insert_vs_delete_client_after_deleted_range() {
        let new_pos = transform_pos(
            &OpKind::Insert, 8, "x",
            &OpKind::Delete, 3, "abc",
        );
        assert_eq!(new_pos, 5);
    }

    #[test]
    fn delete_vs_insert_client_after_insertion() {
        // Server inserted 2 chars at 3; client deletes at 5 → shift right by 2.
        let new_pos = transform_pos(
            &OpKind::Delete, 5, "x",
            &OpKind::Insert, 3, "ab",
        );
        assert_eq!(new_pos, 7);
    }

    #[test]
    fn delete_vs_insert_client_before_insertion() {
        let new_pos = transform_pos(
            &OpKind::Delete, 2, "x",
            &OpKind::Insert, 5, "ab",
        );
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn delete_vs_delete_non_overlapping_client_after() {
        // Server deleted 3 chars at pos 0; client deletes at pos 5 → shift left by 3.
        let new_pos = transform_pos(
            &OpKind::Delete, 5, "x",
            &OpKind::Delete, 0, "abc",
        );
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn delete_vs_delete_non_overlapping_client_before() {
        let new_pos = transform_pos(
            &OpKind::Delete, 1, "x",
            &OpKind::Delete, 5, "abc",
        );
        assert_eq!(new_pos, 1);
    }

    #[test]
    fn delete_vs_delete_client_inside_server_range() {
        // Server deleted chars 2-4; client deletes at 3 → collapses to 2.
        let new_pos = transform_pos(
            &OpKind::Delete, 3, "x",
            &OpKind::Delete, 2, "abc",
        );
        assert_eq!(new_pos, 2);
    }
}

// ── Network I/O helpers ───────────────────────────────────────────────────────

/// Write a length-prefixed JSON message to a tokio `AsyncWrite`.
pub async fn write_msg<W, T>(writer: &mut W, msg: &T) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    use tokio::io::AsyncWriteExt;
    let json = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let len = json.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&json).await?;
    Ok(())
}

/// Read a length-prefixed JSON message from a tokio `AsyncRead`.
pub async fn read_msg<R, T>(reader: &mut R) -> std::io::Result<T>
where
    R: tokio::io::AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    use tokio::io::AsyncReadExt;
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}
