use super::{
    lag::Lag,
    wire::{Framer, Message},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc, oneshot, watch},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
const QUEUE: usize = 128;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub enum Event {
    Connected {
        id: u64,
        sender: mpsc::Sender<Message>,
        close: oneshot::Sender<()>,
    },
    Message {
        id: u64,
        message: Message,
    },
    Disconnected {
        id: u64,
    },
    Mined {
        generation: u64,
        block: crate::protocol::Block,
    },
}
async fn connection(
    stream: impl AsyncRead + AsyncWrite + Unpin,
    events: mpsc::Sender<Event>,
    enabled: bool,
    seed: u64,
) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, mut receiver) = mpsc::channel::<Message>(QUEUE);
    let (close, mut closed) = oneshot::channel();
    if events
        .send(Event::Connected { id, sender, close })
        .await
        .is_err()
    {
        return;
    }
    {
        let (mut read, mut write) = tokio::io::split(stream);
        let writer = async move {
            let mut lag = Lag::new(enabled, seed ^ id);
            while let Some(message) = receiver.recv().await {
                let Some(delay) = lag.schedule() else {
                    continue;
                };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let Ok(mut bytes) = serde_json::to_vec(&message) else {
                    break;
                };
                bytes.push(b'\n');
                if !matches!(
                    timeout(Duration::from_secs(5), write.write_all(&bytes)).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        };
        tokio::pin!(writer);
        let mut framer = Framer::default();
        let mut bytes = [0; 8192];
        'session: loop {
            tokio::select! {
                _ = &mut closed => break,
                _ = &mut writer => break,
                _ = tokio::time::sleep(Duration::from_secs(30)) => break,
                result = read.read(&mut bytes) => {
                    let Ok(count) = result else { break; };
                    if count == 0 { break; }
                    let Ok(messages) = framer.push(&bytes[..count]) else { break; };
                    for message in messages {
                        tokio::select! {
                            _ = &mut closed => break 'session,
                            result = events.send(Event::Message { id, message }) => {
                                if result.is_err() { return; }
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = events.send(Event::Disconnected { id }).await;
}
/// Own every network task so returning from the coordinator closes its sockets.
pub struct Network {
    tasks: Vec<JoinHandle<()>>,
    stop: watch::Sender<bool>,
}
impl Network {
    /// Wait until every listener, reconnect loop, and transport has closed.
    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
pub async fn start(
    listen: SocketAddr,
    peers: Vec<SocketAddr>,
    events: mpsc::Sender<Event>,
    enabled: bool,
    seed: u64,
) -> crate::Result<Network> {
    let listener = TcpListener::bind(listen).await?;
    let (stop, mut stopped) = watch::channel(false);
    let incoming_events = events.clone();
    let slots = Arc::new(Semaphore::new(128));
    let mut tasks = Vec::new();
    tasks.push(tokio::spawn(async move {
        let mut incoming = JoinSet::new();
        loop {
            let stream = tokio::select! {
                biased;
                _ = stopped.changed() => break,
                result = listener.accept() => match result {
                    Ok((stream, _)) => stream,
                    Err(_) => break,
                },
                _ = incoming.join_next(), if !incoming.is_empty() => continue,
            };
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let events = incoming_events.clone();
            incoming.spawn(async move {
                let _permit = permit;
                connection(stream, events, enabled, seed).await;
            });
        }
        incoming.shutdown().await;
    }));
    let peers: std::collections::BTreeSet<_> =
        peers.into_iter().filter(|peer| *peer != listen).collect();
    for peer in peers {
        let events = events.clone();
        let mut stopped = stop.subscribe();
        tasks.push(tokio::spawn(async move {
            loop {
                let session = async {
                    if let Ok(Ok(stream)) =
                        timeout(Duration::from_secs(5), TcpStream::connect(peer)).await
                    {
                        connection(stream, events.clone(), enabled, seed).await;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                };
                tokio::select! {
                    biased;
                    _ = stopped.changed() => break,
                    _ = session => {},
                }
            }
        }));
    }
    Ok(Network { tasks, stop })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removing_session_closes_transport_without_draining_lag_queue() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(16);
        let task = tokio::spawn(connection(stream, events, true, 42));
        let Event::Connected { sender, close, .. } = receiver.recv().await.unwrap() else {
            panic!("expected connection");
        };
        for _ in 0..QUEUE {
            sender.try_send(Message::GetTip {}).unwrap();
        }
        drop(sender);
        drop(close);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
        let mut data = Vec::new();
        timeout(Duration::from_millis(500), remote.read_to_end(&mut data))
            .await
            .unwrap()
            .unwrap();
        assert!(data.len() < QUEUE * b"{\"type\":\"get_tip\"}\n".len());
    }

    #[tokio::test]
    async fn session_cancellation_interrupts_coordinator_backpressure() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(connection(stream, events, false, 1));
        let Event::Connected {
            sender: _sender,
            close,
            ..
        } = receiver.recv().await.unwrap()
        else {
            panic!("expected connection");
        };
        remote
            .write_all(b"{\"type\":\"get_tip\"}\n{\"type\":\"get_tip\"}\n")
            .await
            .unwrap();
        tokio::task::yield_now().await;
        drop(close);
        let mut byte = [0];
        assert_eq!(
            timeout(Duration::from_millis(500), remote.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        drop(receiver);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn transport_closes_before_backpressured_disconnect_notification() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(connection(stream, events.clone(), false, 1));
        let Event::Connected {
            sender: _sender,
            close,
            ..
        } = receiver.recv().await.unwrap()
        else {
            panic!("expected connection");
        };
        events.send(Event::Disconnected { id: 0 }).await.unwrap();
        drop(close);
        let mut byte = [0];
        assert_eq!(
            timeout(Duration::from_millis(500), remote.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        drop(receiver);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}
