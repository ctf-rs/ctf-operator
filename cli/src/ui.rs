//! Terminal output for commands.
//!
//! Commands print ordinary inline output. On a terminal, finished tasks become permanent
//! coloured lines and running tasks are shown in a small live region below them with a
//! spinner, progress bar and their latest log line. The terminal is never taken over:
//! no alternate screen, no raw mode, and Ctrl-C stays a normal signal. Without a terminal
//! (or with `--plain`) every event is printed as a plain line.

use std::{
    future::Future,
    io::{self, IsTerminal, Stderr},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use parking_lot::Mutex;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{cursor, execute},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
    Terminal, TerminalOptions, Viewport,
};
use tokio::sync::watch;
use unicode_width::UnicodeWidthChar;

/// Height of the live region showing running tasks.
const LIVE_HEIGHT: u16 = 8;
/// Log lines of a failed task echoed above the live region; complete logs stay on disk.
const FAILURE_TAIL: usize = 15;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BAR_WIDTH: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug)]
struct TaskState {
    name: String,
    status: Status,
    started: Instant,
    elapsed: Option<Duration>,
    message: String,
    progress: Option<(u64, u64)>,
    logs: Vec<String>,
}

#[derive(Debug, Default)]
struct State {
    tasks: Vec<TaskState>,
    /// Finished lines waiting to be printed above the live region.
    pending: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
pub struct Ui {
    state: Arc<Mutex<State>>,
    cancel: watch::Sender<bool>,
    live: bool,
}

#[derive(Debug, Clone)]
pub struct Task {
    ui: Ui,
    index: usize,
}

/// Restores the cursor even if rendering fails part-way.
struct ShowCursor;

impl Drop for ShowCursor {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), cursor::Show);
    }
}

impl Ui {
    #[must_use]
    pub fn new(plain: bool) -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            state: Arc::new(Mutex::new(State::default())),
            cancel,
            live: !plain && io::stderr().is_terminal(),
        }
    }

    #[must_use]
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel.subscribe()
    }

    pub fn message(&self, message: impl Into<String>) {
        let message = message.into();
        if self.live {
            self.state.lock().pending.push(Line::from(message));
        } else {
            eprintln!("{message}");
        }
    }

    pub fn task(&self, name: impl Into<String>) -> Task {
        let name = name.into();
        if !self.live {
            eprintln!("[START] {name}");
        }
        let mut state = self.state.lock();
        let index = state.tasks.len();
        state.tasks.push(TaskState {
            name,
            status: Status::Running,
            started: Instant::now(),
            elapsed: None,
            message: String::new(),
            progress: None,
            logs: Vec::new(),
        });
        Task {
            ui: self.clone(),
            index,
        }
    }

    /// Runs `future` while rendering progress. The first Ctrl-C requests cancellation so the
    /// command can clean up; the command's own result is returned.
    pub async fn execute(&self, future: impl Future<Output = Result<()>>) -> Result<()> {
        tokio::pin!(future);
        if !self.live {
            return tokio::select! {
                result = &mut future => result,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    self.cancel.send_replace(true);
                    future.await
                }
            };
        }

        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(io::stderr()),
            TerminalOptions {
                viewport: Viewport::Inline(LIVE_HEIGHT),
            },
        )?;
        let _cursor = ShowCursor;
        execute!(io::stderr(), cursor::Hide)?;
        let mut interval = tokio::time::interval(Duration::from_millis(80));
        let result = loop {
            tokio::select! {
                result = &mut future => break result,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    self.cancel.send_replace(true);
                    self.message("Cancelling; cleaning up owned resources...".yellow().to_string());
                }
                _ = interval.tick() => self.render(&mut terminal)?,
            }
        };
        let summary = self.summary();
        self.state.lock().pending.push(summary);
        self.flush(&mut terminal)?;
        terminal.clear()?;
        let top = terminal.get_frame().area().top();
        terminal.set_cursor_position((0, top))?;
        result
    }

    fn render(&self, terminal: &mut Terminal<CrosstermBackend<Stderr>>) -> io::Result<()> {
        self.flush(terminal)?;
        let state = self.state.lock();
        terminal.draw(|frame| {
            let area = frame.area();
            let width = usize::from(area.width);
            let running = state
                .tasks
                .iter()
                .filter(|task| task.status == Status::Running)
                .collect::<Vec<_>>();
            let mut lines = Vec::new();
            for task in &running {
                lines.push(running_line(task, width));
                if let Some(log) = task.logs.last() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", truncate(log, width.saturating_sub(2))),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            let capacity = usize::from(area.height);
            if lines.len() > capacity {
                lines.truncate(capacity.saturating_sub(1));
                lines.push(Line::from(
                    format!("  … {} tasks running", running.len()).dark_gray(),
                ));
            }
            Paragraph::new(lines).render(area, frame.buffer_mut());
        })?;
        Ok(())
    }

    /// Prints finished lines permanently above the live region.
    fn flush(&self, terminal: &mut Terminal<CrosstermBackend<Stderr>>) -> io::Result<()> {
        let pending = std::mem::take(&mut self.state.lock().pending);
        if pending.is_empty() {
            return Ok(());
        }
        let width = usize::from(terminal.size()?.width.max(1));
        let lines = pending
            .into_iter()
            .flat_map(|line| wrap(line, width))
            .collect::<Vec<_>>();
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        terminal.insert_before(height, |buffer| {
            Paragraph::new(lines).render(buffer.area, buffer);
        })
    }

    fn summary(&self) -> Line<'static> {
        let state = self.state.lock();
        let count = |status| {
            state
                .tasks
                .iter()
                .filter(|task| task.status == status)
                .count()
        };
        let mut spans = vec![Span::raw("  ")];
        for (status, label, color) in [
            (Status::Passed, "passed", Color::Green),
            (Status::Failed, "failed", Color::Red),
            (Status::Skipped, "skipped", Color::Yellow),
        ] {
            let n = count(status);
            let style = if n > 0 {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!("{n} {label}"), style));
            spans.push(Span::raw("  "));
        }
        Line::from(spans)
    }
}

