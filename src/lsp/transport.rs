use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use serde_json::Value;

use crate::config::LspServerConfig;

pub struct LspTransport {
    outgoing: Sender<Value>,
    incoming: Receiver<Value>,
    #[allow(dead_code)]
    child: Child,
}

impl LspTransport {
    pub fn spawn(config: &LspServerConfig) -> io::Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing LSP stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing LSP stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut sink = Vec::new();
                let _ = reader.read_to_end(&mut sink);
            });
        }

        let (outgoing_tx, outgoing_rx) = mpsc::channel::<Value>();
        let (incoming_tx, incoming_rx) = mpsc::channel::<Value>();

        thread::spawn(move || {
            while let Ok(message) = outgoing_rx.recv() {
                if write_frame(&mut stdin, &message).is_err() {
                    break;
                }
            }
        });

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(message)) => {
                        if incoming_tx.send(message).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            outgoing: outgoing_tx,
            incoming: incoming_rx,
            child,
        })
    }

    pub fn send(&self, message: Value) {
        let _ = self.outgoing.send(message);
    }

    pub fn try_recv(&self) -> Option<Value> {
        self.incoming.try_recv().ok()
    }
}

fn write_frame(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let payload = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    writer.flush()
}

fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_len = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_len = value.trim().parse::<usize>().ok();
        }
    }

    let Some(content_len) = content_len else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length",
        ));
    };
    let mut payload = vec![0; content_len];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}
