use lazy_static::lazy_static;
use regex::Regex;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};

lazy_static! {
    static ref TAG_PATTERN: Regex = Regex::new(r"^([^:]*): *(.*)$").unwrap();
}

pub async fn ami_connect(
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
