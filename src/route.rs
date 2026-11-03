use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    NetworkManagement::IpHelper::*,
    Networking::WinSock::{AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6, SOCKADDR_IN6_0},
};

pub const MAX_HOPS: u8 = 30;
const TIMEOUT_MS: u32 = 850;
const PAYLOAD: [u8; 16] = *b"Barracuda route!";

pub struct Hop {
    pub number: u8,
    pub reply: Option<(IpAddr, u32)>,
}

pub enum Finish {
    Reached,
    Limit,
    Cancelled,
    Failed(u32),
}

pub enum Event {
    Hop(Hop),
    Done(Finish),
}

struct Icmp(HANDLE);

impl Icmp {
    fn open(ip: IpAddr) -> io::Result<Self> {
        let handle = unsafe {
            if ip.is_ipv4() {
                IcmpCreateFile()
            } else {
                Icmp6CreateFile()
            }
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
}

impl Drop for Icmp {
    fn drop(&mut self) {
        unsafe { IcmpCloseHandle(self.0) };
    }
}

pub fn trace(
    local: SocketAddr,
    remote: SocketAddr,
    cancel: &AtomicBool,
    mut hop: impl FnMut(Hop),
) -> Finish {
    let local = canonical_endpoint(local);
    let remote = canonical_endpoint(remote);
    let handle = match Icmp::open(remote.ip()) {
        Ok(handle) => handle,
        Err(error) => return Finish::Failed(error.raw_os_error().unwrap_or_default() as u32),
    };
    for number in 1..=MAX_HOPS {
        if cancel.load(Ordering::Relaxed) {
            return Finish::Cancelled;
        }
        let options = IP_OPTION_INFORMATION {
            Ttl: number,
            ..Default::default()
        };
        let (reply, status) = probe(&handle, local, remote, &options);
        if cancel.load(Ordering::Relaxed) {
            return Finish::Cancelled;
        }
        hop(Hop { number, reply });
        match status {
            IP_SUCCESS if reply.is_some_and(|(ip, _)| ip == remote.ip()) => return Finish::Reached,
            IP_SUCCESS | IP_TTL_EXPIRED_TRANSIT | IP_REQ_TIMED_OUT => {}
            other => return Finish::Failed(other),
        }
    }
    Finish::Limit
}

fn canonical_endpoint(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V6(v6) if v6.ip().to_ipv4_mapped().is_some() => {
            SocketAddr::new(addr.ip().to_canonical(), addr.port())
        }
        _ => addr,
    }
}

fn sockaddr6(addr: SocketAddrV6) -> SOCKADDR_IN6 {
    SOCKADDR_IN6 {
        sin6_family: AF_INET6,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: IN6_ADDR {
            u: IN6_ADDR_0 {
                Byte: addr.ip().octets(),
            },
        },
        Anonymous: SOCKADDR_IN6_0 {
            sin6_scope_id: addr.scope_id(),
        },
    }
}

fn probe(
    handle: &Icmp,
    local: SocketAddr,
    remote: SocketAddr,
    options: &IP_OPTION_INFORMATION,
) -> (Option<(IpAddr, u32)>, u32) {
    let payload = PAYLOAD.as_ptr().cast();
    let size = PAYLOAD.len() as u16;
    // Aligned storage for the largest reply structure, payload and ICMP error data.
    let mut buffer = [0_u64; 128];
    let buffer_size = size_of::<[u64; 128]>() as u32;
    let reply_buffer = buffer.as_mut_ptr().cast();
    match (local, remote) {
        (SocketAddr::V4(source), SocketAddr::V4(destination)) => {
            let result = unsafe {
                IcmpSendEcho2Ex(
                    handle.0,
                    null_mut(),
                    None,
                    null(),
                    u32::from_ne_bytes(source.ip().octets()),
                    u32::from_ne_bytes(destination.ip().octets()),
                    payload,
                    size,
                    options,
                    reply_buffer,
                    buffer_size,
                    TIMEOUT_MS,
                )
            };
            if result == 0 {
                return (None, unsafe { GetLastError() });
            }
            // One synchronous response resides at the start of the reply buffer.
            let reply = unsafe { &*buffer.as_ptr().cast::<ICMP_ECHO_REPLY>() };
            let ip = Ipv4Addr::from(reply.Address.to_ne_bytes());
            (
                (!ip.is_unspecified()).then_some((ip.into(), reply.RoundTripTime)),
                reply.Status,
            )
        }
        (SocketAddr::V6(source), SocketAddr::V6(destination)) => {
            let result = unsafe {
                Icmp6SendEcho2(
                    handle.0,
                    null_mut(),
                    None,
                    null(),
                    &sockaddr6(source),
                    &sockaddr6(destination),
                    payload,
                    size,
                    options,
                    reply_buffer,
                    buffer_size,
                    TIMEOUT_MS,
                )
            };
            if result == 0 {
                return (None, unsafe { GetLastError() });
            }
            let reply = unsafe { &*buffer.as_ptr().cast::<ICMPV6_ECHO_REPLY_LH>() };
            let segments = reply.Address.sin6_addr;
            let ip = Ipv6Addr::from(segments.map(u16::from_be));
            (
                (!ip.is_unspecified()).then_some((ip.into(), reply.RoundTripTime)),
                reply.Status,
            )
        }
        _ => (None, IP_BAD_DESTINATION),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_reaches_ipv4_and_ipv6_loopback() {
        for ip in ["127.0.0.1:0", "[::1]:0", "[::ffff:127.0.0.1]:0"] {
            let address = ip.parse::<SocketAddr>().unwrap();
            let mut hops = Vec::new();
            let result = trace(address, address, &AtomicBool::new(false), |hop| {
                hops.push(hop)
            });
            assert!(matches!(result, Finish::Reached), "{ip} was not reached");
            assert_eq!(hops.len(), 1);
            assert_eq!(hops[0].reply.unwrap().0, address.ip().to_canonical());
        }
    }
}
