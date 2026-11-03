use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::mem::{align_of, size_of};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER},
    NetworkManagement::IpHelper::*,
    Networking::WinSock::{AF_INET, AF_INET6, IN6_ADDR, IN6_ADDR_0},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key {
    pub pid: u32,
    pub local: SocketAddr,
    pub remote: SocketAddr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Traffic<T> {
    pub down: T,
    pub up: T,
}

#[derive(Clone)]
pub struct Sample {
    pub time: Instant,
    pub rate: Option<Traffic<f64>>,
}

#[derive(Clone, Default)]
pub struct Transfer {
    previous: Option<(Instant, Traffic<u64>)>,
    pub totals: Traffic<u64>,
    pub history: VecDeque<Sample>,
}

impl Transfer {
    fn record(&mut self, now: Instant, bytes: Option<Traffic<u64>>) {
        let rate = match (self.previous, bytes) {
            (Some((then, previous)), Some(current)) => {
                let elapsed = (now - then).as_secs_f64();
                match (
                    current.down.checked_sub(previous.down),
                    current.up.checked_sub(previous.up),
                ) {
                    (Some(down), Some(up)) if elapsed > 0.0 => {
                        self.totals.down = self.totals.down.saturating_add(down);
                        self.totals.up = self.totals.up.saturating_add(up);
                        Some(Traffic {
                            down: down as f64 / elapsed,
                            up: up as f64 / elapsed,
                        })
                    }
                    _ => {
                        self.totals = Traffic::default();
                        self.history.clear();
                        None
                    }
                }
            }
            _ => None,
        };
        self.previous = bytes.map(|bytes| (now, bytes));
        self.history.push_back(Sample { time: now, rate });
        while self
            .history
            .front()
            .is_some_and(|s| now - s.time > Duration::from_secs(60))
        {
            self.history.pop_front();
        }
    }

    pub fn rate(&self) -> Option<Traffic<f64>> {
        self.history.back()?.rate
    }

    pub fn has_counters(&self) -> bool {
        self.previous.is_some()
    }
}

pub struct Connection {
    pub key: Key,
    pub process: Arc<Process>,
    pub state: u32,
    pub transfer: Transfer,
    pub traffic_error: Option<u32>,
}

pub struct Process {
    pub name: String,
    pub image: Option<PathBuf>,
}

#[derive(Default)]
pub struct Snapshot {
    pub connections: Vec<Connection>,
    pub table_errors: Vec<String>,
}

#[derive(Default)]
pub struct Monitor {
    transfers: HashMap<Key, Transfer>,
}

impl Monitor {
    pub fn poll(&mut self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        let mut names = HashMap::new();
        let mut previous = std::mem::take(&mut self.transfers);
        let now = Instant::now();
        for family in [AF_INET, AF_INET6] {
            let rows = match rows(family) {
                Ok(rows) => rows,
                Err(code) => {
                    let version = if family == AF_INET { "IPv4" } else { "IPv6" };
                    snapshot.table_errors.push(format!("{version} ({code})"));
                    self.transfers.extend(
                        previous
                            .extract_if(|key, _| key.local.is_ipv4() == (family == AF_INET))
                            .map(|(key, mut transfer)| {
                                transfer.record(now, None);
                                (key, transfer)
                            }),
                    );
                    continue;
                }
            };
            for row in rows {
                let (key, state) = row.key_and_state();
                if key.remote.port() == 0 || key.remote.ip().is_unspecified() {
                    continue;
                }
                let process = names
                    .entry(key.pid)
                    .or_insert_with(|| Arc::new(process_info(key.pid)))
                    .clone();
                let mut transfer = previous.remove(&key).unwrap_or_default();
                let counters = if state == MIB_TCP_STATE_ESTAB as u32 {
                    row.counters().map(Some)
                } else {
                    Ok(None)
                };
                transfer.record(now, counters.ok().flatten());
                self.transfers.insert(key, transfer.clone());
                snapshot.connections.push(Connection {
                    key,
                    process,
                    state,
                    transfer,
                    traffic_error: counters.err(),
                });
            }
        }
        snapshot
            .connections
            .sort_unstable_by(|a, b| a.process.name.cmp(&b.process.name).then(a.key.cmp(&b.key)));
        snapshot
    }
}

enum Row {
    V4(MIB_TCPROW_OWNER_PID),
    V6(MIB_TCP6ROW_OWNER_PID),
}

fn rows(family: u16) -> Result<Vec<Row>, u32> {
    if family == AF_INET {
        // These Windows table rows are C structs made of u32 values.
        unsafe { table(family) }.map(|rows| rows.into_iter().map(Row::V4).collect())
    } else {
        // IPv6 rows have the same alignment and also contain byte arrays.
        unsafe { table(family) }.map(|rows| rows.into_iter().map(Row::V6).collect())
    }
}

/// The caller supplies the owner-PID row type matching the address family.
unsafe fn table<T: Copy>(family: u16) -> Result<Vec<T>, u32> {
    assert!(align_of::<T>() <= align_of::<u32>());
    let mut size = 0;
    let read = |ptr, size| unsafe {
        GetExtendedTcpTable(ptr, size, 0, u32::from(family), TCP_TABLE_OWNER_PID_ALL, 0)
    };
    let mut status = read(null_mut(), &mut size);
    for _ in 0..4 {
        if status != ERROR_INSUFFICIENT_BUFFER {
            break;
        }
        let mut buffer = vec![0_u32; (size as usize).div_ceil(size_of::<u32>())];
        status = read(buffer.as_mut_ptr().cast(), &mut size);
        if status == 0 {
            let count = buffer.first().copied().unwrap_or_default() as usize;
            let available = (size as usize).min(buffer.len() * size_of::<u32>());
            if available < 4 || count > (available - 4) / size_of::<T>() {
                return Err(13); // ERROR_INVALID_DATA
            }
            let ptr = unsafe { buffer.as_ptr().add(1).cast::<T>() };
            return Ok(unsafe { std::slice::from_raw_parts(ptr, count) }.to_vec());
        }
    }
    if status == 0 {
        Ok(Vec::new())
    } else {
        Err(status)
    }
}

fn port(network: u32) -> u16 {
    u16::from_be(network as u16)
}

impl Row {
    fn key_and_state(&self) -> (Key, u32) {
        match self {
            Self::V4(row) => (
                Key {
                    pid: row.dwOwningPid,
                    local: (
                        Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()),
                        port(row.dwLocalPort),
                    )
                        .into(),
                    remote: (
                        Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes()),
                        port(row.dwRemotePort),
                    )
                        .into(),
                },
                row.dwState,
            ),
            Self::V6(row) => {
                let socket = |ip, port_number, scope| {
                    SocketAddrV6::new(
                        Ipv6Addr::from(ip),
                        port(port_number),
                        0,
                        u32::from_be(scope),
                    )
                    .into()
                };
                (
                    Key {
                        pid: row.dwOwningPid,
                        local: socket(row.ucLocalAddr, row.dwLocalPort, row.dwLocalScopeId),
                        remote: socket(row.ucRemoteAddr, row.dwRemotePort, row.dwRemoteScopeId),
                    },
                    row.dwState,
                )
            }
        }
    }

    fn counters(&self) -> Result<Traffic<u64>, u32> {
        match self {
            Self::V4(r) => counters_v4(&MIB_TCPROW_LH {
                Anonymous: MIB_TCPROW_LH_0 { dwState: r.dwState },
                dwLocalAddr: r.dwLocalAddr,
                dwLocalPort: r.dwLocalPort,
                dwRemoteAddr: r.dwRemoteAddr,
                dwRemotePort: r.dwRemotePort,
            }),
            Self::V6(r) => counters_v6(&MIB_TCP6ROW {
                State: r.dwState as MIB_TCP_STATE,
                LocalAddr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: r.ucLocalAddr,
                    },
                },
                dwLocalScopeId: r.dwLocalScopeId,
                dwLocalPort: r.dwLocalPort,
                RemoteAddr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: r.ucRemoteAddr,
                    },
                },
                dwRemoteScopeId: r.dwRemoteScopeId,
                dwRemotePort: r.dwRemotePort,
            }),
        }
    }
}

