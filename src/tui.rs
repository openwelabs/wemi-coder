use crate::{agent::{Agent, AgentEvent, ConfirmRequest}, model::ModelClient};
use anyhow::{Context, Result};
use crossterm::{event::{self, Event, KeyCode, KeyEvent, KeyModifiers}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{backend::CrosstermBackend, layout::{Constraint, Direction, Layout}, style::{Modifier, Style}, text::Line, widgets::{Block, Borders, List, ListItem, Paragraph, Wrap}, Terminal};
use std::{io, path::{Path, PathBuf}, time::Duration};
use tokio::sync::mpsc;

pub struct App {
    root: PathBuf,
    api_settings: PathBuf,
    input: String,
    messages: Vec<String>,
    files: Vec<String>,
    status: String,
    spinner: usize,
    agent_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    confirm_rx: Option<mpsc::UnboundedReceiver<ConfirmRequest>>,
    pending_confirm: Option<ConfirmRequest>,
    chat_scroll: u16,
    follow_chat: bool,
}

impl App {
    pub fn new(root: PathBuf, api_settings: PathBuf) -> Result<Self> {
        let root = root.canonicalize().context("project directory does not exist")?;
        Ok(Self { files: list_files(&root), root, api_settings, input: String::new(), messages: vec!["wemi-coder ready. Enter a request, or use /open <path>.".into()], status: "Ready".into(), spinner: 0, agent_rx: None, confirm_rx: None, pending_confirm: None, chat_scroll: 0, follow_chat: true })
    }

    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self.event_loop(&mut terminal).await;
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        result
    }

    async fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            if let Some(rx) = &mut self.agent_rx {
                let mut events = Vec::new();
                while let Ok(message) = rx.try_recv() { events.push(message); }
                for message in events { self.handle_agent_event(message); }
            }
            if let Some(rx) = &mut self.confirm_rx {
                if self.pending_confirm.is_none() {
                    self.pending_confirm = rx.try_recv().ok();
                }
            }
            self.spinner = self.spinner.wrapping_add(1);
            tokio::time::sleep(Duration::from_millis(80)).await;
            if !event::poll(Duration::from_millis(1))? { continue; }
            match event::read()? {
                Event::Key(key) if self.pending_confirm.is_some() => {
                    if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('n') | KeyCode::Char('N')) {
                        if let Some(request) = self.pending_confirm.take() {
                            let _ = request.response.send(matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')));
                        }
                    }
                }
                Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => break,
                Event::Key(KeyEvent { code: KeyCode::Esc, .. }) => break,
                Event::Key(KeyEvent { code: KeyCode::Char(ch), .. }) => self.input.push(ch),
                Event::Key(KeyEvent { code: KeyCode::Backspace, .. }) => { self.input.pop(); },
                Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                    let input = std::mem::take(&mut self.input);
                    if input.trim().is_empty() { continue; }
                    if let Some(path) = input.strip_prefix("/open ") {
                        match PathBuf::from(path.trim()).canonicalize() {
                            Ok(root) if root.is_dir() => { self.root = root; self.files = list_files(&self.root); self.push_message(format!("Opened {}", self.root.display())); },
                            _ => self.push_message("Cannot open that directory."),
                        }
                        continue;
                    }
                    self.push_message(format!("You: {}", input));
                    if self.agent_rx.is_some() { self.push_message("Agent is still working."); continue; }
                    let model = match ModelClient::from_settings(&self.api_settings) {
                        Ok(model) => model,
                        Err(error) => { self.push_message(format!("Config error: {}", error)); continue; }
                    };
                    let agent = Agent::new(model);
                    let (event_tx, event_rx) = mpsc::unbounded_channel();
                    let (confirm_tx, confirm_rx) = mpsc::unbounded_channel();
                    let root = self.root.clone();
                    tokio::spawn(async move {
                        if let Err(error) = agent.run(&root, input, event_tx.clone(), confirm_tx).await {
                            let _ = event_tx.send(AgentEvent::TextDelta(format!("Error: {}", error)));
                            let _ = event_tx.send(AgentEvent::Done);
                        }
                    });
                    self.agent_rx = Some(event_rx);
                    self.confirm_rx = Some(confirm_rx);
                }
                Event::Key(KeyEvent { code: KeyCode::Up, .. }) => { self.follow_chat = false; self.chat_scroll = self.chat_scroll.saturating_sub(1); },
                Event::Key(KeyEvent { code: KeyCode::Down, .. }) => { self.follow_chat = false; self.chat_scroll = self.chat_scroll.saturating_add(1); },
                Event::Key(KeyEvent { code: KeyCode::PageUp, .. }) => { self.follow_chat = false; self.chat_scroll = self.chat_scroll.saturating_sub(10); },
                Event::Key(KeyEvent { code: KeyCode::PageDown, .. }) => { self.follow_chat = false; self.chat_scroll = self.chat_scroll.saturating_add(10); },
                Event::Key(KeyEvent { code: KeyCode::Home, .. }) => { self.follow_chat = false; self.chat_scroll = 0; },
                Event::Key(KeyEvent { code: KeyCode::End, .. }) => { self.follow_chat = true; },
                _ => {}
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let outer = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(1), Constraint::Length(3)]).split(frame.area());
        let columns = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(30), Constraint::Min(1)]).split(outer[0]);
        let items: Vec<ListItem> = self.files.iter().map(|file| ListItem::new(file.as_str())).collect();
        frame.render_widget(List::new(items).block(Block::default().title(format!(" Files: {} ", self.root.display())).borders(Borders::ALL)).highlight_style(Style::default().add_modifier(Modifier::REVERSED)), columns[0]);
        let text: Vec<Line> = self.messages.iter().flat_map(|message| message.lines().map(|line| Line::from(line.to_string()))).collect();
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let viewport = columns[1].height.saturating_sub(2);
        let content_height = visual_line_count(&self.messages, columns[1].width);
        let max_scroll = content_height.saturating_sub(viewport as usize) as u16;
        let scroll = if self.follow_chat { max_scroll } else { self.chat_scroll.min(max_scroll) };
        let position = if max_scroll == 0 { "bottom".into() } else if self.follow_chat { "bottom".into() } else { format!("line {}/{}", scroll.saturating_add(1), max_scroll.saturating_add(1)) };
        frame.render_widget(paragraph.scroll((scroll, 0)).block(Block::default().title(format!(" AI conversation | {} ", position)).borders(Borders::ALL)), columns[1]);
        let status = if let Some(request) = &self.pending_confirm {
            format!("{}  [y/n]\n{}", self.status, request.question)
        } else {
            format!("{}{}", self.status, if self.agent_rx.is_some() { [" |", "/", "-", "\\" ][self.spinner % 4] } else { "" })
        };
        frame.render_widget(Paragraph::new(format!("{}\n{}", self.input, status)).block(Block::default().title(" Input (Enter send, Ctrl-C quit) ").borders(Borders::ALL)), outer[1]);
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Thinking { turn } => self.status = format!("Thinking (loop {})", turn),
            AgentEvent::TextDelta(text) => {
                self.status = "AI response".into();
                if let Some(last) = self.messages.last_mut() {
                    if last.starts_with("AI: ") { last.push_str(&text); } else { self.push_message(format!("AI: {}", text)); }
                } else { self.push_message(format!("AI: {}", text)); }
            }
            AgentEvent::ToolStarted { name } => {
                self.status = match name.as_str() {
                    "read_file" => "Reading...".into(),
                    "write_file" => "Editing...".into(),
                    "shell" => "Running...".into(),
                    "search_files" => "Searching...".into(),
                    _ => format!("Running {}...", name),
                };
            }
            AgentEvent::ToolFinished { name, success, summary } => {
                self.status = format!("{} {}", if success { "Done:" } else { "Failed:" }, name);
                self.push_message(format!("Tool {} {}: {}", name, if success { "succeeded" } else { "failed" }, summary));
            }
            AgentEvent::Done => {
                self.status = "Ready".into();
                self.agent_rx = None;
                self.confirm_rx = None;
            }
        }
    }

    fn push_message(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
        if self.follow_chat {
            self.chat_scroll = u16::MAX;
        }
    }
}

fn list_files(root: &Path) -> Vec<String> {
    walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && !entry.path().to_string_lossy().contains("/target/"))
        .filter_map(|entry| entry.path().strip_prefix(root).ok().map(|p| p.display().to_string()))
        .take(500).collect()
}

fn visual_line_count(messages: &[String], width: u16) -> usize {
    let width = usize::from(width.max(1));
    messages.iter()
        .flat_map(|message| message.lines())
        .map(|line| line.chars().count().max(1).div_ceil(width))
        .sum()
}
