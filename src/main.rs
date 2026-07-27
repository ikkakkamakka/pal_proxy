use std::env;
use std::error::Error;
use std::net::SocketAddr;
use tokio::time::Duration;

use pal_proxy::{run, ProxyConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listen_addr: SocketAddr = env_var("PAL_PROXY_LISTEN", "0.0.0.0:8211").parse()?;
    let server_addr: SocketAddr = env_var("PAL_SERVER_ADDR", "127.0.0.1:15575").parse()?;
    let session_idle_timeout =
        Duration::from_secs(env_var("PAL_SESSION_IDLE_SECONDS", "300").parse()?);
    let startup_grace = Duration::from_secs(env_var("PAL_SERVER_STARTUP_SECONDS", "30").parse()?);
    let server_program = env::var("PAL_SERVER_PROGRAM").ok();
    let server_args = env::var("PAL_SERVER_ARGS")
        .map(|args| args.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    let max_sessions = env_var("PAL_MAX_SESSIONS", "128").parse()?;
    let session_queue_size = env_var("PAL_SESSION_QUEUE_SIZE", "256").parse()?;

    println!("Pal Proxy listening on {listen_addr}, forwarding to {server_addr}");
    if server_program.is_some() {
        println!("The first datagram will start the server and wait {startup_grace:?}");
    }

    let proxy = run(ProxyConfig {
        listen_addr,
        server_addr,
        session_idle_timeout,
        server_program,
        server_args,
        startup_grace,
        max_sessions,
        session_queue_size,
    });

    tokio::select! {
        result = proxy => result?,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            println!("shutdown signal received");
        }
    }
    Ok(())
}

fn env_var(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}
