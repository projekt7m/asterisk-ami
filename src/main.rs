use lazy_static::lazy_static;
use regex::Regex;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

lazy_static! {
    static ref TAG_PATTERN: Regex = Regex::new(r"^([^:]*): *(.*)$").unwrap();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let username = dotenv::var("USERNAME").unwrap();
    let secret = dotenv::var("SECRET").unwrap();
    asterisk_ami("127.0.0.1:5038", &username, &secret, |ev| {
        println!("Got Event: {:?}", ev)
    })
    .await?;
    Ok(())
}

async fn asterisk_ami<H>(
    server: &str,
    username: &str,
    secret: &str,
    event_handler: H,
) -> Result<(), Box<dyn Error>>
where
    H: Fn(Vec<(String, String)>),
{
    let socket = TcpStream::connect(server).await?;
    let mut reader = BufReader::new(socket);
    let mut greeting = String::new();
    reader.read_line(&mut greeting).await?;

    print!("Server greeting: {}", greeting);

    let login_action = format!(
        "Action: Login\r\nUsername:{}\r\nSecret:{}\r\n\r\n",
        username, secret
    );
    reader.write_all(login_action.as_bytes()).await?;

    let mut in_packet: Vec<(String, String)> = vec![];
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
            if !in_packet.is_empty() && in_packet[0].0 == "Event" {
                event_handler(in_packet.clone());
            } else {
                println!("Got Packet: {:?}", in_packet);
            }
            in_packet.clear()
        } else {
            if let Some(tag) = line_to_tag(trimmed_line) {
                in_packet.push(tag);
            }
        }
    }
    Ok(())
}

fn line_to_tag(line: &str) -> Option<(String, String)> {
    let caps = TAG_PATTERN.captures(line)?;
    Some((String::from(&caps[1]), String::from(&caps[2])))
}
