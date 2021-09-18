use asterisk_ami::ami_connect;
use clap::{clap_app, crate_version};
use std::error::Error;
use tokio::io;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use std::net::SocketAddr;

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
    let mut in_response = false;

    let username = args.value_of("USER").map(String::from).or(dotenv::var("USERNAME").ok()).expect("No username given");
    let secret = args.value_of("PASS").map(String::from).or(dotenv::var("SECRET").ok()).expect("No password given");
    let server = args.value_of("SERVER").map(String::from).or(dotenv::var("SERVER").ok()).unwrap_or(String::from("127.0.0.1:5038"));
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
                let response = resp?;
                println!("| {:?}", response);
                if response.len() >= 2 && response[1].0 == "EventList" && response[1].1 == "start" {
                    in_response = true;
                }
            }

            evt = event_rx.recv() => {
                let event = evt?;
                if all_events || in_response {
                    println!("| {:?}", event);
                }
                if event.len() >= 2 && event[1].0 == "EventList" && event[1].1 == "Complete" {
                    in_response = false;
                }
            }
        }
    }

    Ok(())
}
