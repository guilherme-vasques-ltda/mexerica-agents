//! Framing por evento, taxa temporal e leitor único do GET legado com backpressure.
use super::super::entrada::MAX_FRAME;
use crate::LockExt;
use anyhow::{Result, bail};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};

pub(super) type SharedRate = Arc<Mutex<Rate>>;

pub(super) struct Rate {
    pub started: Instant,
    bytes: usize,
    events: usize,
    reverse: usize,
}

impl Default for Rate {
    fn default() -> Self {
        Self { started: Instant::now(), bytes: 0, events: 0, reverse: 0 }
    }
}

impl Rate {
    pub fn charge_at(&mut self, now: Instant, bytes: usize, events: usize, reverse: usize) -> Result<()> {
        if now.duration_since(self.started) >= Duration::from_secs(1) {
            *self = Self { started: now, bytes: 0, events: 0, reverse: 0 };
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.events = self.events.saturating_add(events);
        self.reverse = self.reverse.saturating_add(reverse);
        if self.bytes > 256 * 1024 || self.events > 64 || self.reverse > 8 {
            bail!("Taxa MCP excedida; conteúdo retido");
        }
        Ok(())
    }
}

pub(super) fn charge(rate: &SharedRate, bytes: usize, events: usize, reverse: usize) -> Result<()> {
    rate.locked().charge_at(Instant::now(), bytes, events, reverse)
}

pub(super) struct Events {
    response: reqwest::Response,
    parser: Parser,
    chunk: Vec<u8>,
    offset: usize,
    rate: SharedRate,
}

#[derive(Default)]
pub(super) struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        if self.buffer.len() + bytes.len() > MAX_FRAME {
            bail!("Frame SSE acima de 64 KiB; conteúdo retido");
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Option<String>> {
        let mut start = 0;
        let mut cursor = 0;
        let mut boundary = None;
        while cursor < self.buffer.len() {
            if !matches!(self.buffer[cursor], b'\r' | b'\n') {
                cursor += 1;
                continue;
            }
            if self.buffer[cursor] == b'\r' && cursor + 1 == self.buffer.len() {
                break;
            }
            let length = if self.buffer[cursor..].starts_with(b"\r\n") { 2 } else { 1 };
            if cursor == start {
                boundary = Some(cursor + length);
                break;
            }
            cursor += length;
            start = cursor;
        }
        let Some(end) = boundary else { return Ok(None) };
        let event = String::from_utf8(self.buffer.drain(..end).collect())
            .map_err(|_| anyhow::anyhow!("UTF-8 SSE inválido; conteúdo retido"))?;
        Ok(Some(event))
    }

    fn finish(&self) -> Result<()> {
        if !self.buffer.iter().all(u8::is_ascii_whitespace) {
            bail!("SSE terminou com frame incompleto; conteúdo retido");
        }
        Ok(())
    }
}

impl Events {
    pub fn new(response: reqwest::Response, rate: SharedRate) -> Self {
        Self { response, parser: Parser::default(), chunk: Vec::new(), offset: 0, rate }
    }

    pub async fn next(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(event) = self.parser.pop()? {
                charge(&self.rate, 0, 1, 0)?;
                return Ok(Some(event));
            }
            if self.offset < self.chunk.len() {
                let capacity = MAX_FRAME - self.parser.buffer.len();
                if capacity == 0 {
                    bail!("Frame SSE acima de 64 KiB; conteúdo retido");
                }
                let end = self.chunk.len().min(self.offset + capacity);
                self.parser.push(&self.chunk[self.offset..end])?;
                self.offset = end;
                continue;
            }
            if let Some(chunk) = self.response.chunk().await.map_err(|_| anyhow::anyhow!("Leitura SSE indisponível"))?
            {
                // O agregado HTTP pode conter vários eventos; quota temporal limita a cópia.
                charge(&self.rate, chunk.len(), 0, 0)?;
                self.chunk.clear();
                self.chunk.extend_from_slice(&chunk);
                self.offset = 0;
            } else {
                self.parser.finish()?;
                return Ok(None);
            }
        }
    }
}

pub(super) struct Legacy {
    pub endpoint: reqwest::Url,
    pub receiver: mpsc::Receiver<Result<String>>,
    reader: tokio::task::JoinHandle<()>,
}

impl Legacy {
    pub fn start(endpoint: reqwest::Url, mut events: Events) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        let reader = tokio::spawn(async move {
            loop {
                match events.next().await {
                    Ok(Some(event)) => {
                        if sender.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => return,
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        return;
                    }
                }
            }
        });
        Self { endpoint, receiver, reader }
    }
}

impl Drop for Legacy {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

pub(super) fn text(event: &str) -> String {
    event
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| data.strip_prefix(' ').unwrap_or(data))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn endpoint(event: &str) -> bool {
    event
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .any(|line| line.strip_prefix("event:").is_some_and(|kind| kind.trim() == "endpoint"))
}
