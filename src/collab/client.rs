// Collaboration guest / TCP client.
//
// Connects to a host, sends local operations, receives and applies
// remote operations with optimistic local prediction.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use tokio::net::TcpStream;

use super::{
    ClientMsg, CollabEvent, CollabHandle, CollabRole, CollabState, OpKind, PeerEvent, ServerMsg,
    read_msg, write_msg,
};

/// Task that manages the TCP connection, sending local ops and receiving
/// remote ops.  Automatically retries on disconnect (up to 10 times).
async fn connection_task(
    addr: SocketAddr,
    username: String,
    op_rx: std::sync::mpsc::Receiver<(OpKind, usize, String)>,
    cursor_rx: std::sync::mpsc::Receiver<usize>,
    event_tx: std::sync::mpsc::SyncSender<CollabEvent>,
    state: Arc<Mutex<CollabState>>,
    init_tx: Option<std::sync::mpsc::SyncSender<Result<()>>>,
) {
    let mut retry_count = 0;
    let mut init_tx = init_tx;

    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                retry_count = 0;
                {
                    let mut s = state.lock().unwrap();
                    s.connected = true;
                }
                let _ = event_tx.try_send(CollabEvent::ConnectionStatus { connected: true });

                let disconnected = run_session(
                    stream,
                    &username,
                    &op_rx,
                    &cursor_rx,
                    &event_tx,
                    &state,
                    &mut init_tx,
                )
                .await;

                {
                    let mut s = state.lock().unwrap();
                    s.connected = false;
                }
                let _ = event_tx.try_send(CollabEvent::ConnectionStatus { connected: false });

                if !disconnected {
                    // Clean exit — don't retry.
                    break;
                }
            }
            Err(e) => {
                if let Some(tx) = init_tx.take() {
                    let _ = tx.try_send(Err(anyhow::anyhow!("Connect failed: {}", e)));
                    return;
                }
            }
        }

        retry_count += 1;
        if retry_count > 10 {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Run one connected session.  Returns `true` if the disconnection was
/// unexpected (should retry), `false` if it was a clean exit.
async fn run_session(
    stream: TcpStream,
    username: &str,
    op_rx: &std::sync::mpsc::Receiver<(OpKind, usize, String)>,
    cursor_rx: &std::sync::mpsc::Receiver<usize>,
    event_tx: &std::sync::mpsc::SyncSender<CollabEvent>,
    state: &Arc<Mutex<CollabState>>,
    init_tx: &mut Option<std::sync::mpsc::SyncSender<Result<()>>>,
) -> bool {
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    // Send Join.
    if write_msg(
        &mut writer,
        &ClientMsg::Join {
            username: username.to_string(),
        },
    )
    .await
    .is_err()
    {
        return true;
    }
    let _ = tokio::io::AsyncWriteExt::flush(&mut writer).await;

    // Wait for Sync.
    let (content, rev, peers) = match read_msg::<_, ServerMsg>(&mut reader).await {
        Ok(ServerMsg::Sync {
            content,
            rev,
            peers,
        }) => (content, rev, peers),
        _ => return true,
    };

    {
        let mut s = state.lock().unwrap();
        s.peers = peers.clone();
    }
    let _ = event_tx.try_send(CollabEvent::FullSync { content, rev });
    let _ = event_tx.try_send(CollabEvent::PeersChanged { peers });

    // Signal successful initialization.
    if let Some(tx) = init_tx.take() {
        let _ = tx.try_send(Ok(()));
    }

    let mut local_rev = rev;

    // Main loop: poll for local ops/cursors to send, and read incoming messages.
    loop {
        // Drain local op queue and send.
        while let Ok((kind, pos, text)) = op_rx.try_recv() {
            let msg = ClientMsg::Op {
                op: kind,
                pos,
                text,
                rev: local_rev,
            };
            if write_msg(&mut writer, &msg).await.is_err() {
                return true;
            }
        }
        while let Ok(pos) = cursor_rx.try_recv() {
            let msg = ClientMsg::Cursor { pos };
            if write_msg(&mut writer, &msg).await.is_err() {
                return true;
            }
        }
        let _ = tokio::io::AsyncWriteExt::flush(&mut writer).await;

        // Check for incoming server message (with short timeout so we keep
        // sending local ops promptly).
        let read_result = tokio::time::timeout(
            Duration::from_millis(16),
            read_msg::<_, ServerMsg>(&mut reader),
        )
        .await;

        match read_result {
            Ok(Ok(msg)) => match msg {
                ServerMsg::Op {
                    op,
                    pos,
                    text,
                    rev,
                    peer,
                } => {
                    local_rev = rev;
                    if peer == username {
                        // This is the server's confirmation of our own local op; the
                        // edit was already applied optimistically — just update revision.
                        let _ = event_tx.try_send(CollabEvent::LocalConfirm { rev });
                    } else {
                        let _ = event_tx.try_send(CollabEvent::Edit {
                            kind: op,
                            pos,
                            text,
                            peer,
                            rev,
                        });
                    }
                }
                ServerMsg::Sync {
                    content,
                    rev,
                    peers,
                } => {
                    local_rev = rev;
                    {
                        let mut s = state.lock().unwrap();
                        s.peers = peers.clone();
                    }
                    let _ = event_tx.try_send(CollabEvent::FullSync { content, rev });
                    let _ = event_tx.try_send(CollabEvent::PeersChanged { peers });
                }
                ServerMsg::PeerUpdate {
                    peers,
                    event,
                    username: uname,
                } => {
                    {
                        let mut s = state.lock().unwrap();
                        s.peers = peers.clone();
                        if matches!(event, PeerEvent::Left) {
                            s.peer_cursors.remove(&uname);
                        }
                    }
                    let _ = event_tx.try_send(CollabEvent::PeersChanged { peers });
                }
                ServerMsg::Cursor { peer, pos } => {
                    {
                        let mut s = state.lock().unwrap();
                        s.peer_cursors.insert(peer.clone(), pos);
                    }
                    let _ = event_tx.try_send(CollabEvent::PeerCursor { peer, pos });
                }
            },
            Ok(Err(_)) => return true, // connection error
            Err(_) => {}               // timeout — continue sending loop
        }
    }
}

/// Connect to a collaboration host and return a `CollabHandle` for the guest
/// editor.  Blocks until the initial sync is received.
pub fn connect_client(addr: SocketAddr, username: String) -> Result<CollabHandle> {
    let (op_tx, op_rx) = std::sync::mpsc::sync_channel::<(OpKind, usize, String)>(256);
    let (cursor_tx, cursor_rx) = std::sync::mpsc::sync_channel::<usize>(64);
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel::<CollabEvent>(1024);

    let collab_state = Arc::new(Mutex::new(CollabState {
        peers: vec![],
        peer_cursors: HashMap::new(),
        connected: false,
    }));
    let state_clone = Arc::clone(&collab_state);

    let host_str = addr.ip().to_string();
    let port = addr.port();

    // Simple std channel for init signalling (no tokio dependency).
    let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);

    let event_tx_clone = event_tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(connection_task(
            addr,
            username,
            op_rx,
            cursor_rx,
            event_tx_clone,
            state_clone,
            Some(init_tx),
        ));
    });

    // Block until the connection is established and the Sync message arrives.
    init_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| anyhow::anyhow!("Connection timed out"))??;

    Ok(CollabHandle {
        role: CollabRole::Guest {
            host: host_str,
            port,
        },
        op_tx,
        cursor_tx,
        event_rx,
        state: collab_state,
        revision: 0,
    })
}
