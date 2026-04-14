use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
};

use crate::daemon::Supervisor;

const HTML_TEMPLATE: &str = include_str!("../../assets/block_page/index.html");
const QUOTES_EN: &str = include_str!("../../assets/block_page/quotes.json");
const QUOTES_RU: &str = include_str!("../../assets/block_page/quotes.ru.json");

#[derive(Debug, Clone, Deserialize)]
struct Quote {
    text: String,
    author: String,
}

#[derive(Clone, Copy)]
enum Lang {
    En,
    Ru,
}

impl Lang {
    fn from_code(code: &str) -> Self {
        let c = code.to_ascii_lowercase();
        if c.starts_with("ru") {
            Lang::Ru
        } else {
            Lang::En
        }
    }
    fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }
}

struct Strings {
    title: &'static str,
    sub: &'static str,
    headline: &'static str,
    footer: &'static str,
    profile_label: &'static str,
    no_profile: &'static str,
}

fn strings(lang: Lang) -> Strings {
    match lang {
        Lang::En => Strings {
            title: "Blocked by monk",
            sub: "focus · distraction blocker",
            headline: "this site is blocked",
            footer: "stay with the task. your future self is watching.",
            profile_label: "profile:",
            no_profile: "—",
        },
        Lang::Ru => Strings {
            title: "Заблокировано monk",
            sub: "фокус · блокировщик отвлечений",
            headline: "сайт заблокирован",
            footer: "оставайся в задаче. твоё будущее «я» наблюдает.",
            profile_label: "профиль:",
            no_profile: "—",
        },
    }
}

struct Quotes {
    en: Vec<Quote>,
    ru: Vec<Quote>,
}

impl Quotes {
    fn load() -> Self {
        let en = serde_json::from_str(QUOTES_EN).unwrap_or_default();
        let ru = serde_json::from_str(QUOTES_RU).unwrap_or_default();
        Self { en, ru }
    }
    fn pick(&self, lang: Lang) -> Quote {
        let list = match lang {
            Lang::En => &self.en,
            Lang::Ru => &self.ru,
        };
        if list.is_empty() {
            return Quote { text: String::new(), author: String::new() };
        }
        let idx = rand::random_range(0..list.len());
        list[idx].clone()
    }
}

struct Shared {
    quotes: Quotes,
    supervisor: Arc<Supervisor>,
}

pub fn spawn(supervisor: Arc<Supervisor>, shutdown: Arc<Notify>) {
    let shared = Arc::new(Shared { quotes: Quotes::load(), supervisor });
    let v4: SocketAddr = (Ipv4Addr::LOCALHOST, 80).into();
    let v6: SocketAddr = (Ipv6Addr::LOCALHOST, 80).into();

    for addr in [v4, v6] {
        let shared = shared.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            let listener = match bind_with_fallback(addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(?e, %addr, "block page: all bind attempts failed");
                    return;
                }
            };
            let actual_addr = listener.local_addr().unwrap_or(addr);
            tracing::info!(%actual_addr, "block page listening");
            loop {
                tokio::select! {
                    _ = shutdown.notified() => break,
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let shared = shared.clone();
                                tokio::spawn(async move {
                                    let _ = serve(stream, &shared).await;
                                });
                            }
                            Err(e) => {
                                tracing::debug!(?e, "block page: accept failed");
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn serve(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    let request = read_request(&mut stream).await?;
    let host = parse_host(&request).unwrap_or_default();

    let locale_cfg = shared.supervisor.get_general().locale.unwrap_or_default();
    let lang = Lang::from_code(&locale_cfg);
    let s = strings(lang);

    let profile = shared
        .supervisor
        .active()
        .map(|sess| sess.profile)
        .unwrap_or_else(|| s.no_profile.to_string());

    let quote = shared.quotes.pick(lang);

    let body = render(lang, &s, &host, &profile, &quote);
    let bytes = body.as_bytes();
    let header = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(bytes).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() >= 16 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn parse_host(request: &str) -> Option<String> {
    for line in request.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("host:") {
            let raw = rest.trim();
            let no_port = raw.split(':').next().unwrap_or(raw);
            if no_port.is_empty() {
                return None;
            }
            return Some(no_port.to_string());
        }
    }
    None
}

fn render(lang: Lang, s: &Strings, host: &str, profile: &str, quote: &Quote) -> String {
    let domain = if host.is_empty() { String::new() } else { format!(" {host}") };
    HTML_TEMPLATE
        .replace("__LANG__", lang.code())
        .replace("__TITLE__", &escape(s.title))
        .replace("__SUB__", &escape(s.sub))
        .replace("__HEADLINE__", &escape(s.headline))
        .replace("__FOOTER__", &escape(s.footer))
        .replace("__PROFILE_LABEL__", &escape(s.profile_label))
        .replace("__DOMAIN__", &escape(domain.trim()))
        .replace("__PROFILE__", &escape(profile))
        .replace("__QUOTE__", &escape(&quote.text))
        .replace("__AUTHOR__", &escape(&quote.author))
}

fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

async fn bind_with_fallback(mut addr: SocketAddr) -> std::io::Result<TcpListener> {
    match TcpListener::bind(addr).await {
        Ok(listener) => return Ok(listener),
        Err(e) if is_eacces(&e) && addr.port() == 80 => {
            tracing::info!("block page on :8080 (run as root for :80)");
            addr.set_port(8080);
        }
        Err(e) => return Err(e),
    }

    for attempt in 0..3 {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if is_eaddrinuse(&e) => {
                addr.set_port(addr.port() + 1);
                if attempt == 2 {
                    tracing::warn!("block page: ports 8080-8082 all in use, giving up");
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }

    unreachable!()
}

fn is_eacces(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
}

fn is_eaddrinuse(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::AddrInUse
}
