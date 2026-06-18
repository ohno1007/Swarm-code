//! Full-screen TUI chat, in the spirit of Claude Code.
//!
//! Layout: a header bar, a scrollable transcript (streamed assistant text with
//! inline tool-call feedback), and an input box. The agent runs asynchronously;
//! its events stream into the transcript while the UI stays responsive.

use std::path::PathBuf;

use futures::StreamExt;
use ratatui::crossterm::event::{
    Event, EventStream, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use swarm_core::{AgentEvent, SessionManager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User,
    Assistant,
    Info,
    ToolStart,
    ToolOk,
    ToolErr,
}

struct Block_ {
    kind: Kind,
    text: String,
}

/// Messages flowing into the UI loop from the agent task.
enum UiMsg {
    Agent(AgentEvent),
    Done(Result<String, String>),
}

struct App {
    manager: SessionManager,
    model: String,
    current: Uuid,

    blocks: Vec<Block_>,
    /// Index of the assistant block currently being streamed into.
    live: Option<usize>,

    input: String,
    cursor: usize, // char index

    scroll: u16,
    follow: bool,
    busy: bool,
    spinner: usize,

    quit: bool,
}

pub async fn run(workspace: PathBuf) -> anyhow::Result<()> {
    let manager = swarm_core::build_manager(workspace)?;
    let model = manager.swarm().model().to_string();
    let current = manager.create("main").await;

    let mut app = App {
        manager,
        model,
        current,
        blocks: Vec::new(),
        live: None,
        input: String::new(),
        cursor: 0,
        scroll: 0,
        follow: true,
        busy: false,
        spinner: 0,
        quit: false,
    };
    app.info("Welcome to Swarm-code. Type a message and press Enter. /help for commands, Ctrl+C to quit.");

    let mut terminal = ratatui::init();

    // Channels: agent task -> UI.
    let (ui_tx, mut ui_rx): (UnboundedSender<UiMsg>, UnboundedReceiver<UiMsg>) = unbounded_channel();
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));

    let res = loop {
        if let Err(e) = terminal.draw(|f| app.draw(f)) {
            break Err(anyhow::anyhow!(e));
        }
        if app.quit {
            break Ok(());
        }

        tokio::select! {
            maybe_ev = term_events.next() => {
                match maybe_ev {
                    Some(Ok(ev)) => app.on_term_event(ev, &ui_tx).await,
                    Some(Err(_)) | None => break Ok(()),
                }
            }
            Some(msg) = ui_rx.recv() => app.on_ui_msg(msg),
            _ = tick.tick() => { if app.busy { app.spinner = (app.spinner + 1) % SPINNER.len(); } }
        }
    };

    ratatui::restore();
    res
}

impl App {
    fn info(&mut self, text: impl Into<String>) {
        self.blocks.push(Block_ {
            kind: Kind::Info,
            text: text.into(),
        });
        self.follow = true;
    }

    // ---- event handling -------------------------------------------------

    async fn on_term_event(&mut self, ev: Event, ui_tx: &UnboundedSender<UiMsg>) {
        let Event::Key(key) = ev else { return };
        if key.kind != KeyEventKind::Press {
            return; // ignore key-release (matters on Windows)
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('d') if ctrl && self.input.is_empty() => self.quit = true,
            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => {
                if self.cursor < self.input.chars().count() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.chars().count(),
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-10),
            KeyCode::PageDown => self.scroll_by(10),
            KeyCode::Esc => {
                self.input.clear();
                self.cursor = 0;
            }
            KeyCode::Enter => self.submit(ui_tx).await,
            _ => {}
        }
    }

