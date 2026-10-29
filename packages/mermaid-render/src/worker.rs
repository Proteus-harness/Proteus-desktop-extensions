use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use proteus_extension::ui;
use serde_json::Value;

const MAX_REPLY: usize = 2 * 1024 * 1024 + 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Request {
    line: String,
    reply: SyncSender<Result<Value>>,
}

pub(super) struct Worker {
    child: Child,
    requests: Option<SyncSender<Request>>,
    io_thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(script: &Path) -> Result<Self> {
        ensure!(
            script.is_file(),
            "Mermaid worker.mjs is missing from the package"
        );
        let mut child = Command::new("node")
            .args(["--max-old-space-size=96", "--no-addons"])
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Mermaid requires Node.js >= 20 on PATH")?;
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (requests, receiver) = mpsc::sync_channel::<Request>(1);
        let io_thread = thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                let result = (|| {
                    writeln!(stdin, "{}", request.line)?;
                    stdin.flush()?;
                    let line = bounded_line(&mut stdout)?;
                    Ok(serde_json::from_slice(&line)?)
                })();
                let failed = result.is_err();
                let _ = request.reply.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            requests: Some(requests),
            io_thread: Some(io_thread),
        })
    }

    pub fn render(&mut self, line: &str) -> Result<Value> {
        let (sender, reply) = mpsc::sync_channel(1);
        self.requests
            .as_ref()
            .unwrap()
            .send(Request {
                line: line.to_owned(),
                reply: sender,
            })
            .map_err(|_| anyhow!("Mermaid worker stopped"))?;
        let result = reply
            .recv_timeout(TIMEOUT)
            .context("Mermaid worker timed out or stopped")??;
        ensure!(
            result["ok"].as_bool() == Some(true),
            "{}",
            result["error"]
                .as_str()
                .unwrap_or("Mermaid rendering failed")
        );
        let svg = result["svg"]
            .as_str()
            .ok_or_else(|| anyhow!("Mermaid SVG is missing"))?;
        ensure!(
            svg.len() <= 1024 * 1024 && svg.starts_with("<svg"),
            "invalid Mermaid SVG output"
        );
        let width = result["width"]
            .as_f64()
            .ok_or_else(|| anyhow!("Mermaid width is missing"))?;
        let height = result["height"]
            .as_f64()
            .ok_or_else(|| anyhow!("Mermaid height is missing"))?;
        ensure!(
            [width, height]
                .iter()
                .all(|value| value.is_finite() && (1.0..=4096.0).contains(value)),
            "invalid Mermaid dimensions"
        );
        Ok(ui::svg(svg, width, height))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.requests.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.io_thread.take() {
            let _ = thread.join();
        }
    }
}

fn bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            bail!("Mermaid worker closed its output");
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let take = end.map_or(available.len(), |index| index + 1);
        ensure!(
            line.len() + take <= MAX_REPLY,
            "Mermaid worker response is too large"
        );
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if end.is_some() {
            return Ok(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterminated_or_oversized_worker_output_is_rejected() {
        assert!(bounded_line(&mut &b"partial"[..]).is_err());
        assert!(bounded_line(&mut vec![b'x'; MAX_REPLY + 1].as_slice()).is_err());
        assert_eq!(bounded_line(&mut &b"{}\nremaining"[..]).unwrap(), b"{}\n");
    }
}
