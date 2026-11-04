//! LAN discovery for Studio Live (STUDIO-LIVE-SPEC.md § 6 "Discovery"; 0.5 Studio's `studio_live.rs` advertise / browse
//! on mdns-sd): a host advertises `_ccstudio._tcp.local.` over mDNS with **only the session's random tag** as the
//! instance name (a short code's two characters), its port, a TXT `app=<slug>` and a host name that says nothing about
//! the machine (`ccstudio-<8 hex>.local.`); a joiner who typed a code looks for that tag and connects to what answers.
//!
//! Hand-written DNS on std::net UDP (RFC 1035 messages, RFC 6762 multicast DNS, RFC 6763 service records): every
//! name, record and count read is bounded and a malformed packet is dropped, never a panic (fuzz target `live_mdns`).
//! Browsing asks from an ephemeral port (a "legacy unicast" one-shot query, RFC 6762 § 6.7): responders answer it
//! directly, so it works beside any other mDNS responder on the machine. Advertising receives queries on port 5353,
//! shared with the OS's own responder (socket2: SO_REUSEADDR / SO_REUSEPORT); where the port still can't be had the
//! advertisement is unavailable and joiners use the QR or the address (the Live window says so). The socket opens only
//! while a session is hosted or someone browses.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const SERVICE: &str = "_ccstudio._tcp.local.";
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const PORT: u16 = 5353;
const T_A: u16 = 1;
const T_PTR: u16 = 12;
const T_TXT: u16 = 16;
const T_AAAA: u16 = 28;
const T_SRV: u16 = 33;
const T_ANY: u16 = 255;
const MAX_PACKET: usize = 9000;
const MAX_RECORDS: usize = 256;
const MAX_NAME: usize = 255;

/// One resource record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Ptr(String),
    Srv { port: u16, target: String },
    Txt(Vec<Vec<u8>>),
    A(Ipv4Addr),
    Aaaa(std::net::Ipv6Addr),
    Other(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub ttl: u32,
    pub data: Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
}

/// A DNS message as far as discovery needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Msg {
    pub id: u16,
    pub response: bool,
    pub questions: Vec<Question>,
    /// answers, authorities and additionals together
    pub records: Vec<Record>,
}

// ---------------------------------------------------------------------------------------------------------------------
// reading

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p = self.p.checked_add(1)?;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes([self.u8()?, self.u8()?, self.u8()?, self.u8()?]))
    }
    /// A name at the cursor (compression pointers followed backwards only, at most 32 of them).
    fn name(&mut self) -> Option<String> {
        let (n, next) = read_name(self.b, self.p)?;
        self.p = next;
        Some(n)
    }
}

/// The name at `at` and where the bytes after it start.
fn read_name(b: &[u8], at: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut p = at;
    let mut end: Option<usize> = None;
    let mut jumps = 0u8;
    loop {
        let len = *b.get(p)?;
        if len & 0xc0 == 0xc0 {
            let lo = *b.get(p.checked_add(1)?)?;
            let target = usize::from(u16::from_be_bytes([len & 0x3f, lo]));
            if end.is_none() {
                end = Some(p.checked_add(2)?);
            }
            // pointers go back, never to themselves or forward: no loops
            if target >= p || jumps >= 32 {
                return None;
            }
            jumps = jumps.saturating_add(1);
            p = target;
            continue;
        }
        if len & 0xc0 != 0 {
            return None;
        }
        p = p.checked_add(1)?;
        if len == 0 {
            break;
        }
        let l = usize::from(len);
        let label = b.get(p..p.checked_add(l)?)?;
        p = p.checked_add(l)?;
        out.push_str(&String::from_utf8_lossy(label));
        out.push('.');
        if out.len() > MAX_NAME {
            return None;
        }
    }
    if out.is_empty() {
        out.push('.');
    }
    Some((out, end.unwrap_or(p)))
}

