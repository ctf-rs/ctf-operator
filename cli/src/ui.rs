use std::{
    future::Future,
    io::{self, IsTerminal},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use parking_lot::Mutex;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap},
    Frame,
};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug)]
struct State {
    tasks: Vec<TaskState>,
    messages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Ui {
    state: Arc<Mutex<State>>,
    cancel: watch::Sender<bool>,
    interactive: bool,
}

#[derive(Debug, Clone)]
pub struct Task {
    ui: Ui,
    index: usize,
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl Ui {
    #[must_use]
    pub fn new(plain: bool) -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            state: Arc::new(Mutex::new(State {
                tasks: Vec::new(),
                messages: Vec::new(),
            })),
            cancel,
            interactive: !plain && io::stdout().is_terminal() && io::stdin().is_terminal(),
        }
    }

    #[must_use]
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel.subscribe()
    }

    pub fn message(&self, message: impl Into<String>) {
        let message = message.into();
        if !self.interactive {
            eprintln!("{message}");
        }
        self.state.lock().messages.push(message);
    }

    pub fn task(&self, name: impl Into<String>) -> Task {
        let name = name.into();
        if !self.interactive {
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

    pub async fn execute(&self, future: impl Future<Output = Result<()>>) -> Result<()> {
        tokio::pin!(future);
        let result = if self.interactive {
            let mut terminal = ratatui::try_init()?;
            let guard = RestoreTerminal;
            let mut selected = 0_usize;
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            let result = loop {
                tokio::select! {
                    result = &mut future => break result,
                    signal = tokio::signal::ctrl_c() => {
                        signal?;
                        self.cancel.send_replace(true);
                    }
                    _ = interval.tick() => {
                        if event::poll(Duration::ZERO)? {
                            if let Event::Key(key) = event::read()? {
                                match key.code {
                                    KeyCode::Char('q') => { self.cancel.send_replace(true); }
                                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        self.cancel.send_replace(true);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        selected = selected.saturating_add(1)
                                            .min(self.state.lock().tasks.len().saturating_sub(1));
                                    }
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        selected = selected.saturating_sub(1);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        terminal.draw(|frame| self.draw(frame, selected))?;
                    }
                }
            };
            drop(guard);
            self.summary();
            result
        } else {
            tokio::select! {
                result = &mut future => result,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    self.cancel.send_replace(true);
                    future.await
                }
            }
        };
        result
    }

    fn draw(&self, frame: &mut Frame<'_>, selected: usize) {
        let state = self.state.lock();
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Percentage(45),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(frame.area());
        frame.render_widget(
            Paragraph::new("ctf  /  local builds and external tests")
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::ALL)),
            areas[0],
        );
        let rows = state.tasks.iter().enumerate().map(|(index, task)| {
            let (label, color) = status_display(task.status);
            Row::new([
                Cell::from(if index == selected { ">" } else { " " }),
                Cell::from(label).style(Style::default().fg(color)),
                Cell::from(task.name.clone()),
                Cell::from(format!(
                    "{:.1}s",
                    task.elapsed
                        .unwrap_or_else(|| task.started.elapsed())
                        .as_secs_f64()
                )),
                Cell::from(task.message.clone()),
            ])
        });
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(1),
                    Constraint::Length(8),
                    Constraint::Percentage(35),
                    Constraint::Length(9),
                    Constraint::Fill(1),
                ],
            )
            .block(Block::default().borders(Borders::ALL).title("Tasks"))
            .header(Row::new(["", "State", "Task", "Elapsed", "Progress"])),
            areas[1],
        );
        let selected_task = state.tasks.get(selected);
        #[expect(
            clippy::cast_precision_loss,
            reason = "the ratio only drives a progress gauge"
        )]
        let (ratio, label) = selected_task
            .and_then(|task| task.progress)
            .filter(|(_, total)| *total > 0)
            .map_or_else(
                || (0.0, "Waiting for measurable progress".to_owned()),
                |(current, total)| {
                    (
                        (current as f64 / total as f64).clamp(0.0, 1.0),
                        format!("{current} / {total}"),
                    )
                },
            );
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(label)
                .gauge_style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL)),
            areas[2],
        );
        let lines = selected_task.map_or(&state.messages, |task| &task.logs);
        let visible = usize::from(areas[3].height.saturating_sub(2));
        let logs = lines
            .iter()
            .skip(lines.len().saturating_sub(visible))
            .map(|line| Line::from(line.as_str()))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(logs)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title("Live logs")),
            areas[3],
        );
        frame.render_widget(
            Paragraph::new("j/k select task  |  q / Ctrl-C cancel"),
            areas[4],
        );
    }

    fn summary(&self) {
        let state = self.state.lock();
        for task in &state.tasks {
            let (label, _) = status_display(task.status);
            eprintln!("[{label}] {} {}", task.name, task.message);
            if matches!(task.status, Status::Failed) {
                for line in &task.logs {
                    eprintln!("{line}");
                }
            }
        }
        for message in &state.messages {
            eprintln!("{message}");
        }
    }
}

impl Task {
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        let mut state = self.ui.state.lock();
        let task = &mut state.tasks[self.index];
        if !self.ui.interactive {
            eprintln!("[{}] {line}", task.name);
        }
        task.logs.push(line);
    }

    pub fn message(&self, message: impl Into<String>) {
        self.ui.state.lock().tasks[self.index].message = message.into();
    }

    pub fn progress(&self, current: u64, total: u64) {
        self.ui.state.lock().tasks[self.index].progress = Some((current, total));
    }

    pub fn pass(&self, message: impl Into<String>) {
        self.finish(Status::Passed, message.into());
    }

    pub fn fail(&self, message: impl Into<String>) {
        self.finish(Status::Failed, message.into());
    }

    pub fn skip(&self, message: impl Into<String>) {
        self.finish(Status::Skipped, message.into());
    }

    fn finish(&self, status: Status, message: String) {
        let mut state = self.ui.state.lock();
        let task = &mut state.tasks[self.index];
        task.status = status;
        task.elapsed = Some(task.started.elapsed());
        task.message = message;
        if !self.ui.interactive {
            eprintln!(
                "[{}] {} {}",
                status_display(status).0,
                task.name,
                task.message
            );
        }
    }
}

fn status_display(status: Status) -> (&'static str, Color) {
    match status {
        Status::Running => ("RUNNING", Color::Cyan),
        Status::Passed => ("PASSED", Color::Green),
        Status::Failed => ("FAILED", Color::Red),
        Status::Skipped => ("SKIPPED", Color::Yellow),
    }
}
