//! Test raw TCP communication

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn create_tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });

    let (server, _) = listener.accept().await.unwrap();
    let client = client_task.await.unwrap();

    (client, server)
}

#[tokio::test]
async fn test_raw_tcp() {
    println!("Creating TCP pair...");
    let (mut client, mut server) = create_tcp_pair().await;

    println!("Spawning server read task...");
    let server_task = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        let n = server.read(&mut buf).await.unwrap();
        println!("Server received {} bytes", n);
        let received = String::from_utf8_lossy(&buf[..n]);
        println!("Server received: {}", received);
        received.to_string()
    });

    println!("Client writing data...");
    client.write_all(b"Hello from client").await.unwrap();
    client.flush().await.unwrap();
    println!("Client wrote data");

    println!("Waiting for server...");
    let received = server_task.await.unwrap();
    assert_eq!(received, "Hello from client");
    println!("Test passed!");
}
