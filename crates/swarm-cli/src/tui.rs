//! Full-screen TUI chat, in the spirit of Claude Code.

use std::collections::HashMap;
use std::path::PathBuf;

use futures::StreamExt;
use ratatui::crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use swarm_core::{AgentEvent, AgentMsg, SessionManager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::md;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MODELS: [&str; 2] = ["deepseek-chat", "deepseek-reasoner"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User,
    Assistant,
    Info,
    ToolStart,
    ToolOk,
    ToolErr,
}

struct Blk {
    kind: Kind,
    agent: String,
    depth: usize,
    text: String,
}

enum UiMsg {
    Agent(AgentMsg),
    Done(Result<String, String>),
}

struct App {
    manager: SessionManager,
    current: Uuid,

    model: String,
    temp: f32,
    usage: (usize, usize),

    blocks: Vec<Blk>,
    live: HashMap<String, usize>,

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
    let current = manager.create("main").await;
    let (model, temp, usage) = manager
        .status(current)
        .await
        .unwrap_or_else(|| ("deepseek-chat".into(), 0.2, (0, 24000)));

    let mut app = App {
        manager,
        current,
        model,
        temp,
        usage,
        blocks: Vec::new(),
        live: HashMap::new(),
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
            maybe_ev = term_events.next() => match maybe_ev {
                Some(Ok(ev)) => app.on_term_event(ev, &ui_tx).await,
                _ => break Ok(()),
            },
            Some(msg) = ui_rx.recv() => app.on_ui_msg(msg).await,
            _ = tick.tick() => { if app.busy { app.spinner = (app.spinner + 1) % SPINNER.len(); } }
        }
    };

    ratatui::restore();
    res
}

impl App {
    fn info(&mut self, text: impl Into<String>) {
        self.blocks.push(Blk {
            kind: Kind::Info,
            agent: "orchestrator".into(),
            depth: 0,
            text: text.into(),
        });
        self.follow = true;
    }

    async fn refresh_status(&mut self) {
        if let Some((m, t, u)) = self.manager.status(self.current).await {
            self.model = m;
            self.temp = t;
            self.usage = u;
        }
    }

    // ---- input ----------------------------------------------------------