macro_rules! estats {
    ($name:ident, $row:ty, $get:ident, $set:ident) => {
        fn $name(row: &$row) -> Result<Traffic<u64>, u32> {
            let mut data = TCP_ESTATS_DATA_ROD_v0::default();
            let mut enabled = 0_u64;
            let flag_size = size_of::<TCP_ESTATS_DATA_RW_v0>() as u32;
            let mut read = |flag: &mut u64| unsafe {
                $get(
                    row,
                    TcpConnectionEstatsData,
                    (flag as *mut u64).cast(),
                    0,
                    flag_size,
                    null_mut(),
                    0,
                    0,
                    (&raw mut data).cast(),
                    0,
                    size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
                )
            };
            let mut status = read(&mut enabled);
            if status != 0 || enabled == 0 {
                // The data read can fail for a connection with collection disabled.
                // Enable using a buffer with a single BOOLEAN and retry once.
                let request = 1_u64;
                status = unsafe {
                    $set(
                        row,
                        TcpConnectionEstatsData,
                        (&raw const request).cast(),
                        0,
                        flag_size,
                        0,
                    )
                };
                if status != 0 {
                    return Err(status);
                }
                status = read(&mut enabled);
            }
            if status != 0 {
                return Err(status);
            }
            if enabled == 0 {
                return Err(windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED);
            }
            Ok(Traffic {
                down: data.DataBytesIn,
                up: data.DataBytesOut,
            })
        }
    };
}