    fn insert(&mut self, c: char) {
        let byte = self.byte_at(self.cursor);
        self.input.insert(byte, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.input.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        let count = self.input.chars().count();
        if self.cursor >= count {
            return;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.input.replace_range(start..end, "");
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.input.len())
    }

    fn scroll_by(&mut self, delta: i32) {
        self.follow = false;
        let new = self.scroll as i32 + delta;
        self.scroll = new.max(0) as u16;
    }

    async fn submit(&mut self, ui_tx: &UnboundedSender<UiMsg>) {
        if self.busy {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.cursor = 0;

        if let Some(cmd) = text.strip_prefix('/') {
            self.command(cmd).await;
            return;
        }

        self.blocks.push(Block_ {
            kind: Kind::User,
            text,
        });
        self.live = None;
        self.busy = true;
        self.follow = true;

        let input = self.last_user_text();
        let id = self.current;
        let manager = self.manager.clone();
        let ui_tx = ui_tx.clone();

        // Per-submit observer channel, forwarded into the UI channel.
        let (obs_tx, mut obs_rx) = unbounded_channel::<AgentEvent>();
        let ui_tx2 = ui_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = obs_rx.recv().await {
                let _ = ui_tx2.send(UiMsg::Agent(ev));
            }
        });
        tokio::spawn(async move {
            let res = manager
                .send_observed(id, input, obs_tx)
                .await
                .map_err(|e| e.to_string());
            let _ = ui_tx.send(UiMsg::Done(res));
        });
    }

    fn last_user_text(&self) -> String {
        self.blocks
            .iter()
            .rev()
            .find(|b| b.kind == Kind::User)
            .map(|b| b.text.clone())
            .unwrap_or_default()
    }

    async fn command(&mut self, cmd: &str) {
        let mut parts = cmd.split_whitespace();
        let name = parts.next().unwrap_or("");
        let rest = parts.collect::<Vec<_>>().join(" ");
        match name {
            "quit" | "exit" | "q" => self.quit = true,
            "help" | "h" => self.info(
                "commands: /new [title]  /sessions  /switch <n>  /help  /quit  •  scroll: ↑/↓ PgUp/PgDn",
            ),
            "new" => {
                let title = if rest.is_empty() { "session".into() } else { rest };
                self.current = self.manager.create(title).await;
                self.info(format!("created session {}", short(self.current)));
            }
            "sessions" => {
                let list = self.manager.list().await;
                let mut s = String::from("sessions:");
                for (i, (id, title, turns)) in list.iter().enumerate() {
                    let mark = if *id == self.current { "*" } else { " " };
                    s.push_str(&format!("\n {mark}[{i}] {} \"{title}\" ({turns} msgs)", short(*id)));
                }
                self.info(s);
            }
            "switch" => match rest.trim().parse::<usize>() {
                Ok(n) => {
                    let list = self.manager.list().await;
                    match list.get(n) {
                        Some((id, title, _)) => {
                            self.current = *id;
                            self.info(format!("switched to [{n}] \"{title}\""));
                        }
                        None => self.info(format!("no session #{n}")),
                    }
                }
                Err(_) => self.info("usage: /switch <number>"),
            },
            other => self.info(format!("unknown command: /{other} (try /help)")),
        }
    }

    fn on_ui_msg(&mut self, msg: UiMsg) {
        match msg {
            UiMsg::Agent(ev) => self.on_agent_event(ev),
            UiMsg::Done(res) => {
                self.busy = false;
                self.live = None;
                if let Err(e) = res {
                    self.info(format!("error: {e}"));
                }
            }
        }
        self.follow = true;
    }

