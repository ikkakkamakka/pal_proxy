//This Autostarts and stops the server when packets are received.
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::process::{Command, Child};
use std::collections::HashMap;
use std::sync::Arc;
use std::net::SocketAddr;
use std::error::Error;
use tokio::time::{self, Duration, Instant};

const CLIENT_PORT: &str = "0.0.0.0:8211";
const SERVER_ADDR: &str = "127.0.0.1:15575";
const TIMEOUT_SECONDS: u64 = 60; // Adjust the timeout as needed

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client_socket = Arc::new(UdpSocket::bind(CLIENT_PORT).await?);
    println!("Proxy listening for client packets on {}", CLIENT_PORT);

    let server_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    println!("Proxy will send to game server from {}", server_socket.local_addr()?);

    let server_addr: SocketAddr = SERVER_ADDR.parse()?;
    println!("Game server address set to {}", SERVER_ADDR);

    let client_mappings = Arc::new(Mutex::new(HashMap::<SocketAddr, SocketAddr>::new()));
    let server_running = Arc::new(Mutex::new(false));
    let server_process = Arc::new(Mutex::new(None::<Child>));
    let last_received = Arc::new(Mutex::new(Instant::now()));

    let client_socket_clone = client_socket.clone();
    let server_socket_clone = server_socket.clone();
    let server_addr_clone = server_addr.clone();
    let client_mappings_clone = client_mappings.clone();
    let server_running_clone = server_running.clone();
    let server_process_clone = server_process.clone();
    let last_received_clone = last_received.clone();

    // Client to Server
    tokio::spawn(async move {
        let mut buf = [0; 4096];
        loop {
            match client_socket_clone.recv_from(&mut buf).await {
                Ok((len, client_addr)) => {
                    *last_received_clone.lock().await = Instant::now();

                    // Start the server if not already running
					let mut server_running = server_running_clone.lock().await;
					if !*server_running {
						println!("Starting game server...");
						let child = Command::new("C:\\PATH\\TO\\PalServer\\PalServer.exe")
							.args(["-port=15575", "-useperfthreads", "-NoAsyncLoadingThread", "-UseMultithreadForDS", "-log"])
							.spawn()
							.expect("Failed to start server");
						*server_process_clone.lock().await = Some(child);
						*server_running = true;
					}


                    let data = &buf[..len];
                    client_mappings_clone.lock().await.insert(client_addr, client_addr);
                    if let Err(e) = server_socket_clone.send_to(data, &server_addr_clone).await {
                        eprintln!("Error forwarding to server: {}", e);
                    }
                },
                Err(e) => eprintln!("Error receiving from client: {}", e),
            }
        }
    });

    // Server to Client
    let server_to_client_socket = client_socket.clone();
    let server_to_client_mappings = client_mappings.clone();
    tokio::spawn(async move {
        let mut buf = [0; 4096];
        loop {
            match server_socket.recv_from(&mut buf).await {
                Ok((len, _)) => {
                    let data = &buf[..len];
                    let mappings = server_to_client_mappings.lock().await;
                    for &client_addr in mappings.values() {
                        if let Err(e) = server_to_client_socket.send_to(data, &client_addr).await {
                            eprintln!("Error forwarding to client {}: {}", client_addr, e);
                        }
                    }
                },
                Err(e) => eprintln!("Error receiving from game server: {}", e),
            }
        }
    });

	// Timeout check and server shutdown
	let last_received_check = last_received.clone();
	let server_running_check = server_running.clone();
	tokio::spawn(async move {
		loop {
			tokio::time::sleep(Duration::from_secs(TIMEOUT_SECONDS)).await;
			let last = *last_received_check.lock().await;
			if last.elapsed() >= Duration::from_secs(TIMEOUT_SECONDS) {
				// Timeout, stop the server if it's running
				if *server_running_check.lock().await {
					println!("Stopping game server due to inactivity...");
					
					// Use taskkill to terminate the main server executable directly
					let kill_result = Command::new("taskkill")
						.args(&["/F", "/IM", "PalServer-Win64-Test-Cmd.exe"])
						.output()
						.await;

					match kill_result {
						Ok(output) => {
							if output.status.success() {
								println!("Game server has been stopped.");
							} else {
								let error_message = String::from_utf8_lossy(&output.stderr);
								eprintln!("Failed to kill the game server process: {}", error_message);
							}
						},
						Err(e) => eprintln!("Failed to execute taskkill command: {}", e),
					}

					*server_running_check.lock().await = false;
				}
			}
		}
	});



    // Wait for a ctrl-c event for graceful shutdown
    tokio::signal::ctrl_c().await?;
    println!("Shutdown signal received, terminating proxy...");

    Ok(())
}
