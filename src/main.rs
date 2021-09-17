use asterisk_ami::ami_connect;
use std::error::Error;
use tokio::io;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;

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
