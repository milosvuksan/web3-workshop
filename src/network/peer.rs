use super::{
    lag::Lag,
    wire::{Framer, Message},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
    task::{JoinHandle, JoinSet},
    time::{Instant, sleep_until, timeout},
};
const QUEUE: usize = 128;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Schedule at enqueue time, rather than adding a fresh delay after every write.
/// The permit covers queued AND in-flight messages, enforcing the 128 limit.
pub struct Sender {
    sender: mpsc::Sender<Queued>,
    schedule: Mutex<(Lag, Instant)>,
    slots: Arc<Semaphore>,
}
pub(crate) struct Queued {
    message: Message,
    when: Instant,
    _permit: OwnedSemaphorePermit,
}
impl Sender {
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
    pub fn try_send(&self, message: Message) -> Result<(), &'static str> {
        let mut schedule = self.schedule.lock().expect("lag schedule");
        if self.sender.is_closed() {
            return Err("closed session");
        }
        let Some(delay) = schedule.0.schedule() else {
            return Ok(());
        };
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "send queue full")?;
        let when = (Instant::now() + Duration::from_millis(delay)).max(schedule.1);
        schedule.1 = when;
        self.sender
            .try_send(Queued {
                message,
                when,
                _permit: permit,
            })
            .map_err(|_| "closed session")
    }
}
pub(crate) fn outbox(enabled: bool, seed: u64) -> (Sender, mpsc::Receiver<Queued>) {
    let (sender, receiver) = mpsc::channel(QUEUE);
    (
        Sender {
            sender,
            schedule: Mutex::new((Lag::new(enabled, seed), Instant::now())),
            slots: Arc::new(Semaphore::new(QUEUE)),
        },
        receiver,
    )
}
pub enum Event {
    Connected {
        id: u64,
        sender: Sender,
        address: SocketAddr,
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
    address: SocketAddr,
    events: mpsc::Sender<Event>,
    enabled: bool,
    seed: u64,
) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, mut receiver) = outbox(enabled, seed ^ id);
    let (close, mut closed) = oneshot::channel();
    if events
        .send(Event::Connected {
            id,
            sender,
            close,
            address,
        })
        .await
        .is_err()
    {
        return;
    }
    {
        let (mut read, mut write) = tokio::io::split(stream);
        let writer = async move {
            while let Some(queued) = receiver.recv().await {
                sleep_until(queued.when).await;
                let Ok(mut bytes) = serde_json::to_vec(&queued.message) else {
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
        let idle = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(idle);
        'session: loop {
            tokio::select! {
                _ = &mut closed => break,
                _ = &mut writer => break,
                _ = &mut idle => break,
                result = read.read(&mut bytes) => {
                    let Ok(count) = result else { break; };
                    if count == 0 { break; }
                    idle.as_mut().reset(Instant::now() + Duration::from_secs(30));
                    let Ok(messages) = framer.push(&bytes[..count]) else { break; };
                    for message in messages {
                        tokio::select! {
                            _ = &mut closed => break 'session,
                            _ = &mut idle => break 'session,
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
    peers: watch::Sender<Vec<SocketAddr>>,
}
impl Network {
    /// Update outgoing targets without interrupting the listener or other peers.
    pub fn set_peers(&self, peers: Vec<SocketAddr>) {
        self.peers.send_replace(peers);
    }
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
    discovery: Option<super::discovery::Discovery>,
) -> crate::Result<Network> {
    let listener = TcpListener::bind(listen).await?;
    let tcp_port = listener.local_addr()?.port();
    let udp = discovery.map(|d| d.bind()).transpose()?;
    let (stop, mut stopped) = watch::channel(false);
    let (peer_updates, mut configured) = watch::channel(peers);
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
                    Ok((stream, address)) => (stream, address),
                    Err(_) => break,
                },
                _ = incoming.join_next(), if !incoming.is_empty() => continue,
            };
            let (stream, address) = stream;
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let events = incoming_events.clone();
            incoming.spawn(async move {
                let _permit = permit;
                connection(stream, address, events, enabled, seed).await;
            });
        }
        incoming.shutdown().await;
    }));
    let mut stopped = stop.subscribe();
    tasks.push(tokio::spawn(async move {
        let mut addresses = std::collections::BTreeMap::new();
        let mut discovered = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut outgoing = JoinSet::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut bytes = [0; 1024];
        loop {
            let desired: std::collections::BTreeSet<_> = configured.borrow_and_update().iter().copied()
                .chain(discovered.iter().copied()).filter(|address| *address != listen).collect();
            addresses.retain(|address, task: &mut tokio::task::AbortHandle| {
                if desired.contains(address) { true } else { task.abort(); false }
            });
            for address in desired {
                addresses.entry(address).or_insert_with(|| outgoing.spawn(reconnect(address, events.clone(), enabled, seed)));
            }
            tokio::select! {
                biased;
                _ = stopped.changed() => break,
                result = configured.changed() => { if result.is_err() { break; } },
                result = async {
                    match &udp {
                        Some(socket) => socket.recv_from(&mut bytes).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let (Ok((len, source)), Some(discovery)) = (result, discovery)
                        && let Some((identity, address)) = discovery.parse(&bytes[..len], source)
                        && addresses.len() < 4096
                        && identities.len() < 4096
                        && identities.insert(identity)
                        && !addresses.contains_key(&address)
                    {
                        discovered.insert(address);
                    }
                }
                _ = ticker.tick(), if udp.is_some() => {
                    let discovery = discovery.expect("discovery socket");
                    let packet = discovery.announcement(tcp_port);
                    for ip in [[255,255,255,255], [127,0,0,1]] {
                        let _ = udp.as_ref().unwrap().send_to(packet.as_bytes(), SocketAddr::from((ip, discovery.port))).await;
                    }
                }
                _ = outgoing.join_next(), if !outgoing.is_empty() => {},
            }
        }
        outgoing.shutdown().await;
    }));
    Ok(Network {
        tasks,
        stop,
        peers: peer_updates,
    })
}

async fn reconnect(peer: SocketAddr, events: mpsc::Sender<Event>, enabled: bool, seed: u64) {
    loop {
        if let Ok(Ok(stream)) = timeout(Duration::from_secs(5), TcpStream::connect(peer)).await {
            connection(stream, peer, events.clone(), enabled, seed).await;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn lag_deadlines_overlap_in_fifo_order_instead_of_accumulating() {
        let (sender, mut receiver) = outbox(true, 42);
        let start = Instant::now();
        for _ in 0..100 {
            sender.try_send(Message::GetTip {}).unwrap();
        }
        let mut last = start;
        let mut count = 0;
        while let Ok(queued) = receiver.try_recv() {
            assert!(queued.when >= last);
            assert!(queued.when <= start + Duration::from_millis(250));
            last = queued.when;
            count += 1;
        }
        assert!(count > 50 && count < 100);
    }

    #[tokio::test]
    async fn queue_limit_includes_the_message_being_written() {
        let (sender, mut receiver) = outbox(false, 1);
        sender.try_send(Message::GetTip {}).unwrap();
        let in_flight = receiver.recv().await.unwrap();
        for _ in 1..QUEUE {
            sender.try_send(Message::GetTip {}).unwrap();
        }
        assert!(sender.try_send(Message::GetTip {}).is_err());
        drop(in_flight);
        sender.try_send(Message::GetTip {}).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn outgoing_traffic_does_not_reset_the_receive_idle_timeout() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(8);
        let task = tokio::spawn(connection(
            stream,
            "127.0.0.1:1".parse().unwrap(),
            events,
            false,
            1,
        ));
        let Event::Connected {
            sender,
            close: _close,
            ..
        } = receiver.recv().await.unwrap()
        else {
            panic!("connection");
        };
        for _ in 0..14 {
            tokio::time::advance(Duration::from_secs(2)).await;
            sender.try_send(Message::GetTip {}).unwrap();
            let mut bytes = vec![0; b"{\"type\":\"get_tip\"}\n".len()];
            remote.read_exact(&mut bytes).await.unwrap();
            assert!(!task.is_finished());
        }
        tokio::time::advance(Duration::from_secs(2)).await;
        task.await.unwrap();
        assert_eq!(remote.read(&mut [0]).await.unwrap(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn partial_reads_reset_the_idle_timeout() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(8);
        let task = tokio::spawn(connection(
            stream,
            "127.0.0.1:1".parse().unwrap(),
            events,
            false,
            1,
        ));
        let Event::Connected {
            sender: _sender,
            close: _close,
            ..
        } = receiver.recv().await.unwrap()
        else {
            panic!("connection");
        };
        tokio::time::advance(Duration::from_secs(20)).await;
        remote.write_all(b" ").await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(!task.is_finished());
        tokio::time::advance(Duration::from_secs(1)).await;
        task.await.unwrap();
    }

    #[tokio::test]
    async fn removing_session_closes_transport_without_draining_lag_queue() {
        let (stream, mut remote) = tokio::io::duplex(4096);
        let (events, mut receiver) = mpsc::channel(16);
        let task = tokio::spawn(connection(
            stream,
            "127.0.0.1:1".parse().unwrap(),
            events,
            true,
            42,
        ));
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
        let task = tokio::spawn(connection(
            stream,
            "127.0.0.1:1".parse().unwrap(),
            events,
            false,
            1,
        ));
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
        let task = tokio::spawn(connection(
            stream,
            "127.0.0.1:1".parse().unwrap(),
            events.clone(),
            false,
            1,
        ));
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