    async fn on_term_event(&mut self, ev: Event, ui_tx: &UnboundedSender<UiMsg>) {
        let Event::Key(key) = ev else { return };
        if key.kind != KeyEventKind::Press {
            return; // ignore key-release (Windows fires both)
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
        let (s, e) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
        self.input.replace_range(s..e, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.chars().count() {
            return;
        }
        let (s, e) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
        self.input.replace_range(s..e, "");
    }

    fn byte_at(&self, idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(idx)
            .map(|(b, _)| b)
            .unwrap_or(self.input.len())
    }

    fn scroll_by(&mut self, delta: i32) {
        self.follow = false;
        self.scroll = (self.scroll as i32 + delta).max(0) as u16;
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

        self.blocks.push(Blk {
            kind: Kind::User,
            agent: "user".into(),
            depth: 0,
            text: text.clone(),
        });
        self.live.clear();
        self.busy = true;
        self.follow = true;

        let id = self.current;
        let manager = self.manager.clone();
        let ui_tx_done = ui_tx.clone();
        let (obs_tx, mut obs_rx) = unbounded_channel::<AgentMsg>();
        let ui_tx_fwd = ui_tx.clone();
        tokio::spawn(async move {
            while let Some(m) = obs_rx.recv().await {
                let _ = ui_tx_fwd.send(UiMsg::Agent(m));
            }
        });
        tokio::spawn(async move {
            let res = manager
                .send_observed(id, text, obs_tx)
                .await
                .map_err(|e| e.to_string());
            let _ = ui_tx_done.send(UiMsg::Done(res));
        });
    }

    async fn command(&mut self, cmd: &str) {
        let mut parts = cmd.split_whitespace();
        let name = parts.next().unwrap_or("");
        let rest = parts.collect::<Vec<_>>().join(" ");
        match name {
            "quit" | "exit" | "q" => self.quit = true,
            "help" | "h" => self.info(
                "commands: /model [name|#]  /temp <0-2>  /new [title]  /sessions  /switch <n>  /help  /quit\nscroll: ↑/↓ PgUp/PgDn",
            ),
            "model" => {
                if rest.is_empty() {
                    let list = MODELS
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("  {i}. {m}{}", if *m == self.model { "  (current)" } else { "" }))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.info(format!("models:\n{list}\nusage: /model <name|#>"));
                } else {
                    let chosen = rest
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| MODELS.get(i).map(|s| s.to_string()))
                        .unwrap_or(rest);
                    self.manager.set_model(self.current, chosen.clone()).await;
                    self.model = chosen.clone();
                    self.info(format!("model → {chosen}"));
                }
            }
            "temp" | "temperature" => match rest.trim().parse::<f32>() {
                Ok(t) => {
                    self.manager.set_temperature(self.current, t).await;
                    self.refresh_status().await;
                    self.info(format!("temperature → {:.2}", self.temp));
                }
                Err(_) => self.info("usage: /temp <0.0-2.0>"),
            },
            "new" => {
                let title = if rest.is_empty() { "session".into() } else { rest };
                self.current = self.manager.create(title).await;
                self.refresh_status().await;
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
                            self.refresh_status().await;
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

    async fn on_ui_msg(&mut self, msg: UiMsg) {
        match msg {
            UiMsg::Agent(m) => self.on_agent_msg(m),
            UiMsg::Done(res) => {
                self.busy = false;
                self.live.clear();
                if let Err(e) = res {
                    self.info(format!("error: {e}"));
                }
                self.refresh_status().await;
            }
        }
        self.follow = true;
    }

    fn on_agent_msg(&mut self, m: AgentMsg) {
        let AgentMsg { agent, depth, event } = m;
        match event {
            AgentEvent::Text { text: t } => {
                if let Some(&i) = self.live.get(&agent) {
                    self.blocks[i].text.push_str(&t);
                } else {
                    self.blocks.push(Blk {
                        kind: Kind::Assistant,
                        agent: agent.clone(),
                        depth,
                        text: t,
                    });
                    self.live.insert(agent, self.blocks.len() - 1);
                }
            }
            AgentEvent::ToolStart { name, args } => {
                self.live.remove(&agent);
                self.blocks.push(Blk {
                    kind: Kind::ToolStart,
                    agent,
                    depth,
                    text: format!("{name}\u{0}{}", format_args(&args)),
                });
            }
            AgentEvent::ToolEnd { name, ok, preview } => {
                self.blocks.push(Blk {
                    kind: if ok { Kind::ToolOk } else { Kind::ToolErr },
                    agent,
                    depth,
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
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(f.area());
        let (header, middle, input) = (rows[0], rows[1], rows[2]);

        // Sidebar only on wide terminals.
        let (body, sidebar) = if middle.width >= 80 {
            let cols = Layout::horizontal([Constraint::Min(1), Constraint::Length(22)]).split(middle);
            (cols[0], Some(cols[1]))
        } else {
            (middle, None)
        };

        self.draw_header(f, header);
        self.draw_body(f, body);
        if let Some(side) = sidebar {
            self.draw_sidebar(f, side);
        }
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
            Span::styled(self.model.clone(), Style::default().fg(Color::Magenta)),
            Span::styled(format!("  t={:.1}", self.temp), Style::default().fg(Color::DarkGray)),
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
        let max_scroll = total.saturating_sub(area.height);
        if self.follow {
            self.scroll = max_scroll;
        }
        self.scroll = self.scroll.min(max_scroll);
        f.render_widget(Paragraph::new(Text::from(lines)).scroll((self.scroll, 0)), area);
    }

    fn draw_input(&self, f: &mut Frame, area: Rect) {
        let block = Block::new().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let avail = inner.width.saturating_sub(2) as usize;
        let chars: Vec<char> = self.input.chars().collect();
        // Horizontal scroll so the cursor stays visible (display-width aware).
        let mut start = 0usize;
        while width_of(&chars[start..self.cursor]) > avail {
            start += 1;
        }
        let mut end = start;
        while end < chars.len() && width_of(&chars[start..=end]) <= avail {
            end += 1;
        }
        let visible: String = chars[start..end].iter().collect();
        let line = Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(visible),
        ]);
        f.render_widget(Paragraph::new(line), inner);

        let cx = inner.x + 2 + width_of(&chars[start..self.cursor]) as u16;
        f.set_cursor_position(Position::new(cx.min(inner.x + inner.width.saturating_sub(1)), inner.y));
    }

    fn draw_sidebar(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(area);
        let (ring_area, stats_area) = (rows[0], rows[1]);

        let (used, max) = self.usage;
        let frac = if max == 0 { 0.0 } else { (used as f64 / max as f64).min(1.0) };
        let accent = if frac < 0.6 {
            Color::Green
        } else if frac < 0.85 {
            Color::Yellow
        } else {
            Color::Red
        };
        let pct = (frac * 100.0).round() as u32;

        // Keep the ring round: equalize braille dots-per-unit across x/y.
        let w = ring_area.width.max(1) as f64;
        let h = ring_area.height.max(1) as f64;
        let xr = 1.25 * (w / (2.0 * h));
        let label = format!("{pct}%");
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-xr, xr])
            .y_bounds([-1.25, 1.25])
            .paint(move |ctx| {
                let n = 160;
                let mut used_pts = Vec::new();
                let mut rest_pts = Vec::new();
                for i in 0..n {
                    let t = i as f64 / n as f64;
                    let ang = std::f64::consts::FRAC_PI_2 - t * std::f64::consts::TAU;
                    let p = (ang.cos(), ang.sin());
                    if t <= frac {
                        used_pts.push(p);
                    } else {
                        rest_pts.push(p);
                    }
                }
                ctx.draw(&Points { coords: &rest_pts, color: Color::DarkGray });
                ctx.draw(&Points { coords: &used_pts, color: accent });
                ctx.print(-0.18 * label.len() as f64, 0.0, Span::styled(label.clone(), Style::default().fg(accent).add_modifier(Modifier::BOLD)));
            });
        f.render_widget(canvas, ring_area);

        let stats = Text::from(vec![
            Line::from(Span::styled("context memory", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                format!("{} / {} tok", fmt_k(used), fmt_k(max)),
                Style::default().fg(accent),
            )),
            Line::from(Span::styled(self.model.clone(), Style::default().fg(Color::Magenta))),
        ]);
        f.render_widget(Paragraph::new(stats), stats_area);
    }
}

/// Render transcript blocks into wrapped, styled lines (markdown for assistant,
/// per-agent gutters for sub-agents, pretty tool lines).
fn render_blocks(blocks: &[Blk], width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    for (idx, b) in blocks.iter().enumerate() {
        // Worker section header when the source agent changes.
        if b.depth > 0 && (idx == 0 || blocks[idx - 1].agent != b.agent) {
            out.push(Line::from(Span::styled(
                format!("▸ {}", b.agent),
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            )));
        }

        let gutter = b.depth > 0;
        let cw = if gutter { width.saturating_sub(2) } else { width };
        let mut content = block_content(b, cw.max(1));

        if gutter {
            for line in content.iter_mut() {
                let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::Blue))];
                spans.append(&mut line.spans);
                *line = Line::from(spans);
            }
        }
        out.append(&mut content);

        if matches!(b.kind, Kind::User | Kind::Assistant) {
            out.push(Line::default());
        }
    }
    out
}