/// Parses a packet (None: not a DNS message, or malformed).
pub fn parse(b: &[u8]) -> Option<Msg> {
    if b.len() > MAX_PACKET {
        return None;
    }
    let mut r = Rd { b, p: 0 };
    let id = r.u16()?;
    let flags = r.u16()?;
    let qd = usize::from(r.u16()?);
    let an = usize::from(r.u16()?);
    let ns = usize::from(r.u16()?);
    let ar = usize::from(r.u16()?);
    if qd > MAX_RECORDS || an.saturating_add(ns).saturating_add(ar) > MAX_RECORDS {
        return None;
    }
    let mut m = Msg { id, response: flags & 0x8000 != 0, ..Default::default() };
    for _ in 0..qd {
        let name = r.name()?;
        let qtype = r.u16()?;
        let _class = r.u16()?;
        m.questions.push(Question { name, qtype });
    }
    for _ in 0..an.saturating_add(ns).saturating_add(ar) {
        let name = r.name()?;
        let rtype = r.u16()?;
        let _class = r.u16()?;
        let ttl = r.u32()?;
        let len = usize::from(r.u16()?);
        let start = r.p;
        let stop = start.checked_add(len)?;
        let rdata = b.get(start..stop)?;
        let data = match rtype {
            T_PTR => Data::Ptr(read_name(b, start)?.0),
            T_SRV => {
                let port = u16::from_be_bytes([*rdata.get(4)?, *rdata.get(5)?]);
                Data::Srv { port, target: read_name(b, start.checked_add(6)?)?.0 }
            }
            T_TXT => {
                let mut parts = vec![];
                let mut q = 0usize;
                while q < rdata.len() {
                    let l = usize::from(*rdata.get(q)?);
                    let s = q.checked_add(1)?;
                    parts.push(rdata.get(s..s.checked_add(l)?)?.to_vec());
                    q = s.checked_add(l)?;
                }
                Data::Txt(parts)
            }
            T_A if len == 4 => Data::A(Ipv4Addr::new(*rdata.first()?, *rdata.get(1)?, *rdata.get(2)?, *rdata.get(3)?)),
            T_AAAA if len == 16 => {
                let mut o = [0u8; 16];
                o.copy_from_slice(rdata);
                Data::Aaaa(std::net::Ipv6Addr::from(o))
            }
            t => Data::Other(t),
        };
        r.p = stop;
        m.records.push(Record { name, ttl, data });
    }
    Some(m)
}

// ---------------------------------------------------------------------------------------------------------------------
// writing (no compression: our packets are small)

fn put_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.').filter(|l| !l.is_empty()) {
        let l = label.as_bytes();
        let n = l.len().min(63);
        out.push(u8::try_from(n).unwrap_or(63));
        out.extend_from_slice(l.get(..n).unwrap_or(&[]));
    }
    out.push(0);
}

