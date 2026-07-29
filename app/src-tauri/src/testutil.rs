//! Test-only helpers. Compiled out of every real build.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A one-shot HTTP server on loopback, for exercising real transfers without
/// reaching the internet.
///
/// A suite that talks to `maven.minecraftforge.net` goes red when the network
/// does, and then stops telling you anything about the code under test.
/// Loopback keeps the request real and the outcome deterministic.
///
/// `chunk_delay_ms` paces the body in 1 KB pieces, which is what makes
/// mid-transfer behaviour (progress updates, cancellation) observable.
pub async fn serve(status_line: &'static str, body: Vec<u8>, chunk_delay_ms: u64) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();

        // Drain the request line and headers.
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while socket.read_exact(&mut byte).await.is_ok() {
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }

        let header = format!(
            "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        if socket.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        for piece in body.chunks(1024) {
            if socket.write_all(piece).await.is_err() {
                return;
            }
            if chunk_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(chunk_delay_ms)).await;
            }
        }
        let _ = socket.flush().await;
    });

    addr
}
