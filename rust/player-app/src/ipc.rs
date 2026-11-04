use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use player_lua::NativeValue;
use player_lua::utils::{format_json, parse_json};

pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    client: Option<Client>,
}

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl IpcServer {
    pub fn bind(path: &Path) -> Result<Self, String> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("cannot bind IPC socket {}: {error}", path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot make IPC socket nonblocking: {error}"))?;
        Ok(IpcServer {
            listener,
            path: path.to_owned(),
            client: None,
        })
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
        while let Some(newline) = client.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = client.buffer.drain(..=newline).collect();
            let Ok(text) = std::str::from_utf8(&line[..line.len() - 1]) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            match parse_json(text, false) {
                Ok((value, _)) => messages.push(value),
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
        let Ok(mut text) = format_json(value) else {
            return;
        };
        text.push('\n');
        if client.stream.write_all(text.as_bytes()).is_err() {
            self.client = None;
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.client = None;
        let _ = std::fs::remove_file(&self.path);
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
