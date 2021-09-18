use lazy_static::lazy_static;
use regex::Regex;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::broadcast::{Receiver, Sender};

lazy_static! {
    static ref TAG_PATTERN: Regex = Regex::new(r"^([^:]*): *(.*)$").unwrap();
}

/// A tag is a single line of communication on the AMI
///
/// It is similar to an entry in a map. It has a `key` and a `value`.
#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

/// A `Packet` is a sequence of `Tag`s being transmitted over the AMI, terminated by an empty line
pub type Packet = Vec<Tag>;

/// Establishes a connection to an asterisk server
///
/// # Arguments
///
/// * `server` - address of the asterisk server's AMI interface, e.g `127.0.0.1:5038`
/// * `cmd_rx` - receiver side of a channel that is used to send actions to Asterisk
/// * `resp_tx` - sender side of a channel used to send responses back to the caller
/// * `event_tx` - sender side of a channel used to send events to the caller
///
/// Returns `Ok(())` on success and `Err(_)` on failure.
pub async fn ami_connect<A: ToSocketAddrs>(
    server: A,
    mut cmd_rx: Receiver<Packet>,
    resp_tx: Sender<Vec<Packet>>,
    event_tx: Sender<Packet>,
) -> Result<(), Box<dyn Error>> {
    let socket = TcpStream::connect(server).await?;
    let mut reader = BufReader::new(socket);
    let mut greeting = String::new();
    reader.read_line(&mut greeting).await?;

    let mut response: Vec<Packet> = vec![];
    let mut in_packet: Packet = vec![];
    let mut line = String::new();
    let mut in_response_sequence = false;
    loop {
        tokio::select! {
            bytes_read = reader.read_line(&mut line) => {
                if bytes_read? == 0 {
                    break;
                }

                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    if !in_response_sequence && !in_packet.is_empty() && in_packet[0].key.eq_ignore_ascii_case("Event") {
                        event_tx.send(in_packet.clone())?;
                    } else {
                        response.push(in_packet.clone());
                        let event_list = find_tag(&in_packet, "EventList").map(|tag| &tag.value);
                        if event_list.filter(|el| el.eq_ignore_ascii_case("start")).is_some() {
                            in_response_sequence = true;
                        } else if event_list.filter(|el| el.eq_ignore_ascii_case("Complete")).is_some() {
                            in_response_sequence = false;
                        }
                        if !in_response_sequence {
                            resp_tx.send(response.clone())?;
                            response.clear();
                        }
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

/// Searches for a `Tag` within a packet
///
/// # Arguments
///
/// * `pkt` - The `Packet` to search in
/// * `key` - The key to search the `Tag` for
pub fn find_tag<'a>(pkt: &'a Packet, key: &str) -> Option<&'a Tag> {
    pkt.iter()
        .find(|&tag| tag.key.eq_ignore_ascii_case(key))
}

fn line_to_tag(line: &str) -> Option<Tag> {
    let caps = TAG_PATTERN.captures(line)?;
    Some(Tag { key: String::from(&caps[1]), value: String::from(&caps[2])})
}

fn packet_to_string(pkt: &Packet) -> String {
    pkt.iter()
        .map(|Tag { key, value }| format!("{}: {}", key, value))
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
