mod socks;

use clap::{arg, Parser};
use std::net::Ipv6Addr;
use std::process;
use std::str::FromStr;
use tokio::net::{TcpListener, UdpSocket};

#[derive(Parser)]
#[command(version)]
pub struct Args {
    #[arg(
        default_value = "0.0.0.0",
        help = "Specify bind address",
        long,
        short,
        value_name = "ADDRESS"
    )]
    pub bind: String,

    #[arg(default_value_t = 1080, help = "Specify bind port")]
    pub port: u16,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::init();

    let args = Args::parse();
    let bind = if let Ok(addr) = Ipv6Addr::from_str(&args.bind) {
        format!("[{addr}]:{}", args.port)
    } else {
        format!("{}:{}", args.bind, args.port)
    };

    let tcp_listener = TcpListener::bind(&bind).await.unwrap_or_else(|err| {
        eprintln!("error: failed to bind to tcp://{bind}: {err}");
        process::exit(1);
    });
    let udp_socket = UdpSocket::bind(&bind).await.unwrap_or_else(|err| {
        eprintln!("error: failed to bind to udp://{bind}: {err}");
        process::exit(1);
    });
    println!("Serving SOCKS on {bind}");

    socks::start_socks_server(tcp_listener, udp_socket).await;
}
