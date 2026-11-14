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
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    time::timeout,
};
const QUEUE: usize = 128;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub enum Event {
    Connected {
        id: u64,
        sender: mpsc::Sender<Message>,
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
async fn connection(stream: TcpStream, events: mpsc::Sender<Event>, enabled: bool, seed: u64) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, mut receiver) = mpsc::channel::<Message>(QUEUE);
    if events.send(Event::Connected { id, sender }).await.is_err() {
        return;
    }
    let (mut read, mut write) = stream.into_split();
    let writer = tokio::spawn(async move {
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
    });
    let mut framer = Framer::default();
    let mut bytes = [0; 8192];
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(30)) => break,
            result = read.read(&mut bytes) => {
                let Ok(count) = result else { break; };
                if count == 0 { break; }
                let Ok(messages) = framer.push(&bytes[..count]) else { break; };
                for message in messages {
                    if events.send(Event::Message { id, message }).await.is_err() { break; }
                }
            }
        }
        if writer.is_finished() {
            break;
        }
    }
    writer.abort();
    let _ = events.send(Event::Disconnected { id }).await;
}
pub async fn start(
    listen: SocketAddr,
    peers: Vec<SocketAddr>,
    events: mpsc::Sender<Event>,
    enabled: bool,
    seed: u64,
) -> crate::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let incoming_events = events.clone();
    let slots = Arc::new(Semaphore::new(128));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let events = incoming_events.clone();
            tokio::spawn(async move {
                let _permit = permit;
                connection(stream, events, enabled, seed).await;
            });
        }
    });
    let peers: std::collections::BTreeSet<_> =
        peers.into_iter().filter(|peer| *peer != listen).collect();
    for peer in peers {
        let events = events.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(Ok(stream)) =
                    timeout(Duration::from_secs(5), TcpStream::connect(peer)).await
                {
                    connection(stream, events.clone(), enabled, seed).await;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    Ok(())
}
