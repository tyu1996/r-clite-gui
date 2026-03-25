// Collaboration host / TCP server.
//
// Maintains the canonical buffer and revision log, accepts client
// connections, transforms incoming operations, and broadcasts
// updates to all connected peers.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ropey::Rope;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc as tokio_mpsc;

use super::{
    ClientMsg, CollabEvent, CollabHandle, CollabRole, CollabState, OpKind, PeerEvent, ServerMsg,
    read_msg, transform_pos, write_msg,
};

struct LoggedOp {
    rev: u64,
    kind: OpKind,
    pos: usize,
    text: String,
}

struct Peer {
    username: String,
    tx: tokio_mpsc::UnboundedSender<ServerMsg>,
    cursor_pos: usize,
}

struct ServerState {
    rope: Rope,
    revision: u64,
    op_log: VecDeque<LoggedOp>,
    peers: HashMap<u64, Peer>,
    next_peer_id: u64,
}

impl ServerState {
    fn new(content: &str) -> Self {
        Self {
            rope: Rope::from_str(content),
            revision: 0,
            op_log: VecDeque::new(),
            peers: HashMap::new(),
            next_peer_id: 0,
        }
    }

    fn peer_names(&self) -> Vec<String> {
        self.peers.values().map(|p| p.username.clone()).collect()
    }

    fn add_peer(&mut self, username: String, tx: tokio_mpsc::UnboundedSender<ServerMsg>) -> u64 {
        let id = self.next_peer_id;
        self.next_peer_id += 1;
        self.peers.insert(
            id,
            Peer {
                username,
                tx,
                cursor_pos: 0,
            },
        );
        id
    }

    fn remove_peer(&mut self, id: u64) -> Option<String> {
        self.peers.remove(&id).map(|p| p.username)
    }

    fn broadcast_all(&self, msg: &ServerMsg) {
        for peer in self.peers.values() {
            let _ = peer.tx.send(msg.clone());
        }
    }