fn block_content(b: &Blk, width: usize) -> Vec<Line<'static>> {
    match b.kind {
        Kind::User => {
            let mut out = Vec::new();
            let style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
            md::wrap_segments(&mut out, &[(b.text.clone(), style)], width.saturating_sub(2), "› ");
            out
        }
        Kind::Assistant => md::render(&b.text, width, Style::default()),
        Kind::Info => {
            let mut out = Vec::new();
            for raw in b.text.split('\n') {
                md::wrap_segments(
                    &mut out,
                    &[(raw.to_string(), Style::default().fg(Color::DarkGray))],
                    width,
                    "",
                );
            }
            out
        }
        Kind::ToolStart => {
            let (name, args) = b.text.split_once('\u{0}').unwrap_or((&b.text, ""));
            let mut spans = vec![
                Span::styled("⚙ ", Style::default().fg(Color::Cyan)),
                Span::styled(name.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ];
            if !args.is_empty() {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(args.to_string(), Style::default().fg(Color::DarkGray)));
            }
            vec![Line::from(spans)]
        }
        Kind::ToolOk => vec![Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled(b.text.clone(), Style::default().fg(Color::DarkGray)),
        ])],
        Kind::ToolErr => vec![Line::from(vec![
            Span::styled("  ✗ ", Style::default().fg(Color::Red)),
            Span::styled(b.text.clone(), Style::default().fg(Color::Red)),
        ])],
    }
}

/// Compact a JSON args object into `k=v, k=v`.
fn format_args(raw: &str) -> String {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return truncate(raw, 80);
    };
    let parts: Vec<String> = map
        .iter()
        .map(|(k, v)| {
            let vs = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}={}", truncate(&vs, 48))
        })
        .collect();
    truncate(&parts.join(", "), 120)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

fn fmt_k(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn width_of(chars: &[char]) -> usize {
    let s: String = chars.iter().collect();
    UnicodeWidthStr::width(s.as_str())
}

fn short(id: Uuid) -> String {
    id.to_string()[..8].to_string()
}