impl Task {
    pub fn log(&self, line: impl Into<String>) {
        let line = sanitize(&line.into());
        let mut state = self.ui.state.lock();
        let task = &mut state.tasks[self.index];
        if !self.ui.live {
            eprintln!("[{}] {line}", task.name);
        }
        task.logs.push(line);
    }

    pub fn message(&self, message: impl Into<String>) {
        self.ui.state.lock().tasks[self.index].message = sanitize(&message.into());
    }

    pub fn progress(&self, current: u64, total: u64) {
        self.ui.state.lock().tasks[self.index].progress = Some((current, total));
    }

    pub fn pass(&self, message: impl Into<String>) {
        self.finish(Status::Passed, &message.into());
    }

    pub fn fail(&self, message: impl Into<String>) {
        self.finish(Status::Failed, &message.into());
    }

    pub fn skip(&self, message: impl Into<String>) {
        self.finish(Status::Skipped, &message.into());
    }

    fn finish(&self, status: Status, message: &str) {
        let mut state = self.ui.state.lock();
        let task = &mut state.tasks[self.index];
        task.status = status;
        task.elapsed = Some(task.started.elapsed());
        task.message = sanitize(message);
        if !self.ui.live {
            eprintln!("[{}] {} {}", plain_label(status), task.name, task.message);
            return;
        }
        let mut lines = vec![finished_line(task)];
        if status == Status::Failed {
            let skip = task.logs.len().saturating_sub(FAILURE_TAIL);
            lines.extend(task.logs.iter().skip(skip).map(|log| {
                Line::from(Span::styled(
                    format!("    │ {log}"),
                    Style::default().fg(Color::DarkGray),
                ))
            }));
        }
        state.pending.extend(lines);
    }
}

