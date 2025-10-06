use std::net::{IpAddr, Ipv4Addr};

/// Get the primary network interface IP address
/// Returns the first non-loopback IPv4 address found
pub fn get_primary_interface_ip() -> Option<Ipv4Addr> {
    // Try to get local IP addresses
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        // Connect to a public DNS server (doesn't actually send data)
        // This trick helps us find the primary network interface
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                if let IpAddr::V4(ipv4) = addr.ip() {
                    if !ipv4.is_loopback() {
                        println!("Detected primary network interface: {}", ipv4);
                        return Some(ipv4);
                    }
                }
            }
        }
    }
    
    println!("Could not detect primary network interface, using 0.0.0.0");
    None
}