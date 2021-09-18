use asterisk_ami::{ami_connect, Tag};
use clap::{clap_app, crate_version};
use std::error::Error;
use std::net::SocketAddr;
use tokio::io;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = clap_app!(
        myapp =>
            (name: "asterisk-ami-example")
            (version: crate_version!())
            (author: "Matthias Wimmer <m@tthias.eu>")
            (about: "Example application for the asterisk-ami crate")
            (@arg SERVER: -s --server +takes_value "Server to connect to")
            (@arg USER: -u --user +takes_value "Username to authenticate with")
            (@arg PASS: -p --pass +takes_value "Password to authenticate with")
            (@arg EVENTS: -e --events "Show all incoming events")
    )
    .get_matches();

    let all_events = args.is_present("EVENTS");

    let username = args
        .value_of("USER")
        .map(String::from)
        .or(dotenv::var("USERNAME").ok())
        .expect("No username given");
    let secret = args
        .value_of("PASS")
        .map(String::from)
        .or(dotenv::var("SECRET").ok())
        .expect("No password given");
    let server = args
        .value_of("SERVER")
        .map(String::from)
        .or(dotenv::var("SERVER").ok())
        .unwrap_or(String::from("127.0.0.1:5038"));
    let server_address: SocketAddr = server.parse()?;

    let mut stdin_reader = BufReader::new(io::stdin());

    let (cmd_tx, cmd_rx) = broadcast::channel(16);
    let (resp_tx, mut resp_rx) = broadcast::channel(16);
    let (event_tx, mut event_rx) = broadcast::channel(16);

    tokio::spawn(async move {
        ami_connect(server_address, cmd_rx, resp_tx, event_tx)
            .await
            .expect("Connection did return with error");
    });

    let login = vec![
        Tag {
            key: String::from("Action"),
            value: String::from("Login"),
        },
        Tag {
            key: String::from("Username"),
            value: username,
        },
        Tag {
            key: String::from("Secret"),
            value: secret,
        },
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
                    let pkt = vec![Tag { key: String::from("Action"), value: String::from(cmd)}];
                    cmd_tx.send(pkt)?;
                }
                line_buffer.clear();
            }

            response = resp_rx.recv() => {
                println!("{:?}", response?);
            }

            event = event_rx.recv() => {
                if all_events {
                    println!("{:?}", event?);
                }
            }
        }
    }

    Ok(())
}
