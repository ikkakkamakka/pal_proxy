use tokio::net::UdpSocket;
use std::collections::HashMap;
use tokio::sync::Mutex;
use std::error::Error;
use std::sync::Arc;
use std::net::SocketAddr;

const CLIENT_PORT: &str = "0.0.0.0:8211"; // The port on which clients will send their packets
const SERVER_ADDR: &str = "127.0.0.1:15575"; // The game server address

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client_socket = Arc::new(UdpSocket::bind(CLIENT_PORT).await?);
    println!("Proxy listening for client packets on {}", CLIENT_PORT);

    let server_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    println!("Proxy will send to game server from {}", server_socket.local_addr()?);

    let server_addr: SocketAddr = SERVER_ADDR.parse()?;
    println!("Game server address set to {}", SERVER_ADDR);

    let client_mappings = Arc::new(Mutex::new(HashMap::<SocketAddr, SocketAddr>::new()));

    let client_socket_clone_for_server = client_socket.clone();
    let server_socket_clone_for_server = server_socket.clone();
    let client_mappings_clone_for_server = client_mappings.clone();
    tokio::spawn(async move {
        let mut buf = [0; 4096];
        loop {
            match client_socket_clone_for_server.recv_from(&mut buf).await {
                Ok((len, client_addr)) => {
                    println!("Received {} bytes from client {}", len, client_addr);
                    let data = &buf[..len];
                    client_mappings_clone_for_server.lock().await.insert(client_addr, client_addr);
                    match server_socket_clone_for_server.send_to(data, &server_addr).await {
                        Ok(_) => println!("Forwarded {} bytes to game server {}", len, server_addr),
                        Err(e) => eprintln!("Error forwarding to server: {}", e),
                    }
                },
                Err(e) => eprintln!("Error receiving from client: {}", e),
            }
        }
    });

    let client_socket_clone_for_client = client_socket.clone();
    let client_mappings_clone_for_client = client_mappings.clone();
    tokio::spawn(async move {
        let mut buf = [0; 4096];
        loop {
            match server_socket.recv_from(&mut buf).await {
                Ok((len, _)) => {
                    println!("Received {} bytes from game server", len);
                    let data = &buf[..len];
                    let mappings = client_mappings_clone_for_client.lock().await;
                    for &client_addr in mappings.values() {
                        match client_socket_clone_for_client.send_to(data, &client_addr).await {
                            Ok(_) => println!("Forwarded {} bytes to client {}", len, client_addr),
                            Err(e) => eprintln!("Error forwarding to client {}: {}", client_addr, e),
                        }
                    }
                },
                Err(e) => eprintln!("Error receiving from game server: {}", e),
            }
        }
    });

    // Wait for a ctrl-c event for graceful shutdown
    tokio::signal::ctrl_c().await?;
    println!("Shutdown signal received, terminating proxy...");

    Ok(())
}