estats!(
    counters_v4,
    MIB_TCPROW_LH,
    GetPerTcpConnectionEStats,
    SetPerTcpConnectionEStats
);
estats!(
    counters_v6,
    MIB_TCP6ROW,
    GetPerTcp6ConnectionEStats,
    SetPerTcp6ConnectionEStats
);

fn process_info(pid: u32) -> Process {
    let unknown = || Process {
        name: format!("PID {pid}"),
        image: None,
    };
    let mut name = [0_u16; 1024];
    let mut len = name.len() as u32;
    // Open for query only and always close the handle.
    let success = unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return unknown();
        }
        let result = QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        result
    };
    if success == 0 {
        return unknown();
    }
    let image = PathBuf::from(OsString::from_wide(&name[..len as usize]));
    let name = image
        .file_name()
        .unwrap_or(image.as_os_str())
        .to_string_lossy()
        .into_owned();
    Process {
        name,
        image: Some(image),
    }
}

pub fn state_label(state: u32) -> &'static str {
    match state as MIB_TCP_STATE {
        MIB_TCP_STATE_ESTAB => "Bağlı",
        MIB_TCP_STATE_SYN_SENT | MIB_TCP_STATE_SYN_RCVD => "Kuruluyor",
        MIB_TCP_STATE_CLOSE_WAIT
        | MIB_TCP_STATE_FIN_WAIT1
        | MIB_TCP_STATE_FIN_WAIT2
        | MIB_TCP_STATE_LAST_ACK
        | MIB_TCP_STATE_TIME_WAIT
        | MIB_TCP_STATE_CLOSING => "Kapanıyor",
        MIB_TCP_STATE_CLOSED | MIB_TCP_STATE_DELETE_TCB => "Kapalı",
        _ => "Bilinmiyor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;

    #[test]
    fn rates_use_elapsed_time_and_omit_gaps() {
        let start = Instant::now();
        let mut transfer = Transfer::default();
        let bytes = |down, up| Some(Traffic { down, up });
        transfer.record(start, bytes(100, 200));
        assert_eq!(transfer.rate(), None);
        transfer.record(start + Duration::from_secs(2), bytes(2_100, 700));
        assert_eq!(
            transfer.rate(),
            Some(Traffic {
                down: 1_000.0,
                up: 250.0
            })
        );
        assert_eq!(
            transfer.totals,
            Traffic {
                down: 2_000,
                up: 500
            }
        );
        transfer.record(start + Duration::from_secs(3), None);
        transfer.record(start + Duration::from_secs(4), bytes(10_100, 900));
        assert_eq!(transfer.rate(), None);
        assert_eq!(
            transfer.totals,
            Traffic {
                down: 2_000,
                up: 500
            }
        );
        transfer.record(start + Duration::from_secs(65), bytes(10_100, 900));
        assert_eq!(transfer.history.len(), 1);
    }

    #[test]
    fn enumerates_and_samples_ipv4_and_ipv6_tcp_sockets() {
        for ip in ["127.0.0.1:0", "[::1]:0"] {
            let listener = TcpListener::bind(ip).unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let key = Key {
                pid: std::process::id(),
                local: client.local_addr().unwrap(),
                remote: client.peer_addr().unwrap(),
            };
            let mut monitor = Monitor::default();
            let initial = monitor.poll();
            let initial_row = initial.connections.iter().find(|c| c.key == key).unwrap();
            if initial_row.traffic_error == Some(ERROR_ACCESS_DENIED) {
                assert!(
                    std::env::var_os("NW_REQUIRE_LIVE_COUNTERS").is_none(),
                    "{ip}: the privileged CI test must be able to enable EStats"
                );
                assert!(!initial_row.transfer.has_counters());
                assert_eq!(initial_row.transfer.rate(), None);
                eprintln!("{ip}: TCP table verified, Windows denied permission to enable EStats");
                continue;
            }
            assert!(
                initial_row.transfer.has_counters(),
                "{ip}: {:?}",
                initial_row.traffic_error
            );

            let mut payload = [0; 8_192];
            client.write_all(&payload).unwrap();
            server.read_exact(&mut payload).unwrap();
            server.write_all(&payload[..4_096]).unwrap();
            client.read_exact(&mut payload[..4_096]).unwrap();
            let updated = monitor.poll();
            let row = updated.connections.iter().find(|c| c.key == key).unwrap();
            assert!(row.transfer.totals.down >= 4_096);
            assert!(row.transfer.totals.up >= 8_192);
            assert!(row.transfer.rate().unwrap().down > 0.0);
            eprintln!("{ip}: TCP table and live byte counters verified");
        }
    }
}
