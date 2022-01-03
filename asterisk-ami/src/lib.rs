use log::{trace, warn};
use response::{Response, ResponseBuilder};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{broadcast, mpsc, oneshot};

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
        Self {
            key: key.to_string(),
            value: value.to_string(),
        }
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
    events_tx: broadcast::Sender<Option<Packet>>,
}

impl AmiConnection {
    /// Establishes a connection to an asterisk server
    ///
    /// # Arguments
    ///
    /// * `server` - address of the asterisk server's AMI interface, e.g `127.0.0.1:5038`
    pub async fn connect<A: ToSocketAddrs + std::fmt::Debug>(
        server: A,
    ) -> Result<AmiConnection, std::io::Error> {
        let reader = Self::connect_to_server(server).await?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(32);
        let (events_tx, _) = broadcast::channel::<Option<Packet>>(32);

        let events_tx2 = events_tx.clone();

        tokio::spawn(async move {
            Self::handle_server_connection(reader, cmd_rx, events_tx2).await;
        });

        Ok(AmiConnection { cmd_tx, events_tx })
    }

    async fn handle_server_connection(
        mut server_connection: BufReader<TcpStream>,
        mut command_channel_rx: Receiver<Command>,
        event_channel_tx: Sender<Option<Packet>>,
    ) {
        let mut current_command: Option<Command> = None;
        let mut response_builder = ResponseBuilder::new();
        let mut line = String::new();
        let mut maybe_response: Option<Response> = None;
        loop {
            if current_command.is_none() {
                tokio::select! {
                    bytes_read = server_connection.read_line(&mut line) => {
                        match bytes_read {
                            Err(e) => {
                                warn!("Error reading from server connection: {:?}", e);
                                break;
                            }
                            Ok(0) => {
                                trace!("Server connection closed");
                                break;
                            }
                            Ok(_) => {
                                maybe_response = response_builder.add_line(line.trim());
                            }
                        }
                    }

                    cmd = command_channel_rx.recv() => {
                        if let Some(c) = cmd {
                            let chunk = format!("{}\r\n\r\n", packet_to_string(&c.packet));
                            current_command = Some(c);
                            if let Err(e) = server_connection.write_all(chunk.as_bytes()).await {
                                warn!("Error writing to server connection: {:?}", e);
                                break;
                            }
                        }
                    }
                }
            } else {
                tokio::select! {
                    bytes_read = server_connection.read_line(&mut line) => {
                        match bytes_read {
                            Err(e) => {
                                warn!("Error reading from server connection: {:?}", e);
                                break;
                            }
                            Ok(0) => {
                                trace!("Server connection closed");
                                break;
                            }
                            Ok(_) => {
                                maybe_response = response_builder.add_line(line.trim());
                            }
                        }
                    }
                }
            }

            if let Some(resp) = maybe_response {
                match resp {
                    Response::Event(pkt) => {
                        if !Self::publish_event(&event_channel_tx, Some(pkt)) {
                            break;
                        }
                    }
                    Response::CommandResponse(cr) => {
                        if let Some(cmd) = current_command {
                            current_command = None;
                            if let Err(e) = cmd.resp.send(cr) {
                                warn!(
                                    "Cannot send command response back: {:?}",
                                    e
                                );
                                break;
                            }
                        }
                    }
                }
            }
            maybe_response = None;
            line.clear();
        }

        Self::publish_event(&event_channel_tx, None);
        command_channel_rx.close();
        if let Some(cmd) = current_command {
            if let Err(e) = cmd.resp.send(vec![]) {
                warn!("Cannot terminate current command on close: {:?}", e);
            }
        }
    }

    fn publish_event(
        event_channel_tx: &Sender<Option<Packet>>,
        pkt: Option<Packet>,
    ) -> bool {
        if event_channel_tx.receiver_count() > 0 {
            if let Err(e) = event_channel_tx.send(pkt) {
                warn!("Could not send event to subscribers: {:?}", e);
                return false;
            }
        }
        true
    }

    async fn connect_to_server<A: ToSocketAddrs + std::fmt::Debug>(
        server: A,
    ) -> Result<BufReader<TcpStream>, std::io::Error> {
        trace!("Connecting to {:?}", server);
        let mut reader = BufReader::new(TcpStream::connect(server).await?);
        Self::read_greeting(&mut reader).await?;
        Ok(reader)
    }

    async fn read_greeting(
        reader: &mut BufReader<TcpStream>,
    ) -> Result<(), std::io::Error> {
        let mut greeting = String::new();
        reader.read_line(&mut greeting).await?;

        Ok(())
    }

    /// Send a command to the Asterisk server using AMI
    ///
    /// # Arguments
    ///
    /// * `pkt` - The `Packet` to send to the server
    ///
    /// # Return value
    ///
    /// Returns `Some(packets)` on success. `None` signales an error and that the connection
    /// should be reestablished.
    pub async fn send(&self, pkt: Packet) -> Option<Vec<Packet>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command {
                packet: pkt,
                resp: tx,
            })
            .await
            .ok()?;
        rx.await.ok()
    }

    pub fn events(&self) -> broadcast::Receiver<Option<Packet>> {
        self.events_tx.subscribe()
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
