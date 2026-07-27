#![allow(non_snake_case)]

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, timeout, Duration};

const MAX_DATAGRAM_SIZE: usize = 65_535;

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub listen_addr: SocketAddr,
    pub server_addr: SocketAddr,
    pub session_idle_timeout: Duration,
    pub server_program: Option<String>,
    pub server_args: Vec<String>,
    pub startup_grace: Duration,
    pub max_sessions: usize,
    pub session_queue_size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerState {
    Stopped,
    Starting,
    Ready,
}

struct ManagedServer {
    phase: ServerState,
    child: Option<Child>,
}

struct ServerStarter {
    program: Option<String>,
    args: Vec<String>,
    startup_grace: Duration,
    state: Mutex<ManagedServer>,
}

impl ServerStarter {
    async fn ensure_ready(&self) -> io::Result<()> {
        let mut state = self.state.lock().await;
        if self.program.is_none() {
            state.phase = ServerState::Ready;
            return Ok(());
        }

        if state.phase == ServerState::Ready {
            if let Some(child) = state.child.as_mut() {
                if child.try_wait()?.is_none() {
                    return Ok(());
                }
                eprintln!("hosted server exited; it will be restarted");
            }
            state.phase = ServerState::Stopped;
            state.child = None;
        }

        state.phase = ServerState::Starting;
        let program = self.program.as_ref().expect("program checked above");

        let child = Command::new(program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        state.child = Some(child);

        sleep(self.startup_grace).await;
        if let Some(child) = state.child.as_mut() {
            if let Some(status) = child.try_wait()? {
                state.phase = ServerState::Stopped;
                state.child = None;
                return Err(io::Error::other(format!(
                    "hosted server exited during startup with {status}"
                )));
            }
        }
        state.phase = ServerState::Ready;
        Ok(())
    }
}

pub async fn run(config: ProxyConfig) -> io::Result<()> {
    validate_config(&config)?;
    let listener = Arc::new(UdpSocket::bind(config.listen_addr).await?);
    run_on_socket(listener, config).await
}

pub async fn run_on_socket(listener: Arc<UdpSocket>, config: ProxyConfig) -> io::Result<()> {
    validate_config(&config)?;
    let starter = Arc::new(ServerStarter {
        program: config.server_program.clone(),
        args: config.server_args.clone(),
        startup_grace: config.startup_grace,
        state: Mutex::new(ManagedServer {
            phase: ServerState::Stopped,
            child: None,
        }),
    });
    let mut sessions: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut buffer = vec![0_u8; MAX_DATAGRAM_SIZE];
    let mut cleanup = tokio::time::interval(Duration::from_secs(30));
    cleanup.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let (length, client_addr) = tokio::select! {
            packet = listener.recv_from(&mut buffer) => packet?,
            _ = cleanup.tick() => {
                sessions.retain(|_, session| !session.is_closed());
                continue;
            }
        };
        if let Err(error) = starter.ensure_ready().await {
            eprintln!("hosted server is not ready: {error}");
            continue;
        }
        let packet = buffer[..length].to_vec();

        if let Some(session) = sessions.get(&client_addr).cloned() {
            match session.try_send(packet) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => continue,
                Err(mpsc::error::TrySendError::Closed(packet)) => {
                    sessions.remove(&client_addr);
                    if sessions.len() >= config.max_sessions {
                        continue;
                    }
                    create_session(
                        &mut sessions,
                        listener.clone(),
                        client_addr,
                        &config,
                        packet,
                    )
                    .await?;
                    continue;
                }
            }
        }

        sessions.retain(|_, session| !session.is_closed());
        if sessions.len() >= config.max_sessions {
            continue;
        }
        create_session(
            &mut sessions,
            listener.clone(),
            client_addr,
            &config,
            packet,
        )
        .await?;
    }
}

fn validate_config(config: &ProxyConfig) -> io::Result<()> {
    if config.max_sessions == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max_sessions must be greater than zero",
        ));
    }
    if config.session_queue_size == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session_queue_size must be greater than zero",
        ));
    }
    if config.session_idle_timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session_idle_timeout must be greater than zero",
        ));
    }
    Ok(())
}

async fn create_session(
    sessions: &mut HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>,
    listener: Arc<UdpSocket>,
    client_addr: SocketAddr,
    config: &ProxyConfig,
    first_packet: Vec<u8>,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel(config.session_queue_size);
    spawn_session(
        listener,
        client_addr,
        config.server_addr,
        config.session_idle_timeout,
        receiver,
    )
    .await?;
    sender.try_send(first_packet).map_err(|_| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "new client session could not accept its first packet",
        )
    })?;
    sessions.insert(client_addr, sender);
    Ok(())
}