fn running_line(task: &TaskState, width: usize) -> Line<'static> {
    let elapsed = task.started.elapsed();
    let frame = SPINNER[usize::try_from(elapsed.as_millis() / 80).unwrap_or(0) % SPINNER.len()];
    let mut spans = vec![
        Span::styled(format!("{frame} "), Style::default().fg(Color::Cyan)),
        Span::styled(
            task.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    if let Some((current, total)) = task.progress.filter(|(_, total)| *total > 0) {
        spans.extend(bar(current, total));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!("{:.1}s ", elapsed.as_secs_f64()),
        Style::default().fg(Color::DarkGray),
    ));
    let used = spans.iter().map(Span::width).sum::<usize>();
    spans.push(Span::raw(truncate(
        &task.message,
        width.saturating_sub(used),
    )));
    Line::from(spans)
}

fn finished_line(task: &TaskState) -> Line<'static> {
    let (icon, color) = match task.status {
        Status::Passed => ("✓", Color::Green),
        Status::Failed => ("✗", Color::Red),
        Status::Skipped => ("○", Color::Yellow),
        Status::Running => ("•", Color::Cyan),
    };
    let mut spans = vec![
        Span::styled(
            format!("{icon} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            task.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {:.1}s", task.elapsed.unwrap_or_default().as_secs_f64()),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if !task.message.is_empty() {
        let style = if task.status == Status::Failed {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        spans.push(Span::raw("  "));
        spans.push(Span::styled(task.message.clone(), style));
    }
    Line::from(spans)
}

/// A `━━━━━╺───── 12/40` style bar.
fn bar(current: u64, total: u64) -> Vec<Span<'static>> {
    let current = current.min(total);
    let filled =
        usize::try_from(current.saturating_mul(BAR_WIDTH as u64) / total).unwrap_or(BAR_WIDTH);
    let rest = BAR_WIDTH - filled;
    vec![
        Span::styled("━".repeat(filled), Style::default().fg(Color::Cyan)),
        Span::styled(
            if rest > 0 {
                format!("╺{}", "━".repeat(rest - 1))
            } else {
                String::new()
            },
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(" {current}/{total}"),
            Style::default().fg(Color::DarkGray),
        ),
    ]
}

/// Splits a styled line into rows of at most `width` display columns.
fn wrap(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    for span in line.spans {
        let mut chunk = String::new();
        for ch in span.content.chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > width && used > 0 {
                if !chunk.is_empty() {
                    if let Some(row) = rows.last_mut() {
                        row.push(Span::styled(std::mem::take(&mut chunk), span.style));
                    }
                }
                rows.push(Vec::new());
                used = 0;
            }
            chunk.push(ch);
            used += w;
        }
        if !chunk.is_empty() {
            if let Some(row) = rows.last_mut() {
                row.push(Span::styled(chunk, span.style));
            }
        }
    }
    rows.into_iter().map(Line::from).collect()
}

fn truncate(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > width {
            if width > 0 {
                out.pop();
                out.push('…');
            }
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

/// Removes control characters (including ANSI escapes from tool output) that would
/// corrupt the rendered region.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
            }
            '\t' => out.push_str("    "),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

const fn plain_label(status: Status) -> &'static str {
    match status {
        Status::Running => "RUNNING",
        Status::Passed => "PASSED",
        Status::Failed => "FAILED",
        Status::Skipped => "SKIPPED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_ansi_and_control_characters() {
        assert_eq!(sanitize("\u{1b}[31mred\u{1b}[0m\rok\u{7}"), "redok");
    }

    #[test]
    fn wrap_and_truncate_respect_display_width() {
        let rows = wrap(Line::from("abcdef"), 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].width(), 2);
        assert_eq!(truncate("🍌🍌🍌", 4), "🍌…");
    }

    #[test]
    fn finishing_a_live_task_queues_its_line_and_summarizes() {
        let ui = Ui {
            state: Arc::new(Mutex::new(State::default())),
            cancel: watch::channel(false).0,
            live: true,
        };
        ui.task("build").pass("Ready");
        ui.task("solve").fail("exit 1");
        let summary = ui.summary();
        assert_eq!(ui.state.lock().pending.len(), 2);
        assert!(summary.to_string().contains("1 passed"));
        assert!(summary.to_string().contains("1 failed"));
    }

    #[test]
    fn bar_reflects_progress() {
        let filled = bar(12, 24)[0].width();
        assert_eq!(filled, BAR_WIDTH / 2);
    }
}
