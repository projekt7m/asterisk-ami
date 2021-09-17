use lazy_static::lazy_static;
use regex::Regex;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::io;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};

lazy_static! {
    static ref TAG_PATTERN: Regex = Regex::new(r"^([^:]*): *(.*)$").unwrap();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let username = dotenv::var("USERNAME").unwrap();
    let secret = dotenv::var("SECRET").unwrap();

    let mut stdin_reader = BufReader::new(io::stdin());

    let (cmd_tx, cmd_rx) = broadcast::channel(16);
    let (resp_tx, mut resp_rx) = broadcast::channel(16);
    let (event_tx, mut event_rx) = broadcast::channel(16);

    tokio::spawn(async move {
        ami_connect("127.0.0.1:5038", cmd_rx, resp_tx, event_tx)
            .await
            .expect("Connection did return with error");
    });

    let login = vec![
        (String::from("Action"), String::from("Login")),
        (String::from("Username"), username),
        (String::from("Secret"), secret),
    ];
    cmd_tx.send(login)?;

    let mut line_buffer = String::new();
    loop {
        tokio::select! {
            bytes_read = stdin_reader.read_line(&mut line_buffer) => {
                if bytes_read? == 0 {
                    break;
                }

                let cmd = line_buffer.trim();
                if cmd == "" {
                    break;
                } else {
                    let pkt = vec![(String::from("Action"), String::from(cmd))];
                    cmd_tx.send(pkt)?;
                }
                line_buffer.clear();
            }

            resp = resp_rx.recv() => {
                println!("Response: {:?}", resp?);
            }

            evt = event_rx.recv() => {
                println!("Event: {:?}", evt?);
            }
        }
    }

    Ok(())
}

async fn ami_connect(
    server: &str,
    mut cmd_rx: Receiver<Vec<(String, String)>>,
    resp_tx: Sender<Vec<(String, String)>>,
    event_tx: Sender<Vec<(String, String)>>,
) -> Result<(), Box<dyn Error>> {
    let socket = TcpStream::connect(server).await?;
    let mut reader = BufReader::new(socket);
    let mut greeting = String::new();
    reader.read_line(&mut greeting).await?;

    let mut in_packet: Vec<(String, String)> = vec![];
    let mut line = String::new();
    loop {
        tokio::select! {
            bytes_read = reader.read_line(&mut line) => {
                if bytes_read? == 0 {
                    break;
                }

                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    if !in_packet.is_empty() && in_packet[0].0 == "Event" {
                        event_tx.send(in_packet.clone())?;
                    } else {
                        resp_tx.send(in_packet.clone())?;
                    }
                    in_packet.clear()
                } else {
                    if let Some(tag) = line_to_tag(trimmed_line) {
                        in_packet.push(tag);
                    }
                }
                line.clear();
            }

            pkt = cmd_rx.recv() => {
                let cmd = packet_to_string(&pkt?);
                let chunk = format!("{}\r\n\r\n", cmd);
                reader.write_all(chunk.as_bytes()).await?;
            }
        }
    }
    Ok(())
}

fn line_to_tag(line: &str) -> Option<(String, String)> {
    let caps = TAG_PATTERN.captures(line)?;
    Some((String::from(&caps[1]), String::from(&caps[2])))
}

fn packet_to_string(pkt: &[(String, String)]) -> String {
    pkt.iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<String>>()
        .join("\r\n")
}