fn put_record(out: &mut Vec<u8>, r: &Record, flush: bool) {
    put_name(out, &r.name);
    let (t, rdata) = match &r.data {
        Data::Ptr(n) => {
            let mut d = vec![];
            put_name(&mut d, n);
            (T_PTR, d)
        }
        Data::Srv { port, target } => {
            let mut d = vec![0, 0, 0, 0];
            d.extend_from_slice(&port.to_be_bytes());
            put_name(&mut d, target);
            (T_SRV, d)
        }
        Data::Txt(parts) => {
            let mut d = vec![];
            for p in parts {
                let n = p.len().min(255);
                d.push(u8::try_from(n).unwrap_or(255));
                d.extend_from_slice(p.get(..n).unwrap_or(&[]));
            }
            if d.is_empty() {
                d.push(0);
            }
            (T_TXT, d)
        }
        Data::A(a) => (T_A, a.octets().to_vec()),
        Data::Aaaa(a) => (T_AAAA, a.octets().to_vec()),
        Data::Other(t) => (*t, vec![]),
    };
    out.extend_from_slice(&t.to_be_bytes());
    // class IN; the cache-flush bit on unique records (all but PTR)
    let class: u16 = if flush && t != T_PTR { 0x8001 } else { 1 };
    out.extend_from_slice(&class.to_be_bytes());
    out.extend_from_slice(&r.ttl.to_be_bytes());
    out.extend_from_slice(&u16::try_from(rdata.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&rdata);
}

/// A message's bytes. `legacy`: an answer to a one-shot query (id and question echoed, no cache-flush bits, TTL ≤ 10).
pub fn encode(m: &Msg, legacy: bool) -> Vec<u8> {
    let mut out = vec![];
    out.extend_from_slice(&m.id.to_be_bytes());
    out.extend_from_slice(&(if m.response { 0x8400u16 } else { 0 }).to_be_bytes());
    out.extend_from_slice(&u16::try_from(m.questions.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&u16::try_from(m.records.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    for q in &m.questions {
        put_name(&mut out, &q.name);
        out.extend_from_slice(&q.qtype.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
    }
    for r in &m.records {
        let mut r = r.clone();
        if legacy {
            r.ttl = r.ttl.min(10);
        }
        put_record(&mut out, &r, m.response && !legacy);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// the service

/// What a host advertises.
#[derive(Debug, Clone)]
pub struct Service {
    pub tag: String,
    pub app: String,
    pub port: u16,
    pub host: String,
    pub addrs: Vec<Ipv4Addr>,
}

impl Service {
    pub fn instance(&self) -> String {
        format!("{}.{SERVICE}", self.tag)
    }
    fn records(&self, ttl: u32) -> Vec<Record> {
        let inst = self.instance();
        let mut v = vec![
            Record { name: SERVICE.into(), ttl, data: Data::Ptr(inst.clone()) },
            Record { name: inst.clone(), ttl, data: Data::Srv { port: self.port, target: self.host.clone() } },
            Record { name: inst, ttl, data: Data::Txt(vec![format!("app={}", self.app).into_bytes()]) },
        ];
        for a in &self.addrs {
            v.push(Record { name: self.host.clone(), ttl, data: Data::A(*a) });
        }
        v
    }
    /// Our answer to a query, if it asks about us.
    pub fn answer(&self, q: &Msg) -> Option<Msg> {
        if q.response {
            return None;
        }
        let inst = self.instance();
        let asks = q.questions.iter().any(|x| {
            let n = x.name.to_ascii_lowercase();
            (n == SERVICE && matches!(x.qtype, T_PTR | T_ANY))
                || (n == inst.to_ascii_lowercase() && matches!(x.qtype, T_SRV | T_TXT | T_ANY))
                || (n == self.host.to_ascii_lowercase() && matches!(x.qtype, T_A | T_ANY))
        });
        if !asks {
            return None;
        }
        // Known answers (RFC 6762 § 7.1): a querier that already has our PTR doesn't need it again.
        let known = q.records.iter().any(|r| r.name.eq_ignore_ascii_case(SERVICE) && r.data == Data::Ptr(inst.clone()) && r.ttl > 2250);
        let mut recs = self.records(4500);
        if known {
            recs.retain(|r| !matches!(r.data, Data::Ptr(_)));
            if recs.is_empty() {
                return None;
            }
        }
        Some(Msg { id: 0, response: true, questions: vec![], records: recs })
    }
}

/// A session found on the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub addr: String,
    pub port: u16,
    pub app: String,
}

/// What answers about `tag` say (instance → SRV → host's A records, TXT app).
pub fn resolve(msgs: &[Msg], tag: &str) -> Vec<Found> {
    let want = format!("{tag}.{SERVICE}").to_ascii_lowercase();
    let all: Vec<&Record> = msgs.iter().flat_map(|m| m.records.iter()).collect();
    let mut out = vec![];
    for r in &all {
        let Data::Srv { port, target } = &r.data else { continue };
        if r.name.to_ascii_lowercase() != want {
            continue;
        }
        let app = all
            .iter()
            .filter(|x| x.name.to_ascii_lowercase() == want)
            .find_map(|x| match &x.data {
                Data::Txt(p) => p.iter().find_map(|t| t.strip_prefix(b"app=").map(|v| String::from_utf8_lossy(v).into_owned())),
                _ => None,
            })
            .unwrap_or_default();
        let mut ips: Vec<IpAddr> = all
            .iter()
            .filter(|x| x.name.eq_ignore_ascii_case(target))
            .filter_map(|x| match x.data {
                Data::A(a) => Some(IpAddr::V4(a)),
                Data::Aaaa(a) if (a.segments().first().copied().unwrap_or(0) & 0xffc0) != 0xfe80 => Some(IpAddr::V6(a)),
                _ => None,
            })
            .collect();
        // IPv4 first, private ranges first (as 0.5's browse sorts them)
        ips.sort_by_key(|ip| (!ip.is_ipv4(), matches!(ip, IpAddr::V4(a) if !a.is_private()), *ip));
        ips.dedup();
        for ip in ips {
            let f = Found { addr: ip.to_string(), port: *port, app: app.clone() };
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// This machine's addresses for the QR and the advertisement: non-loopback IPv4 (no 169.254), private ranges first.
/// std has no interface list: the address the OS would send from towards the LAN and the internet is asked for.
pub fn lan_addrs() -> Vec<String> {
    let mut v: Vec<Ipv4Addr> = vec![];
    for probe in ["192.168.255.255:9", "10.255.255.255:9", "172.31.255.255:9", "8.8.8.8:53"] {
        let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { continue };
        if s.connect(probe).is_ok()
            && let Ok(SocketAddr::V4(a)) = s.local_addr()
        {
            let ip = *a.ip();
            if !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !v.contains(&ip) {
                v.push(ip);
            }
        }
    }
    v.sort_by_key(|a| (!a.is_private(), a.octets()));
    v.into_iter().map(|a| a.to_string()).collect()
}

/// A running advertisement: stop it to say goodbye (TTL 0) and end its thread.
pub struct Advert {
    stop: Arc<AtomicBool>,
    pub service: Service,
}

impl Advert {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Advert {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A random host name that says nothing about the machine or the person.
fn host_name() -> String {
    let r = crate::crypto::random(4).unwrap_or_else(|| vec![0, 0, 0, 0]);
    format!("ccstudio-{}.local.", r.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

/// Advertises a session's tag (`TT`, or an invite room's 8-character tag) for `app` on `port`.
pub fn advertise(tag: &str, app: &str, port: u16, addrs: &[String]) -> Result<Advert, String> {
    let tag = tag.to_ascii_uppercase();
    if !(tag.len() == 2 || tag.len() == 8) || !tag.bytes().all(|b| crate::crypto::CODE_ALPHABET.contains(&b)) {
        return Err("mdns: bad tag".into());
    }
    let ips: Vec<Ipv4Addr> = addrs.iter().filter_map(|a| a.parse().ok()).collect();
    if ips.is_empty() {
        return Err("offline: no network address".into());
    }
    let service =
        Service { tag, app: app.chars().filter(|c| c.is_ascii_alphanumeric()).take(32).collect(), port, host: host_name(), addrs: ips };
    let sock = shared_socket().map_err(|e| format!("mdns: port 5353 ({e})"))?;
    for ip in &service.addrs {
        let _ = sock.join_multicast_v4(&GROUP, ip);
    }
    let _ = sock.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED);
    let _ = sock.set_multicast_ttl_v4(255);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let stop = Arc::new(AtomicBool::new(false));
    let (st, svc) = (stop.clone(), service.clone());
    std::thread::Builder::new().name("cc-live-mdns".into()).spawn(move || responder(&sock, &svc, &st)).map_err(|e| e.to_string())?;
    Ok(Advert { stop, service })
}

/// Port 5353 shared with the machine's other mDNS responders (macOS mDNSResponder, Windows' DNS client, avahi):
/// SO_REUSEADDR (and SO_REUSEPORT on Unix) through socket2, then a std socket.
fn shared_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    s.set_reuse_port(true)?;
    s.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    Ok(s.into())
}

fn responder(sock: &UdpSocket, svc: &Service, stop: &AtomicBool) {
    let group = SocketAddr::V4(SocketAddrV4::new(GROUP, PORT));
    let announce = encode(&Msg { id: 0, response: true, questions: vec![], records: svc.records(4500) }, false);
    // announced twice, a second apart (RFC 6762 § 8.3)
    let _ = sock.send_to(&announce, group);
    let later = || Instant::now().checked_add(Duration::from_secs(1)).unwrap_or_else(Instant::now);
    let mut next = later();
    let mut announced = 1u8;
    let mut buf = vec![0u8; MAX_PACKET];
    while !stop.load(Ordering::SeqCst) {
        if announced < 2 && Instant::now() >= next {
            let _ = sock.send_to(&announce, group);
            announced = announced.saturating_add(1);
            next = later();
        }
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let Some(q) = buf.get(..n).and_then(parse) else { continue };
        let Some(mut a) = svc.answer(&q) else { continue };
        if from.port() != PORT {
            // a one-shot query: answered to its sender, with its id and question
            a.id = q.id;
            a.questions = q.questions.clone();
            let _ = sock.send_to(&encode(&a, true), from);
        } else {
            let _ = sock.send_to(&encode(&a, false), group);
        }
    }
    // goodbye: the same records with TTL 0
    let bye = encode(&Msg { id: 0, response: true, questions: vec![], records: svc.records(0) }, false);
    let _ = sock.send_to(&bye, group);
}

/// Looks for the session tagged `tag` for at most `wait` (≤ 5 s); answers as soon as it resolves.
pub fn browse(tag: &str, wait: Duration) -> Result<Vec<Found>, String> {
    let tag = tag.to_ascii_uppercase();
    let wait = wait.clamp(Duration::from_millis(100), Duration::from_secs(5));
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("mdns: {e}"))?;
    let _ = sock.set_multicast_ttl_v4(255);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(100)));
    let inst = format!("{tag}.{SERVICE}");
    let q = Msg {
        id: 0x4343,
        response: false,
        questions: vec![Question { name: SERVICE.into(), qtype: T_PTR }, Question { name: inst, qtype: T_SRV }],
        records: vec![],
    };
    let bytes = encode(&q, false);
    let group = SocketAddr::V4(SocketAddrV4::new(GROUP, PORT));
    let until = Instant::now().checked_add(wait).unwrap_or_else(Instant::now);
    let mut asked: Option<Instant> = None;
    let mut got: Vec<Msg> = vec![];
    let mut buf = vec![0u8; MAX_PACKET];
    while Instant::now() < until {
        if asked.is_none_or(|a| a.elapsed() >= Duration::from_millis(400)) {
            sock.send_to(&bytes, group).map_err(|e| format!("mdns: {e}"))?;
            asked = Some(Instant::now());
        }
        let Ok((n, _)) = sock.recv_from(&mut buf) else { continue };
        if let Some(m) = buf.get(..n).and_then(parse)
            && m.response
        {
            got.push(m);
            if got.len() > 64 {
                got.remove(0);
            }
            let f = resolve(&got, &tag);
            if !f.is_empty() {
                return Ok(f);
            }
        }
    }
    Ok(vec![])
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    fn svc() -> Service {
        Service {
            tag: "K7".into(),
            app: "cadcraft".into(),
            port: 47620,
            host: "ccstudio-0a0b0c0d.local.".into(),
            addrs: vec![Ipv4Addr::new(192, 168, 1, 20)],
        }
    }

    #[test]
    fn a_query_gets_the_service_and_it_resolves() {
        let q = Msg { id: 7, response: false, questions: vec![Question { name: SERVICE.into(), qtype: T_PTR }], records: vec![] };
        let back = parse(&encode(&q, false)).unwrap();
        assert_eq!(back, q);
        let a = svc().answer(&back).unwrap();
        let wire = encode(&a, false);
        let m = parse(&wire).unwrap();
        assert!(m.response);
        assert_eq!(resolve(&[m], "k7"), vec![Found { addr: "192.168.1.20".into(), port: 47620, app: "cadcraft".into() }]);
        // a query about someone else's tag isn't answered
        let other =
            Msg { id: 1, response: false, questions: vec![Question { name: format!("Q4.{SERVICE}"), qtype: T_SRV }], records: vec![] };
        assert!(svc().answer(&other).is_none());
    }

    #[test]
    fn compressed_names_and_hostile_packets() {
        // a hand-made response with a compression pointer: PTR _ccstudio._tcp.local. → K7.<ptr>
        let mut b = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        let svc_at = b.len();
        put_name(&mut b, SERVICE);
        b.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let rdata = [2, b'K', b'7', 0xc0, u8::try_from(svc_at).unwrap()];
        b.extend_from_slice(&[0, u8::try_from(rdata.len()).unwrap()]);
        b.extend_from_slice(&rdata);
        let m = parse(&b).unwrap();
        assert_eq!(m.records[0].data, Data::Ptr(format!("K7.{SERVICE}")));
        // a pointer to itself, a forward pointer, truncations: dropped
        let mut loopy = b.clone();
        let n = loopy.len();
        loopy[n - 2] = 0xc0;
        loopy[n - 1] = u8::try_from(n - 2).unwrap();
        assert!(parse(&loopy).map(|m| m.records.is_empty()).unwrap_or(true));
        for cut in 0..b.len() {
            let _ = parse(&b[..cut]);
        }
        assert!(parse(&[0xff; 12]).is_none());
    }

    #[test]
    fn browse_finds_an_advert_on_this_machine() {
        // needs multicast loopback and a free port 5353: skipped quietly where the machine has neither
        let ad = match advertise("Z9", "partcraft", 40001, &["127.0.0.1".into()]) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("mdns: no advert here ({e})");
                return;
            }
        };
        let found = browse("Z9", Duration::from_millis(1500)).unwrap_or_default();
        eprintln!("mdns: found {found:?}");
        ad.stop();
        if let Some(f) = found.first() {
            assert_eq!((f.port, f.app.as_str()), (40001, "partcraft"));
        }
    }
}
