use std::{collections::BTreeMap, path::Path, process::Stdio};

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::Deserialize;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
};

use crate::ui::{Task, Ui};

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Text,
    BuildKit,
}

pub async fn run(
    mut command: Command,
    ui: &Ui,
    task: &Task,
    format: Format,
    log_path: &Path,
) -> Result<()> {
    if *ui.cancellation().borrow() {
        bail!("operation cancelled");
    }
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let mut child = command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {program}"))?;
    let stdout = child.stdout.take().context("missing child stdout")?;
    let stderr = child.stderr.take().context("missing child stderr")?;
    let stdout_path = log_path.with_extension("stdout.log");
    let stderr_path = log_path.with_extension("stderr.log");
    let mut cancellation = ui.cancellation();
    let wait = async {
        if *cancellation.borrow() {
            child
                .kill()
                .await
                .context("could not stop cancelled child")?;
            bail!("operation cancelled");
        }
        tokio::select! {
            status = child.wait() => status.context("could not wait for child"),
            _ = cancellation.changed() => {
                child.kill().await.context("could not stop cancelled child")?;
                let _ = child.wait().await?;
                bail!("operation cancelled")
            }
        }
    };
    let (status, (), ()) = tokio::try_join!(
        wait,
        drain(stdout, task, Format::Text, &stdout_path),
        drain(stderr, task, format, &stderr_path),
    )?;
    if !status.success() {
        bail!(
            "{program} exited with {status}; logs: {}",
            log_path.display()
        );
    }
    Ok(())
}

async fn drain(
    reader: impl AsyncRead + Unpin,
    task: &Task,
    format: Format,
    path: &Path,
) -> Result<()> {
    let mut reader = BufReader::new(reader);
    let mut log = File::create(path)
        .await
        .with_context(|| format!("could not create log {}", path.display()))?;
    let mut line = Vec::new();
    let mut progress = BuildKitProgress::default();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).await? == 0 {
            break;
        }
        log.write_all(&line).await?;
        let text = String::from_utf8_lossy(&line);
        match format {
            Format::Text => task.log(text.trim_end().to_owned()),
            Format::BuildKit => progress.render(text.trim_end(), task),
        }
        progress.finish(task);
    }
    log.flush().await?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SolveStatus {
    vertexes: Vec<Vertex>,
    statuses: Vec<VertexStatus>,
    logs: Vec<VertexLog>,
    warnings: Vec<VertexWarning>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Vertex {
    digest: String,
    name: String,
    cached: bool,
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VertexStatus {
    name: String,
    current: u64,
    total: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VertexLog {
    vertex: String,
    stream: u8,
    data: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VertexWarning {
    short: String,
    detail: Vec<String>,
    url: String,
}

#[derive(Debug, Default)]
struct BuildKitProgress {
    steps: BTreeMap<String, String>,
    fragments: BTreeMap<(String, u8), Vec<u8>>,
}

impl BuildKitProgress {
    fn render(&mut self, line: &str, task: &Task) {
        if line.is_empty() {
            return;
        }
        let event: SolveStatus = match serde_json::from_str(line) {
            Ok(event) if line.starts_with('{') => event,
            Ok(_) | Err(_) => {
                task.log(line);
                return;
            }
        };
        for vertex in event.vertexes {
            let label = if vertex.cached {
                format!("Cached: {}", vertex.name)
            } else {
                vertex.name
            };
            task.message(&label);
            if self.steps.get(&vertex.digest) != Some(&label) {
                task.log(&label);
                self.steps.insert(vertex.digest, label);
            }
            if let Some(error) = vertex.error {
                task.log(error);
            }
        }
        for status in event.statuses {
            task.message(status.name);
            task.progress(status.current, status.total);
        }
        for log in event.logs {
            match decode(&log.data) {
                Ok(bytes) => {
                    let buffer = self.fragments.entry((log.vertex, log.stream)).or_default();
                    buffer.extend(bytes);
                    while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                        task.log(String::from_utf8_lossy(&buffer[..end]).trim_end_matches('\r'));
                        buffer.drain(..=end);
                    }
                }
                Err(error) => task.log(format!(
                    "Invalid BuildKit log encoding: {error:#}; data={}",
                    log.data
                )),
            }
        }
        for warning in event.warnings {
            for encoded in std::iter::once(warning.short).chain(warning.detail) {
                match decode(&encoded) {
                    Ok(bytes) => task.log(format!(
                        "BuildKit warning: {}",
                        String::from_utf8_lossy(&bytes)
                    )),
                    Err(error) => task.log(format!(
                        "Invalid BuildKit warning encoding: {error:#}; data={encoded}"
                    )),
                }
            }
            if !warning.url.is_empty() {
                task.log(warning.url);
            }
        }
    }

    fn finish(&mut self, task: &Task) {
        for (_, bytes) in std::mem::take(&mut self.fragments) {
            if !bytes.is_empty() {
                task.log(String::from_utf8_lossy(&bytes));
            }
        }
    }
}

fn decode(value: &str) -> Result<Vec<u8>> {
    Ok(base64::engine::general_purpose::STANDARD.decode(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_buildkit_events_preserve_steps_chunks_and_plain_errors() {
        let ui = Ui::new(true);
        let task = ui.task("BuildKit fixture");
        let mut progress = BuildKitProgress::default();
        progress.render(
            r#"{"vertexes":[{"digest":"sha256:step","name":"RUN check","cached":true}]}"#,
            &task,
        );
        progress.render(
            r#"{"logs":[{"vertex":"sha256:step","stream":1,"data":"aGVsbG8g"}]}"#,
            &task,
        );
        progress.render(
            r#"{"logs":[{"vertex":"sha256:step","stream":1,"data":"d29ybGQK"}]}"#,
            &task,
        );
        assert_eq!(progress.steps["sha256:step"], "Cached: RUN check");
        assert!(progress.fragments.values().all(Vec::is_empty));
        progress.render("ERROR: failed to build: process exited with 1", &task);
        progress.finish(&task);
    }
}
