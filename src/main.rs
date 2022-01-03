use asterisk_ami::{AmiConnection, Tag};
use clap::{clap_app, crate_version};
use simple_logger::SimpleLogger;
use std::error::Error;
use std::net::SocketAddr;
use log::{error, info, trace, warn};
use tokio::io;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_utc_timestamps()
        .with_colors(true)
        .init()
        .unwrap();

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

    'outer: loop {
        let ami_connection = AmiConnection::connect(server_address).await?;

        if all_events {
            let mut events = ami_connection.events();
            tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Err(e) => warn!("Error on reading event: {:?}", e),
                        Ok(Some(evt)) => info!("Event: {:?}", evt),
                        Ok(None) => {
                            trace!("Connection closed.");
                            continue;
                        }
                    }
                }
            });
        }

        let login = vec![
            Tag::from("Action", "Login"),
            Tag::from("Username", &username),
            Tag::from("Secret", &secret),
        ];
        match ami_connection.send(login).await {
            Some(resp) => info!("Login Response: {:?}", resp),
            None => {
                error!(
                    "Error on logging in ... maybe cannot connect to server?"
                );
                break;
            }
        }

        let mut line_buffer = String::new();
        loop {
            tokio::select! {
                bytes_read = stdin_reader.read_line(&mut line_buffer) => {
                    if bytes_read? == 0 {
                        trace!("Stdin closed");
                        break 'outer;
                    }

                    let cmd = line_buffer.trim();
                    if cmd == "" {
                        trace!("Good Bye");
                        break 'outer;
                    } else {
                        let pkt = vec![Tag::from("Action", cmd)];
                        match ami_connection.send(pkt).await {
                            Some(resp) => info!("Response: {:?}", resp),
                            None => {
                                info!("No response. Connection probably closed.");
                                break;
                            },
                        }
                    }
                    line_buffer.clear();
                }
            }
        }
    }

    Ok(())
}