    fn on_agent_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Text(t) => {
                if let Some(i) = self.live {
                    self.blocks[i].text.push_str(&t);
                } else {
                    self.blocks.push(Block_ {
                        kind: Kind::Assistant,
                        text: t,
                    });
                    self.live = Some(self.blocks.len() - 1);
                }
            }
            AgentEvent::ToolStart { name, args } => {
                self.live = None;
                self.blocks.push(Block_ {
                    kind: Kind::ToolStart,
                    text: format!("{name}  {args}"),
                });
            }
            AgentEvent::ToolEnd { name, ok, preview } => {
                self.blocks.push(Block_ {
                    kind: if ok { Kind::ToolOk } else { Kind::ToolErr },
                    text: format!("{name}: {preview}"),
                });
            }
            AgentEvent::Compacted { summarized } => {
                self.info(format!("… compacted {summarized} earlier messages"));
            }
        }
    }

    // ---- rendering ------------------------------------------------------

    fn draw(&mut self, f: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(f.area());
        let (header, body, input) = (areas[0], areas[1], areas[2]);

        self.draw_header(f, header);
        self.draw_body(f, body);
        self.draw_input(f, input);
    }

    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let status = if self.busy {
            format!("{} thinking…", SPINNER[self.spinner])
        } else {
            "ready".to_string()
        };
        let line = Line::from(vec![
            Span::styled(" Swarm-code ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(format!("{}", self.model), Style::default().fg(Color::Magenta)),
            Span::raw("  "),
            Span::styled(format!("session {}", short(self.current)), Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(status, Style::default().fg(if self.busy { Color::Yellow } else { Color::Green })),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }

    fn draw_body(&mut self, f: &mut Frame, area: Rect) {
        let width = area.width.max(1) as usize;
        let lines = render_blocks(&self.blocks, width);
        let total = lines.len() as u16;
        let viewport = area.height;
        let max_scroll = total.saturating_sub(viewport);
        if self.follow {
            self.scroll = max_scroll;
        }
        self.scroll = self.scroll.min(max_scroll);

        let para = Paragraph::new(Text::from(lines)).scroll((self.scroll, 0));
        f.render_widget(para, area);
    }

    fn draw_input(&self, f: &mut Frame, area: Rect) {
        let block = Block::new().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let avail = inner.width.saturating_sub(2) as usize; // "› "
        let chars: Vec<char> = self.input.chars().collect();
        let start = self.cursor.saturating_sub(avail);
        let end = (start + avail).min(chars.len());
        let visible: String = chars[start..end].iter().collect();

        let line = Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(visible),
        ]);
        f.render_widget(Paragraph::new(line), inner);

        let cursor_x = inner.x + 2 + (self.cursor - start) as u16;
        f.set_cursor_position(Position::new(cursor_x.min(inner.x + inner.width.saturating_sub(1)), inner.y));
    }
}

/// Render transcript blocks into wrapped, styled lines.
fn render_blocks(blocks: &[Block_], width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    for b in blocks {
        match b.kind {
            Kind::User => {
                push_wrapped(&mut out, &b.text, width.saturating_sub(2), "› ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD), Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
                out.push(Line::default());
            }
            Kind::Assistant => {
                push_wrapped(&mut out, &b.text, width, "", Style::default(), Style::default());
                out.push(Line::default());
            }
            Kind::Info => {
                push_wrapped(&mut out, &b.text, width, "", Style::default().fg(Color::DarkGray), Style::default().fg(Color::DarkGray));
            }
            Kind::ToolStart => {
                let style = Style::default().fg(Color::Cyan);
                push_wrapped(&mut out, &b.text, width.saturating_sub(2), "⚙ ", style, Style::default().fg(Color::DarkGray));
            }
            Kind::ToolOk => {
                push_wrapped(&mut out, &b.text, width.saturating_sub(2), "✓ ", Style::default().fg(Color::Green), Style::default().fg(Color::DarkGray));
            }
            Kind::ToolErr => {
                push_wrapped(&mut out, &b.text, width.saturating_sub(2), "✗ ", Style::default().fg(Color::Red), Style::default().fg(Color::DarkGray));
            }
        }
    }
    out
}

/// Wrap `text` to `width`, prefixing the first line with `prefix` (styled
/// `prefix_style`) and styling the body with `body_style`.
fn push_wrapped(
    out: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    prefix: &str,
    prefix_style: Style,
    body_style: Style,
) {
    let width = width.max(1);
    let mut first = true;
    for raw_line in text.split('\n') {
        for chunk in wrap_line(raw_line, width) {
            if first {
                first = false;
                if prefix.is_empty() {
                    out.push(Line::from(Span::styled(chunk, body_style)));
                } else {
                    out.push(Line::from(vec![
                        Span::styled(prefix.to_string(), prefix_style),
                        Span::styled(chunk, body_style),
                    ]));
                }
            } else {
                let indent = " ".repeat(prefix.chars().count());
                out.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(chunk, body_style),
                ]));
            }
        }
    }
}

/// Greedy word-wrap a single line (no embedded newlines) to `width` columns.
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for word in line.split_inclusive(' ') {
        let wlen = word.chars().count();
        if cur_len + wlen > width && cur_len > 0 {
            out.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if wlen > width {
            // Hard-split very long words.
            for c in word.chars() {
                if cur_len == width {
                    out.push(std::mem::take(&mut cur));
                    cur_len = 0;
                }
                cur.push(c);
                cur_len += 1;
            }
        } else {
            cur.push_str(word);
            cur_len += wlen;
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

fn short(id: Uuid) -> String {
    id.to_string()[..8].to_string()
}
