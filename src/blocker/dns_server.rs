use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Instant,
};

use parking_lot::Mutex;
use tokio::{net::UdpSocket, sync::Notify};

pub const PORT: u16 = 53535;

const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;
const TTL: u32 = 60;
const MAX_QUERIES_PER_SECOND: u32 = 100;

pub fn spawn(shutdown: Arc<Notify>) {
    let addr: SocketAddr = (Ipv4Addr::LOCALHOST, PORT).into();
    tokio::spawn(async move {
        let socket = match UdpSocket::bind(addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, %addr, "dns server: bind failed");
                return;
            }
        };
        tracing::info!(%addr, "dns server listening");
        let rate_limiter = Arc::new(Mutex::new(HashMap::<IpAddr, (Instant, u32)>::new()));
        let mut buf = [0u8; 1500];
        loop {
            tokio::select! {
                _ = shutdown.notified() => break,
                recv = socket.recv_from(&mut buf) => {
                    match recv {
                        Ok((len, peer)) => {
                            let peer_ip = peer.ip();
                            if !is_rate_limited(peer_ip, &rate_limiter) {
                                if let Some(resp) = handle_query(&buf[..len]) {
                                    if let Err(e) = socket.send_to(&resp, peer).await {
                                        tracing::debug!(?e, "dns server: send failed");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!(?e, "dns server: recv failed");
                        }
                    }
                }
            }
        }
    });
}

fn is_rate_limited(ip: IpAddr, rate_limiter: &Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>) -> bool {
    let mut limiter = rate_limiter.lock();
    let now = Instant::now();

    limiter.retain(|_, (last_reset, _)| now.duration_since(*last_reset).as_secs() < 1);

    match limiter.get_mut(&ip) {
        Some((last_reset, count)) => {
            if now.duration_since(*last_reset).as_secs() >= 1 {
                *last_reset = now;
                *count = 1;
                false
            } else {
                *count += 1;
                *count > MAX_QUERIES_PER_SECOND
            }
        }
        None => {
            limiter.insert(ip, (now, 1));
            false
        }
    }
}

fn handle_query(req: &[u8]) -> Option<Vec<u8>> {
    if req.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([req[0], req[1]]);
    let flags = u16::from_be_bytes([req[2], req[3]]);
    let qdcount = u16::from_be_bytes([req[4], req[5]]);

    if flags & 0x8000 != 0 {
        return None;
    }
    if qdcount != 1 {
        return Some(formerr(id, flags));
    }

    let mut i = 12;
    loop {
        if i >= req.len() {
            return None;
        }
        let b = req[i];
        if b == 0 {
            i += 1;
            break;
        }
        if b & 0xC0 != 0 {
            return None;
        }
        i += 1 + b as usize;
    }
    if i + 4 > req.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([req[i], req[i + 1]]);
    let qclass = u16::from_be_bytes([req[i + 2], req[i + 3]]);
    let question_end = i + 4;

    let mut resp = Vec::with_capacity(question_end + 16);
    resp.extend_from_slice(&id.to_be_bytes());
    let opcode = flags & 0x7800;
    let rd = flags & 0x0100;
    let resp_flags: u16 = 0x8000 | opcode | 0x0400 | rd;
    resp.extend_from_slice(&resp_flags.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());

    let include_answer = qclass == CLASS_IN && (qtype == TYPE_A || qtype == TYPE_AAAA);
    let ancount: u16 = if include_answer { 1 } else { 0 };
    resp.extend_from_slice(&ancount.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());

    resp.extend_from_slice(&req[12..question_end]);

    if include_answer {
        resp.extend_from_slice(&[0xC0, 0x0C]);
        resp.extend_from_slice(&qtype.to_be_bytes());
        resp.extend_from_slice(&CLASS_IN.to_be_bytes());
        resp.extend_from_slice(&TTL.to_be_bytes());
        if qtype == TYPE_A {
            resp.extend_from_slice(&4u16.to_be_bytes());
            resp.extend_from_slice(&[127, 0, 0, 1]);
        } else {
            resp.extend_from_slice(&16u16.to_be_bytes());
            resp.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        }
    }

    Some(resp)
}

fn formerr(id: u16, flags: u16) -> Vec<u8> {
    let mut resp = Vec::with_capacity(12);
    resp.extend_from_slice(&id.to_be_bytes());
    let opcode = flags & 0x7800;
    let rd = flags & 0x0100;
    let resp_flags: u16 = 0x8000 | opcode | rd | 0x0001;
    resp.extend_from_slice(&resp_flags.to_be_bytes());
    resp.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q
    }

    fn parse_header(resp: &[u8]) -> (u16, u16, u16, u16) {
        let id = u16::from_be_bytes([resp[0], resp[1]]);
        let flags = u16::from_be_bytes([resp[2], resp[3]]);
        let qd = u16::from_be_bytes([resp[4], resp[5]]);
        let an = u16::from_be_bytes([resp[6], resp[7]]);
        (id, flags, qd, an)
    }

    #[test]
    fn a_query_returns_127_0_0_1() {
        let q = build_query(0x1234, "example.com", TYPE_A);
        let r = handle_query(&q).unwrap();
        let (id, flags, qd, an) = parse_header(&r);
        assert_eq!(id, 0x1234);
        assert_eq!(qd, 1);
        assert_eq!(an, 1);
        assert_eq!(flags & 0x8000, 0x8000);
        assert_eq!(flags & 0x000F, 0);
        let rdata = &r[r.len() - 4..];
        assert_eq!(rdata, &[127, 0, 0, 1]);
    }

    #[test]
    fn aaaa_query_returns_loopback() {
        let q = build_query(0x9abc, "foo.test", TYPE_AAAA);
        let r = handle_query(&q).unwrap();
        let (_, _, _, an) = parse_header(&r);
        assert_eq!(an, 1);
        let rdata = &r[r.len() - 16..];
        assert_eq!(rdata, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn mx_query_returns_nodata() {
        let q = build_query(1, "mail.test", 15);
        let r = handle_query(&q).unwrap();
        let (_, flags, qd, an) = parse_header(&r);
        assert_eq!(qd, 1);
        assert_eq!(an, 0);
        assert_eq!(flags & 0x000F, 0);
    }

    #[test]
    fn truncated_query_is_dropped() {
        assert!(handle_query(&[0; 5]).is_none());
    }

    #[test]
    fn bad_qdcount_returns_formerr() {
        let mut q = build_query(7, "x.test", TYPE_A);
        q[4] = 0;
        q[5] = 2;
        let r = handle_query(&q).unwrap();
        let (_, flags, _, _) = parse_header(&r);
        assert_eq!(flags & 0x000F, 1);
    }

    #[test]
    fn compression_pointer_in_question_rejected() {
        let mut q = Vec::new();
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        q.extend_from_slice(&[0xC0, 0x0C]);
        q.extend_from_slice(&TYPE_A.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        assert!(handle_query(&q).is_none());
    }
}
