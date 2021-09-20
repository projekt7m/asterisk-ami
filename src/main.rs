use asterisk_ami::{AmiConnection, Tag};
use clap::{clap_app, crate_version};
use std::error::Error;
use std::net::SocketAddr;
use tokio::io;
use tokio::io::{AsyncBufReadExt, BufReader};

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

    let ami_connection = AmiConnection::connect(server_address).await?;

    if all_events {
        let mut events = ami_connection.events();
        tokio::spawn(async move {
            loop {
                events.changed().await.unwrap();
                println!("Event: {:?}", *events.borrow());
            }
        });
    }

    let login = vec![
        Tag::from("Action", "Login"),
        Tag::from("Username", &username),
        Tag::from("Secret", &secret),
    ];
    let login_response = ami_connection.send(login).await.unwrap();
    println!("Login Response: {:?}", login_response);

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
                    let pkt = vec![Tag::from("Action", cmd)];
                    let response = ami_connection.send(pkt).await.unwrap();
                    println!("Response: {:?}", response);
                }
                line_buffer.clear();
            }
        }
    }

    Ok(())
}