    fn broadcast_except(&self, msg: &ServerMsg, except_id: u64) {
        for (id, peer) in &self.peers {
            if *id != except_id {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    /// Apply a client operation, transforming it against concurrent server ops.
    /// Returns the transformed (kind, pos, text) that was actually applied.
    fn apply_op(
        &mut self,
        kind: OpKind,
        mut pos: usize,
        text: String,
        client_rev: u64,
    ) -> (OpKind, usize, String) {
        // Transform against all ops that happened after client_rev.
        for logged in &self.op_log {
            if logged.rev > client_rev {
                pos = transform_pos(&kind, pos, &text, &logged.kind, logged.pos, &logged.text);
            }
        }

        // Clamp to document bounds and apply.
        let doc_len = self.rope.len_chars();
        match &kind {
            OpKind::Insert => {
                let p = pos.min(doc_len);
                self.rope.insert(p, &text);
            }
            OpKind::Delete => {
                let p = pos.min(doc_len);
                let len = text.chars().count().min(doc_len.saturating_sub(p));
                if len > 0 {
                    self.rope.remove(p..p + len);
                }
            }
        }

        self.revision += 1;
        self.op_log.push_back(LoggedOp {
            rev: self.revision,
            kind: kind.clone(),
            pos,
            text: text.clone(),
        });
        // Keep log bounded.
        if self.op_log.len() > 2000 {
            self.op_log.pop_front();
        }

        (kind, pos, text)
    }
}

/// Task that reads local (host) ops from the editor and applies them to the
/// canonical state, then broadcasts to all guest peers.
async fn host_op_task(
    state: Arc<Mutex<ServerState>>,
    mut op_rx: tokio_mpsc::UnboundedReceiver<(OpKind, usize, String)>,
    mut cursor_rx: tokio_mpsc::UnboundedReceiver<usize>,
    host_username: String,
    event_tx: std::sync::mpsc::SyncSender<CollabEvent>,
) {
    loop {
        tokio::select! {
            Some((kind, pos, text)) = op_rx.recv() => {
                let new_rev = {
                    let mut s = state.lock().unwrap();
                    let client_rev = s.revision; // host is always current
                    let (k, p, t) = s.apply_op(kind, pos, text, client_rev);
                    let rev = s.revision;
                    // Broadcast to all guests.
                    s.broadcast_all(&ServerMsg::Op {
                        op: k,
                        pos: p,
                        text: t,
                        rev,
                        peer: host_username.clone(),
                    });
                    rev
                };
                // Notify the host editor that the op is confirmed (revision update only —
                // the host already applied the edit optimistically; do NOT re-apply).
                let _ = event_tx.try_send(CollabEvent::LocalConfirm { rev: new_rev });
            }
            Some(pos) = cursor_rx.recv() => {
                let s = state.lock().unwrap();
                s.broadcast_all(&ServerMsg::Cursor {
                    peer: host_username.clone(),
                    pos,
                });
            }
            else => break,
        }
    }
}

async fn handle_guest(
    stream: tokio::net::TcpStream,
    state: Arc<Mutex<ServerState>>,
    host_event_tx: std::sync::mpsc::SyncSender<CollabEvent>,
    collab_state: Arc<Mutex<CollabState>>,
) {
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    // First message must be Join.
    let username: String = match read_msg::<_, ClientMsg>(&mut reader).await {
        Ok(ClientMsg::Join { username }) => username,
        _ => return,
    };

    // Create outbound channel for this peer.
    let (peer_tx, mut peer_rx) = tokio_mpsc::unbounded_channel::<ServerMsg>();

    // Register peer and send Sync.
    let (peer_id, sync_msg) = {
        let mut s = state.lock().unwrap();
        let content: String = s.rope.to_string();
        let rev = s.revision;
        let id = s.add_peer(username.clone(), peer_tx);
        let peers = s.peer_names();
        // Broadcast PeerUpdate to all others.
        s.broadcast_except(
            &ServerMsg::PeerUpdate {
                peers: peers.clone(),
                event: PeerEvent::Joined,
                username: username.clone(),
            },
            id,
        );
        (
            id,
            ServerMsg::Sync {
                content,
                rev,
                peers,
            },
        )
    };

    // Update shared collab state.
    {
        let mut cs = collab_state.lock().unwrap();
        cs.peers = state.lock().unwrap().peer_names();
    }
    let _ = host_event_tx.try_send(CollabEvent::PeersChanged {
        peers: state.lock().unwrap().peer_names(),
    });

    if write_msg(&mut writer, &sync_msg).await.is_err() {
        state.lock().unwrap().remove_peer(peer_id);
        return;
    }
    let _ = AsyncWriteExt::flush(&mut writer).await; // tokio BufWriter flush

    // Spawn writer task.
    let (disconnect_tx, mut disconnect_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        while let Some(msg) = peer_rx.recv().await {
            if write_msg(&mut writer, &msg).await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
        let _ = disconnect_tx.send(());
    });

    // Reader loop.
    loop {
        tokio::select! {
            result = read_msg::<_, ClientMsg>(&mut reader) => {
                match result {
                    Ok(ClientMsg::Op { op, pos, text, rev }) => {
                        let (t_kind, t_pos, t_text, new_rev) = {
                            let mut s = state.lock().unwrap();
                            let (k, p, t) = s.apply_op(op, pos, text, rev);
                            let new_rev = s.revision;
                            // Broadcast to ALL peers (including originator).
                            s.broadcast_all(&ServerMsg::Op {
                                op: k.clone(),
                                pos: p,
                                text: t.clone(),
                                rev: new_rev,
                                peer: username.clone(),
                            });
                            (k, p, t, new_rev)
                        };
                        // Forward to host editor.
                        let _ = host_event_tx.try_send(CollabEvent::Edit {
                            kind: t_kind,
                            pos: t_pos,
                            text: t_text,
                            peer: username.clone(),
                            rev: new_rev,
                        });
                    }
                    Ok(ClientMsg::Cursor { pos }) => {
                        let s = state.lock().unwrap();
                        // Update cursor in state.
                        // Broadcast to all others (host included).
                        s.broadcast_except(
                            &ServerMsg::Cursor { peer: username.clone(), pos },
                            peer_id,
                        );
                        drop(s);
                        {
                            let mut cs = collab_state.lock().unwrap();
                            cs.peer_cursors.insert(username.clone(), pos);
                        }
                        let _ = host_event_tx.try_send(CollabEvent::PeerCursor {
                            peer: username.clone(),
                            pos,
                        });
                    }
                    Ok(ClientMsg::Join { .. }) | Err(_) => break,
                }
            }
            _ = &mut disconnect_rx => break,
        }
    }

    // Cleanup.
    let removed = state.lock().unwrap().remove_peer(peer_id);
    if let Some(name) = removed {
        let peers = state.lock().unwrap().peer_names();
        {
            let s = state.lock().unwrap();
            s.broadcast_all(&ServerMsg::PeerUpdate {
                peers: peers.clone(),
                event: PeerEvent::Left,
                username: name.clone(),
            });
        }
        {
            let mut cs = collab_state.lock().unwrap();
            cs.peers = peers.clone();
            cs.peer_cursors.remove(&name);
        }
        let _ = host_event_tx.try_send(CollabEvent::PeersChanged { peers });
    }
}

/// Start the collaboration server, bind a random TCP port, and return the
/// bound port together with a `CollabHandle` for the host editor.
///
/// The server runs in a background thread with its own Tokio runtime.
pub fn start_server(
    initial_content: String,
    username: String,
    host: String,
) -> Result<(u16, CollabHandle)> {
    // Channels between the editor (sync) and the async background tasks.
    let (op_tx, op_rx_std) = std::sync::mpsc::sync_channel::<(OpKind, usize, String)>(256);
    let (cursor_tx, cursor_rx_std) = std::sync::mpsc::sync_channel::<usize>(64);
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel::<CollabEvent>(1024);

    // Bind the listener synchronously so we know the port before returning.
    let listener = std::net::TcpListener::bind("0.0.0.0:0")?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;

    let collab_state = Arc::new(Mutex::new(CollabState {
        peers: vec![],
        peer_cursors: HashMap::new(),
        connected: true,
    }));
    let collab_state_clone = Arc::clone(&collab_state);

    let event_tx_clone = event_tx.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let server_state = Arc::new(Mutex::new(ServerState::new(&initial_content)));

            // Bridge std mpsc → tokio mpsc for the async tasks.
            let (tok_op_tx, tok_op_rx) = tokio_mpsc::unbounded_channel();
            let (tok_cursor_tx, tok_cursor_rx) = tokio_mpsc::unbounded_channel();

            // Forward from std channels to tokio channels.
            let tok_op_tx2 = tok_op_tx.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    while let Ok(op) = op_rx_std.try_recv() {
                        let _ = tok_op_tx2.send(op);
                    }
                    while let Ok(pos) = cursor_rx_std.try_recv() {
                        let _ = tok_cursor_tx.send(pos);
                    }
                }
            });

