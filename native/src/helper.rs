//! One-shot, origin-bound launcher. No service, startup task, or command shell.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, io::{BufRead, BufReader, Write}, net::TcpListener, process::{Child, ChildStdin, Command, Stdio}, sync::mpsc, time::{Duration, Instant}};
use tungstenite::{accept_hdr_with_config, handshake::server::{Request, Response}, protocol::WebSocketConfig, Message};
use url::Url;
const ORIGINS: &[&str] = &["https://pulse.to-unknown.com", "https://pulse-max-friends.to-unknown.chatgpt.site"];
const PORT: u16 = 37654;

pub fn validate_link(link: &str) -> Result<(String, String)> {
    let u = Url::parse(link)?;
    if u.scheme() != "pulse-capture" || u.host_str() != Some("share") || !matches!(u.path(), "" | "/") || u.fragment().is_some() || !u.username().is_empty() || u.password().is_some() { bail!("Invalid launch link (nonempty path: {})", !u.path().is_empty()); }
    let pairs: Vec<_> = u.query_pairs().collect();
    if pairs.len() != 2 { bail!("Invalid launch parameters"); }
    let origin = pairs.iter().find(|p| p.0 == "origin").context("Missing origin")?.1.to_string();
    let token = pairs.iter().find(|p| p.0 == "token").context("Missing token")?.1.to_string();
    if !ORIGINS.contains(&origin.as_str()) || token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) { bail!("Untrusted launch request (origin allowed: {}, token length: {})", ORIGINS.contains(&origin.as_str()), token.len()); }
    Ok((origin, token))
}
struct Worker { child: Child, input: ChildStdin }
impl Drop for Worker { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }
fn media_command(v: &Value) -> Value {
    match v["type"].as_str() {
        Some("candidate") => json!({"type":"candidate","candidate":v["candidate"],"sdpMLineIndex":v["sdpMLineIndex"]}),
        Some("answer") => json!({"type":"answer","sdp":v["sdp"]}),
        _ => Value::Null,
    }
}
pub fn run(link: &str) -> Result<()> {
    let (origin, token) = validate_link(link)?;
    let listener = TcpListener::bind(("127.0.0.1", PORT)).context("Pulse Helper is already in use")?;
    listener.set_nonblocking(true)?;
    let expires = Instant::now() + Duration::from_secs(45);
    let mut socket = loop {
        if Instant::now() > expires { bail!("Pulse did not connect"); }
        let (tcp, _) = match listener.accept() { Ok(v) => v, Err(e) if e.kind()==std::io::ErrorKind::WouldBlock => { std::thread::sleep(Duration::from_millis(50)); continue }, Err(e) => return Err(e.into()) };
        tcp.set_read_timeout(Some(Duration::from_secs(2)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(2)))?;
        let config = WebSocketConfig::default().max_message_size(Some(70_000)).max_frame_size(Some(70_000));
        let result = accept_hdr_with_config(tcp, |req: &Request, mut response: Response| {
            let expected = format!("pulse-v1, {token}");
            if req.uri().path() != "/pulse" || req.headers().get("host").and_then(|h|h.to_str().ok()) != Some("127.0.0.1:37654") || req.headers().get("origin").and_then(|h|h.to_str().ok()) != Some(origin.as_str()) || req.headers().get("sec-websocket-protocol").and_then(|h|h.to_str().ok()) != Some(expected.as_str()) {
                return Err(tungstenite::http::Response::builder().status(403).body(Some("Forbidden".into())).unwrap());
            }
            response.headers_mut().insert("sec-websocket-protocol", "pulse-v1".parse().unwrap());
            Ok(response)
        }, Some(config));
        if let Ok(ws) = result { break ws; }
    };
    drop(listener); // A launch accepts exactly one controlling tab.
    // The share action in Pulse starts this launch; the authenticated bridge is ready immediately.
    socket.get_mut().set_nonblocking(true)?;
    socket.send(Message::Text(json!({"type":"ready","protocol":1,"version":env!("CARGO_PKG_VERSION"),"height":1080,"fps":60}).to_string().into()))?;
    let (tx, rx) = mpsc::sync_channel::<Value>(256);
    let mut workers = HashMap::<String, Worker>::new();
    let mut heartbeat = Instant::now();
    loop {
        if heartbeat.elapsed() > Duration::from_secs(15) { break; }
        match socket.read() {
            Ok(Message::Text(raw)) => {
                let v: Value = serde_json::from_str(&raw)?;
                match v["type"].as_str().context("Missing command")? {
                    "ping" => { heartbeat=Instant::now(); socket.send(Message::Text("{\"type\":\"pong\"}".into()))?; }
                    "stop" => break,
                    "removePeer" => { if let Some(peer)=v["peer"].as_str() { workers.remove(peer); } }
                    "addPeer" => {
                        let peer=v["peer"].as_str().context("Missing peer")?;
                        if peer.len()>64 || !peer.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'-') { bail!("Invalid peer"); }
                        if workers.contains_key(peer) { continue; }
                        if workers.len()>=8 { socket.send(Message::Text(json!({"type":"error","message":"This helper supports up to eight simultaneous viewers."}).to_string().into()))?; continue; }
                        let mut command=Command::new(std::env::current_exe()?); command.arg("--media").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
                        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
                        let mut child=command.spawn()?;
                        let mut input=child.stdin.take().context("Missing child input")?;
                        let output=child.stdout.take().context("Missing child output")?;
                        writeln!(input,"{}",json!({"type":"start","monitor":-1,"audio":true,"wgc":false}))?;
                        let tx=tx.clone(); let id=peer.to_string();
                        std::thread::spawn(move || { let mut reader=BufReader::new(output); loop { let mut bytes=Vec::new(); match std::io::Read::take(&mut reader,70_001).read_until(b'\n',&mut bytes) {Ok(n) if n>0 && n<=70_000=>{},_=>break}; if let Ok(mut v)=serde_json::from_slice::<Value>(&bytes) {v["peer"]=json!(id); if tx.send(v).is_err(){break}} } });
                        workers.insert(peer.to_string(),Worker{child,input});
                    }
                    "answer" | "candidate" => {
                        let peer=v["peer"].as_str().context("Missing peer")?;
                        if let Some(worker)=workers.get_mut(peer) { let command=media_command(&v); writeln!(worker.input,"{command}")?; }
                    }
                    _ => bail!("Unknown command"),
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {},
            Err(tungstenite::Error::Io(e)) if e.kind()==std::io::ErrorKind::WouldBlock => {},
            Err(_) => break,
        }
        while let Ok(v)=rx.try_recv() { if workers.contains_key(v["peer"].as_str().unwrap_or("")) {socket.send(Message::Text(v.to_string().into()))?;} }
        let _=socket.flush();
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(workers); // Explicit stop, closed tab, browser crash, timeout, or malformed input.
    Ok(())
}
#[cfg(test)] mod tests { use super::*; #[test] fn links_are_narrow() {let token="a".repeat(64);assert!(validate_link(&format!("pulse-capture://share?origin=https%3A%2F%2Fpulse.to-unknown.com&token={token}")).is_ok());assert!(validate_link(&format!("pulse-capture://share/?origin=https%3A%2F%2Fpulse.to-unknown.com&token={token}")).is_ok());assert!(validate_link(&format!("pulse-capture://share/other?origin=https%3A%2F%2Fpulse.to-unknown.com&token={token}")).is_err());for bad in ["https://evil.test","https://pulse.to-unknown.com.evil.test","http://pulse.to-unknown.com"]{assert!(validate_link(&format!("pulse-capture://share?origin={bad}&token={token}")).is_err());}assert!(validate_link("pulse-capture://share?origin=https://pulse.to-unknown.com&token=x").is_err());} }

#[cfg(test)] mod bridge_tests {
 use super::*;
 #[test] fn browser_ice_metadata_is_not_forwarded_to_strict_media_protocol() {
  let input=json!({"type":"candidate","peer":"abc","candidate":"candidate:1","sdpMid":"0","sdpMLineIndex":0,"usernameFragment":"ice"});
  let output=media_command(&input);
  assert!(serde_json::from_value::<crate::Command>(output).is_ok());
 }
}
