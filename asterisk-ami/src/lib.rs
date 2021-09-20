use response::{Response, ResponseBuilder};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::{oneshot, watch, mpsc};

mod response;

/// A tag is a single line of communication on the AMI
///
/// It is similar to an entry in a map. It has a `key` and a `value`.
#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

impl Tag {
    pub fn of(key: String, value: String) -> Self {
        Self { key, value }
    }

    pub fn from(key: &str, value: &str) -> Self {
        Self { key: key.to_string(), value: value.to_string() }
    }
}

/// A `Packet` is a sequence of `Tag`s being transmitted over the AMI, terminated by an empty line
pub type Packet = Vec<Tag>;

/// A `Responder` is used to send back the result of a `Command`
pub type Responder<T> = oneshot::Sender<T>;

/// A `Command` can be sent to the Asterisk server, the response will be send back to the
/// caller over the specified `Responder` in the `resp` field.
#[derive(Debug)]
struct Command {
    packet: Packet,
    resp: Responder<Vec<Packet>>,
}

pub struct AmiConnection {
    cmd_tx: mpsc::Sender<Command>,
    events_rx: watch::Receiver<Option<Packet>>,
}

impl AmiConnection {
    /// Establishes a connection to an asterisk server
    ///
    /// # Arguments
    ///
    /// * `server` - address of the asterisk server's AMI interface, e.g `127.0.0.1:5038`
    pub async fn connect<A: ToSocketAddrs>(
        server: A,
    ) -> Result<AmiConnection, std::io::Error> {
        let socket = TcpStream::connect(server).await?;
        let mut reader = BufReader::new(socket);
        let mut greeting = String::new();
        reader.read_line(&mut greeting).await?;

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(32);
        let (events_tx, events_rx) = watch::channel::<Option<Packet>>(None);

        tokio::spawn(async move {
            let mut current_command: Option<Command> = None;
            let mut response_builder = ResponseBuilder::new();
            let mut line = String::new();
            let mut maybe_response: Option<Response> = None;
            loop {
                if current_command.is_none() {
                    tokio::select! {
                        bytes_read = reader.read_line(&mut line) => {
                            if bytes_read.unwrap() == 0 {
                                break;
                            }
                            maybe_response = response_builder.add_line(line.trim());
                        }

                        cmd = cmd_rx.recv() => {
                            if let Some(c) = cmd {
                                let chunk = format!("{}\r\n\r\n", packet_to_string(&c.packet));
                                current_command = Some(c);
                                reader.write_all(chunk.as_bytes()).await.unwrap();
                            }
                        }
                    }
                } else {
                    tokio::select! {
                        bytes_read = reader.read_line(&mut line) => {
                            if bytes_read.unwrap() == 0 {
                                break;
                            }

                            maybe_response = response_builder.add_line(line.trim());
                        }
                    }
                }

                if let Some(resp) = maybe_response {
                    match resp {
                        Response::Event(pkt) => {
                            events_tx.send(Some(pkt)).unwrap();
                        }
                        Response::CommandResponse(cr) => {
                            if let Some(cmd) = current_command {
                                cmd.resp.send(cr).unwrap();
                            }
                            current_command = None;
                        }
                    }
                }
                maybe_response = None;
                line.clear();
            }
        });

        Ok(AmiConnection { cmd_tx, events_rx })
    }

    pub async fn send(&self, packet: Packet) -> Option<Vec<Packet>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command { packet, resp: tx })
            .await
            .unwrap();
        let result = rx.await.ok();
        result
    }

    pub fn events(&self) -> watch::Receiver<Option<Packet>> {
        self.events_rx.clone()
    }
}

/// Searches for a `Tag` within a packet
///
/// # Arguments
///
/// * `pkt` - The `Packet` to search in
/// * `key` - The key to search the `Tag` for
pub fn find_tag<'a>(pkt: &'a Packet, key: &str) -> Option<&'a String> {
    pkt.iter()
        .find(|&tag| tag.key.eq_ignore_ascii_case(key))
        .map(|t| &t.value)
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
