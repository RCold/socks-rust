# socks-rust

A minimal SOCKS server implementation written in Rust

## Features

- No unsafe code
- Ultra lightweight
- Cross-platform
- SOCKS4 is supported
- SOCKS4a is supported
- SOCKS5 no-auth method (`0x00`) is supported
- SOCKS5 connect is supported
- SOCKS5 UDP associate is supported

## Running

```bash
RUST_LOG=debug cargo run -- --bind 127.0.0.1 1080
```

## Important Notes

This SOCKS server does not implement any authentication methods. Anyone connecting to this server has unrestricted access to your network. You should only use this server within a trusted private network (home LAN, VPN, etc.) or behind a firewall.

## License

Licensed under Apache License Version 2.0 ([LICENSE](LICENSE) or https://www.apache.org/licenses/LICENSE-2.0)
