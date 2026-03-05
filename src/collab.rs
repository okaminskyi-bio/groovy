use crossbeam_channel::{Receiver, Sender};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tungstenite::{Error as WsError, Message, accept, connect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Pending,
    Approved,
    Denied,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollabMessage {
    pub id: i64,
    pub author: String,
    pub body: String,
    pub code_quote: String,
    pub decision: Decision,
    pub created_at: String,
    pub parent_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsEnvelope {
    pub event: String,
    pub payload: String,
}

pub struct WsHandle {
    incoming_rx: Receiver<String>,
    outgoing_tx: Sender<String>,
    stop_tx: Sender<()>,
}

impl WsHandle {
    pub fn send(&self, message: String) {
        let _ = self.outgoing_tx.send(message);
    }

    pub fn try_recv(&self) -> Option<String> {
        self.incoming_rx.try_recv().ok()
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(());
    }
}

impl Drop for WsHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

pub fn ensure_db(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            author TEXT NOT NULL,
            body TEXT NOT NULL,
            code_quote TEXT NOT NULL DEFAULT '',
            decision TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            parent_id INTEGER
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn add_message(
    db_path: &Path,
    author: &str,
    body: &str,
    code_quote: &str,
    parent_id: Option<i64>,
) -> Result<i64, String> {
    ensure_db(db_path)?;
    let mut conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO messages (author, body, code_quote, decision, created_at, parent_id)
         VALUES (?1, ?2, ?3, 'pending', datetime('now'), ?4)",
        params![author, body, code_quote, parent_id],
    )
    .map_err(|e| e.to_string())?;
    let id = tx.last_insert_rowid();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn set_decision(db_path: &Path, id: i64, decision: Decision) -> Result<(), String> {
    ensure_db(db_path)?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let decision_text = match decision {
        Decision::Pending => "pending",
        Decision::Approved => "approved",
        Decision::Denied => "denied",
    };
    conn.execute(
        "UPDATE messages SET decision = ?1 WHERE id = ?2",
        params![decision_text, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_messages(db_path: &Path, limit: usize) -> Result<Vec<CollabMessage>, String> {
    ensure_db(db_path)?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, author, body, code_quote, decision, created_at, parent_id
             FROM messages ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let decision_text: String = row.get(4)?;
            Ok(CollabMessage {
                id: row.get(0)?,
                author: row.get(1)?,
                body: row.get(2)?,
                code_quote: row.get(3)?,
                decision: match decision_text.as_str() {
                    "approved" => Decision::Approved,
                    "denied" => Decision::Denied,
                    _ => Decision::Pending,
                },
                created_at: row.get(5)?,
                parent_id: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn start_server(addr: &str) -> Result<WsHandle, String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("Bind failed on {addr}: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("Cannot set nonblocking listener: {e}"))?;

    let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded::<String>();
    let (outgoing_tx, outgoing_rx) = crossbeam_channel::unbounded::<String>();
    let (stop_tx, stop_rx) = crossbeam_channel::unbounded::<()>();

    thread::spawn(move || {
        let mut peers: Vec<Sender<String>> = Vec::new();
        let (fanout_tx, fanout_rx) = crossbeam_channel::unbounded::<String>();

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if stream.set_nonblocking(true).is_ok() {
                            match accept(stream) {
                                Ok(ws) => {
                                    let peer_tx = spawn_peer(ws, fanout_tx.clone());
                                    peers.push(peer_tx);
                                    let _ = incoming_tx.send("Client connected".to_string());
                                }
                                Err(err) => {
                                    let _ = incoming_tx.send(format!("WS accept error: {err}"));
                                }
                            }
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                    Err(err) => {
                        let _ = incoming_tx.send(format!("Accept error: {err}"));
                        break;
                    }
                }
            }

            while let Ok(msg) = outgoing_rx.try_recv() {
                let _ = incoming_tx.send(format!("Local event: {msg}"));
                peers.retain(|peer| peer.send(msg.clone()).is_ok());
            }

            while let Ok(msg) = fanout_rx.try_recv() {
                let _ = incoming_tx.send(format!("Remote event: {msg}"));
                peers.retain(|peer| peer.send(msg.clone()).is_ok());
            }

            thread::sleep(Duration::from_millis(20));
        }
    });

    Ok(WsHandle {
        incoming_rx,
        outgoing_tx,
        stop_tx,
    })
}

pub fn start_client(url: &str) -> Result<WsHandle, String> {
    let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded::<String>();
    let (outgoing_tx, outgoing_rx) = crossbeam_channel::unbounded::<String>();
    let (stop_tx, stop_rx) = crossbeam_channel::unbounded::<()>();
    let url_str = url.to_string();

    thread::spawn(move || {
        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            match connect(url_str.as_str()) {
                Ok((mut ws, _)) => {
                    let _ = incoming_tx.send(format!("Connected to {url_str}"));
                    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
                        let _ = stream.set_nonblocking(true);
                    }
                    loop {
                        if stop_rx.try_recv().is_ok() {
                            return;
                        }

                        match ws.read() {
                            Ok(Message::Text(text)) => {
                                let _ = incoming_tx.send(text.to_string());
                            }
                            Ok(_) => {}
                            Err(WsError::Io(err)) if err.kind() == io::ErrorKind::WouldBlock => {}
                            Err(WsError::ConnectionClosed) => {
                                let _ = incoming_tx.send("Connection closed".to_string());
                                break;
                            }
                            Err(err) => {
                                let _ = incoming_tx.send(format!("WS read error: {err}"));
                                break;
                            }
                        }

                        while let Ok(msg) = outgoing_rx.try_recv() {
                            if ws.send(Message::Text(msg.clone().into())).is_err() {
                                let _ = incoming_tx.send("Send failed, reconnecting".to_string());
                                break;
                            }
                        }

                        thread::sleep(Duration::from_millis(20));
                    }
                }
                Err(err) => {
                    let _ = incoming_tx.send(format!("Connect failed: {err}"));
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });

    Ok(WsHandle {
        incoming_rx,
        outgoing_tx,
        stop_tx,
    })
}

fn spawn_peer<S>(mut ws: tungstenite::WebSocket<S>, fanout_tx: Sender<String>) -> Sender<String>
where
    S: std::io::Read + std::io::Write + Send + 'static,
{
    let (tx, rx) = crossbeam_channel::unbounded::<String>();
    thread::spawn(move || {
        let _ = ws.get_mut();
        loop {
            match ws.read() {
                Ok(Message::Text(text)) => {
                    let _ = fanout_tx.send(text.to_string());
                }
                Ok(_) => {}
                Err(WsError::Io(err)) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(WsError::ConnectionClosed) => break,
                Err(_) => break,
            }

            while let Ok(out) = rx.try_recv() {
                if ws.send(Message::Text(out.into())).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    tx
}
