mod socks;

use clap::Parser;
use std::process;
use tokio::net::{TcpListener, UdpSocket};

#[derive(Parser)]
pub struct Args {
    #[arg(
        help = "specify bind address [default: 0.0.0.0]",
        long = "bind",
        short = 'b'
    )]
    pub address: Option<String>,

    #[arg(help = "specify bind port", default_value_t = 1080)]
    pub port: u16,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();

    let args = Args::parse();
    let addr = format!(
        "{}:{}",
        args.address.unwrap_or_else(|| String::from("0.0.0.0")),
        args.port
    );

    let tcp_listener = TcpListener::bind(&addr).await.unwrap_or_else(|err| {
        eprintln!("error: failed to bind to tcp://{addr}: {err}");
        process::exit(1);
    });
    let udp_socket = UdpSocket::bind(&addr).await.unwrap_or_else(|err| {
        eprintln!("error: failed to bind to udp://{addr}: {err}");
        process::exit(1);
    });
    println!("Serving SOCKS on {}", addr);

    socks::start_socks_server(tcp_listener, udp_socket).await;
}
