use pprof;
use std::error::Error;
use tokio;
use tokio::io::{AsyncWriteExt, Interest};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, Receiver, Sender};
// declear a wrap type
type IResult<T> = Result<T, Box<dyn Error>>;

#[tokio::main]
async fn main() -> IResult<()> {
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .unwrap();
    // bind a port for server
    let listener = TcpListener::bind("0.0.0.0:6601").await?;

    if let Ok(report) = guard.report().build() {
        let file = std::fs::File::create("flamegraph.svg").unwrap();
        report.flamegraph(file).unwrap();
    };
    // receive client connect
    loop {
        let (c, addr) = listener.accept().await.unwrap();
        println!("got a connection from: {}", addr);
        // use tokio spawn handle client connection
        tokio::spawn(async move {
            handle_connection(c).await;
        });
        tokio::signal::ctrl_c().await.unwrap();
    }
}

// this will spawn two Future task to handle read and write
// and use channel sync data to two Future task
async fn handle_connection(c: TcpStream) {
    // declar a buf
    let mut buf = [0u8; 1024];
    // make a channel for two Future
    let (tx, mut rx): (Sender<String>, Receiver<String>) = mpsc::channel(100);
    // split the connection stream to reader and writer
    let (r, mut w) = c.into_split();

    // spawn a new Future to handle read
    tokio::spawn(async move {
        loop {
            let read_ready = match r.ready(Interest::READABLE).await {
                Ok(r) => r,
                Err(e) => {
                    println!("socket listen event error: {}", e);
                    return;
                }
            };

            if read_ready.is_readable() {
                match r.try_read(&mut buf) {
                    Ok(n) if n == 0 => {
                        eprintln!("connection closed");
                        return;
                    }
                    Ok(n) => {
                        let s = String::from_utf8_lossy(&buf[0..n]).to_string();
                        println!("Receive Data: {:?}", s);

                        if let Err(e) = tx.send(s).await {
                            eprintln!("failed to send to channel; err = {:?}", e);
                        }
                    }
                    Err(ref e) if e.kind() == tokio::io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        eprintln!("failed to read from socket; err = {:?}", e);
                        return;
                    }
                };
            }
        }
    });

    // spawn a new Future to handle data from channel. write it to socket
    tokio::spawn(async move {
        loop {
            // receive data from channel and destrucet it
            if let Some(data) = rx.recv().await {
                // write data back to socket
                if let Err(e) = w.write_all(data.as_bytes()).await {
                    eprintln!("failed to write to socket; err = {:?}", e);
                    return;
                }
            }
        }
    });
}
