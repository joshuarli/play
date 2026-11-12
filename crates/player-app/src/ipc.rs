use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use crate::value::{format_json, parse_json, NativeValue};

pub struct IpcServer {
    listener: UnixListener,
    client: Option<Client>,
}

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
    outbound: Vec<u8>,
}

impl IpcServer {
    pub fn bind(path: &Path) -> Result<Self, String> {
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("cannot bind IPC socket {}: {error}", path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot make IPC socket nonblocking: {error}"))?;
        Ok(IpcServer { listener, client: None })
    }

    pub fn poll(&mut self) -> Vec<NativeValue> {
        self.accept();
        let mut messages = Vec::new();
        let Some(client) = self.client.as_mut() else {
            return messages;
        };
        let mut chunk = [0u8; 8192];
        let mut disconnected = false;
        loop {
            match client.stream.read(&mut chunk) {
                Ok(0) => {
                    disconnected = true;
                    break;
                }
                Ok(count) => client.buffer.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.client = None;
            return messages;
        }
        client.flush();
        while let Some(newline) = client.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = client.buffer.drain(..=newline).collect();
            let Ok(text) = std::str::from_utf8(&line[..line.len() - 1]) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            match parse_json(text) {
                Ok(value) => messages.push(value),
                Err(_) => messages.push(NativeValue::Map(vec![(
                    "invalid".to_owned(),
                    NativeValue::String(text.to_owned()),
                )])),
            }
        }
        messages
    }

    fn accept(&mut self) {
        if self.client.is_some() {
            return;
        }
        match self.listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(true);
                self.client = Some(Client {
                    stream,
                    buffer: Vec::new(),
                    outbound: Vec::new(),
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
    }

    pub fn connected(&self) -> bool {
        self.client.is_some()
    }

    pub fn send(&mut self, value: &NativeValue) {
        let Some(client) = self.client.as_mut() else {
            return;
        };
        let mut text = match format_json(value) {
            Ok(text) => text,
            Err(_) => {
                eprintln!("player: dropped an IPC message that has no JSON form");
                return;
            }
        };
        text.push('\n');
        client.outbound.extend_from_slice(text.as_bytes());
        client.flush();
        if client.outbound.len() > 8 * 1024 * 1024 {
            self.client = None;
        }
    }
}

impl Client {
    fn flush(&mut self) {
        while !self.outbound.is_empty() {
            match self.stream.write(&self.outbound) {
                Ok(0) => {
                    self.outbound.clear();
                    break;
                }
                Ok(count) => {
                    self.outbound.drain(..count);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.outbound.clear();
                    break;
                }
            }
        }
    }
}

pub fn reply(request_id: i64, result: Result<NativeValue, String>) -> NativeValue {
    let mut fields = Vec::new();
    match result {
        Ok(data) => {
            fields.push(("error".to_owned(), NativeValue::String("success".to_owned())));
            if !matches!(data, NativeValue::None) {
                fields.push(("data".to_owned(), data));
            }
        }
        Err(message) => {
            fields.push(("error".to_owned(), NativeValue::String(message)));
        }
    }
    fields.push(("request_id".to_owned(), NativeValue::Int(request_id)));
    NativeValue::Map(fields)
}

pub fn event(name: &str, fields: Vec<(String, NativeValue)>) -> NativeValue {
    let mut map = vec![("event".to_owned(), NativeValue::String(name.to_owned()))];
    map.extend(fields);
    NativeValue::Map(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.work/tmp");
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join(format!("ipc-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn socket(&self) -> PathBuf { self.0.join("socket") }
    }
    impl Drop for Directory {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }

    #[test]
    fn occupied_paths_survive_failed_binding() {
        let dir = Directory::new();
        let path = dir.socket();
        std::fs::write(&path, b"keep").unwrap();
        assert!(IpcServer::bind(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        std::fs::remove_file(&path).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        assert!(IpcServer::bind(&path).is_err());
        assert!(UnixStream::connect(&path).is_ok());
        drop(listener);
        assert!(IpcServer::bind(&path).is_err(), "stale sockets require explicit removal");
    }

    #[test]
    fn cleanup_preserves_replacement_entries() {
        let dir = Directory::new();
        let path = dir.socket();
        let server = IpcServer::bind(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        drop(server);
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        std::fs::remove_file(&path).unwrap();
        let server = IpcServer::bind(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        drop(server);
        assert!(UnixStream::connect(&path).is_ok());
        drop(replacement);
    }

    #[test]
    fn close_leaves_the_socket_entry_and_binding_rejects_symlinks() {
        let dir = Directory::new();
        let path = dir.socket();
        drop(IpcServer::bind(&path).unwrap());
        assert!(path.exists());
        assert!(UnixStream::connect(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        let target = dir.0.join("target");
        std::fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(IpcServer::bind(&path).is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert!(IpcServer::bind(&dir.0.join("missing/socket")).is_err());
    }
}