async fn spawn_session(
    listener: Arc<UdpSocket>,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
    idle_timeout: Duration,
    mut outbound: mpsc::Receiver<Vec<u8>>,
) -> io::Result<()> {
    let bind_addr = if server_addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let upstream = UdpSocket::bind(bind_addr).await?;
    upstream.connect(server_addr).await?;

    tokio::spawn(async move {
        let mut response = vec![0_u8; MAX_DATAGRAM_SIZE];
        loop {
            tokio::select! {
                packet = timeout(idle_timeout, outbound.recv()) => {
                    match packet {
                        Ok(Some(packet)) => {
                            if let Err(error) = upstream.send(&packet).await {
                                eprintln!("client {client_addr}: upstream send failed: {error}");
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
                result = upstream.recv(&mut response) => {
                    match result {
                        Ok(length) => {
                            if let Err(error) = listener.send_to(&response[..length], client_addr).await {
                                eprintln!("client {client_addr}: downstream send failed: {error}");
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!("client {client_addr}: upstream receive failed: {error}");
                            break;
                        }
                    }
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(listen_addr: SocketAddr, server_addr: SocketAddr) -> ProxyConfig {
        ProxyConfig {
            listen_addr,
            server_addr,
            session_idle_timeout: Duration::from_secs(5),
            server_program: None,
            server_args: Vec::new(),
            startup_grace: Duration::ZERO,
            max_sessions: 128,
            session_queue_size: 16,
        }
    }

    #[tokio::test]
    async fn concurrent_clients_keep_independent_sessions() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buffer = [0_u8; 64];
            loop {
                let (length, peer) = server.recv_from(&mut buffer).await.unwrap();
                server.send_to(&buffer[..length], peer).await.unwrap();
            }
        });

        let listener = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let proxy_addr = listener.local_addr().unwrap();
        tokio::spawn(run_on_socket(
            listener,
            test_config(proxy_addr, server_addr),
        ));

        let first = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        first.send_to(b"first", proxy_addr).await.unwrap();
        second.send_to(b"second", proxy_addr).await.unwrap();

        let mut first_reply = [0_u8; 64];
        let mut second_reply = [0_u8; 64];
        let first_length = timeout(Duration::from_secs(1), first.recv(&mut first_reply))
            .await
            .unwrap()
            .unwrap();
        let second_length = timeout(Duration::from_secs(1), second.recv(&mut second_reply))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(&first_reply[..first_length], b"first");
        assert_eq!(&second_reply[..second_length], b"second");
        assert!(
            timeout(Duration::from_millis(50), first.recv(&mut first_reply))
                .await
                .is_err()
        );
        assert!(
            timeout(Duration::from_millis(50), second.recv(&mut second_reply))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn session_limit_rejects_excess_clients_without_affecting_existing_ones() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buffer = [0_u8; 64];
            loop {
                let (length, peer) = server.recv_from(&mut buffer).await.unwrap();
                server.send_to(&buffer[..length], peer).await.unwrap();
            }
        });

        let listener = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let proxy_addr = listener.local_addr().unwrap();
        let mut config = test_config(proxy_addr, server_addr);
        config.max_sessions = 1;
        tokio::spawn(run_on_socket(listener, config));

        let admitted = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let rejected = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        admitted.send_to(b"admitted", proxy_addr).await.unwrap();

        let mut reply = [0_u8; 64];
        let length = timeout(Duration::from_secs(1), admitted.recv(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&reply[..length], b"admitted");

        rejected.send_to(b"rejected", proxy_addr).await.unwrap();
        assert!(
            timeout(Duration::from_millis(100), rejected.recv(&mut reply))
                .await
                .is_err()
        );

        admitted.send_to(b"still-alive", proxy_addr).await.unwrap();
        let length = timeout(Duration::from_secs(1), admitted.recv(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&reply[..length], b"still-alive");
    }

    #[tokio::test]
    async fn server_start_failure_does_not_terminate_the_proxy() {
        let listener = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let proxy_addr = listener.local_addr().unwrap();
        let mut config = test_config(proxy_addr, "127.0.0.1:1".parse().unwrap());
        config.server_program = Some("this-program-does-not-exist-pal-proxy".to_owned());

        let proxy = tokio::spawn(run_on_socket(listener, config));
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.send_to(b"wake", proxy_addr).await.unwrap();
        sleep(Duration::from_millis(50)).await;

        assert!(!proxy.is_finished(), "a launch error stopped the proxy");
        proxy.abort();
    }

    #[test]
    fn invalid_resource_limits_are_rejected() {
        let mut config = test_config(
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:1".parse().unwrap(),
        );
        config.max_sessions = 0;
        assert_eq!(
            validate_config(&config).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        config.max_sessions = 1;
        config.session_queue_size = 0;
        assert_eq!(
            validate_config(&config).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