            // Start host op handler task.
            let state_clone = Arc::clone(&server_state);
            let ev_tx = event_tx_clone.clone();
            let uname = username.clone();
            tokio::spawn(host_op_task(
                state_clone,
                tok_op_rx,
                tok_cursor_rx,
                uname,
                ev_tx,
            ));

            // Accept loop.
            let listener = tokio::net::TcpListener::from_std(listener).expect("convert listener");

            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let s = Arc::clone(&server_state);
                        let etx = event_tx_clone.clone();
                        let cs = Arc::clone(&collab_state_clone);
                        tokio::spawn(handle_guest(stream, s, etx, cs));
                    }
                    Err(_) => continue,
                }
            }
        });
    });

    let handle = CollabHandle {
        role: CollabRole::Host { host, port },
        op_tx,
        cursor_tx,
        event_rx,
        state: collab_state,
        revision: 0,
    };

    Ok((port, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::client::connect_client;
    use std::time::Duration;

    /// Drain events from a CollabHandle into a String, waiting up to `timeout`.
    /// Returns the content from the first FullSync seen, accumulating Edits on top.
    fn recv_sync_content(handle: &CollabHandle, timeout: Duration) -> String {
        let deadline = std::time::Instant::now() + timeout;
        let mut content = String::new();
        while std::time::Instant::now() < deadline {
            match handle.event_rx.try_recv() {
                Ok(crate::collab::CollabEvent::FullSync { content: c, .. }) => {
                    content = c;
                    break; // got it
                }
                Ok(_) => {} // ignore other events
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
        content
    }

    #[test]
    fn server_state_two_concurrent_inserts_different_positions() {
        let mut state = ServerState::new("hello world");

        // Client A inserts "FOO" at pos 0, based on rev 0.
        let (_, pos_a, text_a) = state.apply_op(OpKind::Insert, 0, "FOO".to_string(), 0);
        assert_eq!(pos_a, 0);
        assert_eq!(text_a, "FOO");
        assert_eq!(state.revision, 1);

        // Client B inserts "BAR" at pos 11 (end of "hello world"), based on rev 0.
        // After OT, pos should shift right by 3 (len of "FOO").
        let (_, pos_b, text_b) = state.apply_op(OpKind::Insert, 11, "BAR".to_string(), 0);
        assert_eq!(
            pos_b, 14,
            "expected pos shifted from 11 to 14 after FOO insert"
        );
        assert_eq!(text_b, "BAR");
        assert_eq!(state.revision, 2);

        let final_content: String = state.rope.to_string();
        assert_eq!(final_content, "FOOhello worldBAR");
    }

    #[test]
    fn server_state_two_concurrent_inserts_same_position_no_data_loss() {
        let mut state = ServerState::new("abc");

        // Both insert at pos 1, based on rev 0.
        // First insert wins (server processes it first).
        state.apply_op(OpKind::Insert, 1, "X".to_string(), 0);
        // Second insert at pos 1, based on rev 0.  OT shifts it to pos 2
        // (server wins at same pos → client shifts right).
        state.apply_op(OpKind::Insert, 1, "Y".to_string(), 0);

        let final_content: String = state.rope.to_string();
        assert!(
            final_content.contains('X'),
            "missing X: {:?}",
            final_content
        );
        assert!(
            final_content.contains('Y'),
            "missing Y: {:?}",
            final_content
        );
        assert!(
            final_content.contains('a'),
            "missing a: {:?}",
            final_content
        );
        // All original chars preserved.
        assert_eq!(final_content.chars().filter(|&c| c == 'a').count(), 1);
    }

    #[test]
    fn server_state_concurrent_insert_and_delete() {
        let mut state = ServerState::new("hello world");

        // Client A inserts "!" at pos 5 (after "hello"), rev 0.
        state.apply_op(OpKind::Insert, 5, "!".to_string(), 0);
        // "hello! world", revision = 1

        // Client B deletes char at pos 0 ("h"), rev 0.
        // OT: delete pos 0 vs insert at pos 5. Client B's pos < server's pos, unchanged.
        state.apply_op(OpKind::Delete, 0, "h".to_string(), 0);
        // "ello! world", revision = 2

        let content: String = state.rope.to_string();
        assert!(content.contains("ello"), "expected ello in {:?}", content);
        assert!(content.contains('!'), "expected ! in {:?}", content);
    }

    #[test]
    fn network_client_receives_initial_sync() {
        let (port, _host) = start_server(
            "test content".to_string(),
            "host".to_string(),
            "127.0.0.1".to_string(),
        )
        .expect("start server");
        std::thread::sleep(Duration::from_millis(100));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        let client = connect_client(addr, "guest".to_string()).expect("connect");
        let content = recv_sync_content(&client, Duration::from_secs(2));
        assert_eq!(content, "test content");
    }

    #[test]
    fn network_disconnect_guest_does_not_crash_host() {
        let (port, _host) = start_server(
            "hello".to_string(),
            "host".to_string(),
            "127.0.0.1".to_string(),
        )
        .expect("start server");
        std::thread::sleep(Duration::from_millis(100));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        let client = connect_client(addr, "guest".to_string()).expect("connect guest");
        let _ = recv_sync_content(&client, Duration::from_millis(500));
        drop(client);

        // Server must still be alive.
        std::thread::sleep(Duration::from_millis(200));
        let client2 = connect_client(addr, "new".to_string()).expect("reconnect");
        let content = recv_sync_content(&client2, Duration::from_secs(2));
        assert_eq!(content, "hello");
    }

    #[test]
    fn network_client_reconnect_gets_fresh_sync() {
        let (port, _host) = start_server(
            "data".to_string(),
            "host".to_string(),
            "127.0.0.1".to_string(),
        )
        .expect("start server");
        std::thread::sleep(Duration::from_millis(100));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        // First connection.
        let c1 = connect_client(addr, "guest".to_string()).expect("first connect");
        let _ = recv_sync_content(&c1, Duration::from_millis(500));
        drop(c1);
        std::thread::sleep(Duration::from_millis(100));

        // Second connection should also receive full sync.
        let c2 = connect_client(addr, "guest".to_string()).expect("reconnect");
        let content = recv_sync_content(&c2, Duration::from_secs(2));
        assert_eq!(content, "data");
    }

    #[test]
    fn network_convergence_different_positions() {
        let (port, _host) = start_server(
            "hello world".to_string(),
            "host".to_string(),
            "127.0.0.1".to_string(),
        )
        .expect("start server");
        std::thread::sleep(Duration::from_millis(100));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        let client_a = connect_client(addr, "A".to_string()).expect("connect A");
        let _ = recv_sync_content(&client_a, Duration::from_millis(500));

        let client_b = connect_client(addr, "B".to_string()).expect("connect B");
        let _ = recv_sync_content(&client_b, Duration::from_millis(500));

        // Send concurrent ops.
        client_a.send_insert(0, "FOO".to_string());
        client_b.send_insert(11, "BAR".to_string());

        // Wait for the server to process them.
        std::thread::sleep(Duration::from_millis(300));

        // Verify via a fresh sync.
        let verify = connect_client(addr, "v".to_string()).expect("connect verify");
        let content = recv_sync_content(&verify, Duration::from_secs(2));

        assert!(content.contains("FOO"), "missing FOO in {:?}", content);
        assert!(content.contains("BAR"), "missing BAR in {:?}", content);
        assert!(content.contains("hello"), "missing hello in {:?}", content);
    }

    #[test]
    fn network_convergence_same_position_no_data_loss() {
        let (port, _host) = start_server(
            "abc".to_string(),
            "host".to_string(),
            "127.0.0.1".to_string(),
        )
        .expect("start server");
        std::thread::sleep(Duration::from_millis(100));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        let client_a = connect_client(addr, "A".to_string()).expect("connect A");
        let _ = recv_sync_content(&client_a, Duration::from_millis(500));
        let client_b = connect_client(addr, "B".to_string()).expect("connect B");
        let _ = recv_sync_content(&client_b, Duration::from_millis(500));

        client_a.send_insert(1, "X".to_string());
        client_b.send_insert(1, "Y".to_string());
        std::thread::sleep(Duration::from_millis(300));

        let verify = connect_client(addr, "v".to_string()).expect("connect verify");
        let content = recv_sync_content(&verify, Duration::from_secs(2));

        assert!(content.contains('X'), "missing X: {:?}", content);
        assert!(content.contains('Y'), "missing Y: {:?}", content);
        assert!(content.contains('a'), "missing a: {:?}", content);
    }
}
