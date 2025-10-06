
use std::net::UdpSocket;
use std::net::{Ipv4Addr, SocketAddrV4};
use socket2::{Domain, Protocol, Socket, Type};

pub fn run() -> std::io::Result<()> {
    let multicast_addr = "239.255.42.99".parse::<Ipv4Addr>().unwrap();
    let port = 9001;
    let socket_addr = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), port);

    // Create a UDP socket.
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

    // Set SO_REUSEADDR.
    socket.set_reuse_address(true)?;

    // Bind the socket to the address and port.
    socket.bind(&socket_addr.into())?;

    // Join the multicast group.
    socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::new(0, 0, 0, 0))?;

    println!("Listening on {}:{}", multicast_addr, port);

    let mut buf = [0u8; 65536];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((size, src)) => {
                println!("Received {} bytes from {}", size, src);
                // For now, just print the raw bytes as a string.
                // In the future, we'll parse the packet here.
                println!("Data: {}", String::from_utf8_lossy(&buf[..size]));
            }
            Err(e) => {
                eprintln!("Error receiving data: {}", e);
                break;
            }
        }
    }

    Ok(())
}
