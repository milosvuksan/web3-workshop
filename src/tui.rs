//! Live terminal dashboard. The coordinator remains the only owner of node state.
mod canvas;
mod color_editor;
mod network;
mod owned;
mod settings;
use crate::{
    Result, colors,
    diagnostics::{Runner, TASKS},
    ownership::{Acquisition, Source, timestamp_utc},
    settings::{MAX_MINING_THREADS, MiningThreads},
    state::State,
};
use canvas::canvas;
use color_editor::{ColorEditor, EditAction};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use owned::{owned_menu, owned_selection};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use serde_json::Value;
use settings::settings_panel;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, IsTerminal},
    path::Path,
    time::Duration,
};

const PAGES: &[&str] = &[
    "1 Canvas",
    "2 Controls",
    "3 Tests",
    "4 Debug",
    "5 Logs",
    "6 Help",
    "7 Owned",
    "8 Settings",
    "9 Network",
];
const DEBUG: &[&str] = &[
    "Status",
    "Peers",
    "Chain",
    "Mempool",
    "Sync",
    "Config / files",
];
const CONTROLS: &[&str] = &[
    "Toggle networking (online / offline)",
    "Toggle mining (start / stop worker)",
    "Export canvas and JSON diagnostics",
    "Paint selected parcel",
    "Transfer selected parcel",
    "Owned parcels and colors",
    "Settings (threads and TCP port)",
    "Quit program",
];
const ACCENT: Color = Color::Cyan;

pub struct View<'a> {
    pub status: Value,
    pub state: &'a State,
    pub owned: &'a BTreeMap<usize, Acquisition>,
    pub pending: &'a BTreeMap<usize, crate::mempool::PendingPaint>,
    pub logs: &'a VecDeque<String>,
    pub debug: String,
}
impl View<'_> {
    fn display_color(&self, id: usize) -> crate::protocol::Color {
        self.pending
            .get(&id)
            .filter(|_| self.owned.contains_key(&id))
            .map_or(self.state.parcels[id].color, |paint| paint.color)
    }
}
pub struct Input<'a> {
    pub mining: bool,
    pub online: bool,
    pub data_dir: &'a Path,
    pub owned: &'a BTreeMap<usize, Acquisition>,
    pub threads: MiningThreads,
    pub port: u16,
    pub saved_peers: &'a [crate::settings::SavedPeer],
}
pub struct App {
    terminal: DefaultTerminal,
    pub runner: Runner,
    pub debug_page: usize,
    page: usize,
    selection: usize,
    owned_selection: Option<usize>,
    editor: Option<ColorEditor>,
    show_owned: bool,
    task: usize,
    control: usize,
    settings_selection: usize,
    thread_draft: usize,
    port_draft: u16,
    network: network::NetworkPage,
    command: Option<String>,
    ownership: bool,
    scroll: u16,
    message: String,
    last_paint: Option<(usize, crate::protocol::Color)>,
}
impl App {
    pub fn new() -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(crate::Error::Invalid(
                "dashboard needs an interactive terminal; use `node` for pipes and scripts",
            ));
        }
        let terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(error) => {
                ratatui::restore();
                return Err(error.into());
            }
        };
        Ok(Self {
            terminal,
            runner: Runner::default(),
            debug_page: 0,
            page: 0,
            selection: 0,
            owned_selection: None,
            editor: None,
            show_owned: true,
            task: 0,
            control: 0,
            settings_selection: 1,
            thread_draft: 1,
            port_draft: 7001,
            network: network::NetworkPage::default(),
            command: None,
            ownership: false,
            scroll: 0,
            message: "Ready • : command • m mining • o network • ? help".into(),
            last_paint: None,
        })
    }
    pub fn input(&mut self, context: Input<'_>) -> Result<Option<String>> {
        let Input {
            mining,
            online,
            data_dir,
            owned,
            threads,
            port,
            saved_peers,
        } = context;
        self.owned_selection = owned_selection(owned, self.owned_selection);
        // Bound event work per frame so input floods cannot starve networking.
        for _ in 0..32 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(Some("quit".into()));
            }
            if let Some(editor) = &mut self.editor {
                if key.code == KeyCode::Enter && !owned.contains_key(&editor.parcel()) {
                    editor.reject("This parcel is no longer owned by this identity. Esc cancels.");
                    continue;
                }
                match editor.input(key) {
                    EditAction::Cancel => self.editor = None,
                    EditAction::Submit(command) => {
                        // Keep the editor until the coordinator admits the edit.
                        return Ok(Some(command));
                    }
                    EditAction::Continue => {}
                }
                continue;
            }
            if self.page == 8 && self.command.is_none() && self.network.handles(key) {
                if let Some(command) = self.network.input(key, saved_peers) {
                    return Ok(Some(command));
                }
                continue;
            }
            if let Some(command) = &mut self.command {
                match key.code {
                    KeyCode::Esc => self.command = None,
                    KeyCode::Enter => {
                        let line = self.command.take().unwrap();
                        self.message = format!("Submitted: {line}");
                        return Ok(Some(line));
                    }
                    KeyCode::Backspace => {
                        command.pop();
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && command.len() < 1024 =>
                    {
                        command.push(c);
                    }
                    _ => {}
                }
                continue;
            }
            match key.code {
                KeyCode::Char('9') => {
                    self.page = 8;
                    self.reset_scroll();
                }
                KeyCode::Char('8' | 's') => self.open_settings(threads, port),
                KeyCode::Char('q') => return Ok(Some("quit".into())),
                KeyCode::Char(':') => self.command = Some(String::new()),
                KeyCode::Char('m') => {
                    return Ok(Some(format!(
                        "mining {}",
                        if mining { "off" } else { "on" }
                    )));
                }
                KeyCode::Char('o') => {
                    return Ok(Some(format!(
                        "network {}",
                        if online { "off" } else { "on" }
                    )));
                }
                KeyCode::Char('e') => {
                    self.message = format!("Export requested → {}", data_dir.display());
                    return Ok(Some("export".into()));
                }
                KeyCode::Char('p') => self.edit_color(owned),
                KeyCode::Char('t') => {
                    self.command = Some(format!("transfer {} ", self.active_parcel()))
                }
                KeyCode::Char('v') => self.ownership = !self.ownership,
                KeyCode::Char('h') => self.show_owned = !self.show_owned,
                KeyCode::Char('b') => {
                    self.page = 6;
                    self.reset_scroll();
                }
                KeyCode::Char('g') if self.page == 6 => {
                    if let Some(id) = self.owned_selection {
                        self.selection = id;
                        self.page = 0;
                    }
                }
                KeyCode::Char('?') => {
                    self.page = 5;
                    self.scroll = u16::MAX;
                }
                KeyCode::Char(c @ '1'..='7') => {
                    self.page = c as usize - '1' as usize;
                    self.reset_scroll();
                }
                KeyCode::Tab => {
                    self.page = (self.page + 1) % PAGES.len();
                    if self.page == 7 {
                        self.open_settings(threads, port);
                    }
                    self.reset_scroll();
                }
                KeyCode::BackTab => {
                    self.page = (self.page + PAGES.len() - 1) % PAGES.len();
                    if self.page == 7 {
                        self.open_settings(threads, port);
                    }
                    self.reset_scroll();
                }
                KeyCode::Char('x') if self.page == 2 => {
                    self.runner.cancel();
                    self.message = self.runner.status.clone();
                }
                KeyCode::Up if self.page == 6 => self.move_owned(owned, -1),
                KeyCode::Down if self.page == 6 => self.move_owned(owned, 1),
                KeyCode::PageUp if self.page == 6 => self.move_owned(owned, -10),
                KeyCode::PageDown if self.page == 6 => self.move_owned(owned, 10),
                KeyCode::Home if self.page == 6 => {
                    self.owned_selection = owned.keys().next().copied()
                }
                KeyCode::End if self.page == 6 => {
                    self.owned_selection = owned.keys().next_back().copied()
                }
                KeyCode::Enter if self.page == 6 => self.edit_color(owned),
                KeyCode::Up if self.page == 7 => {
                    self.settings_selection = self.settings_selection.saturating_sub(1)
                }
                KeyCode::Down if self.page == 7 => {
                    self.settings_selection = (self.settings_selection + 1).min(2)
                }
                KeyCode::Left | KeyCode::Char('-') if self.page == 7 => {
                    if self.settings_selection == 2 {
                        self.port_draft = self.port_draft.saturating_sub(1).max(1);
                    } else {
                        self.settings_selection = 1;
                        self.thread_draft = self.thread_draft.saturating_sub(1).max(1);
                    }
                }
                KeyCode::Right | KeyCode::Char('+' | '=') if self.page == 7 => {
                    if self.settings_selection == 2 {
                        self.port_draft = self.port_draft.saturating_add(1);
                    } else {
                        self.settings_selection = 1;
                        self.thread_draft = (self.thread_draft + 1).min(MAX_MINING_THREADS);
                    }
                }
                KeyCode::Char('n') if self.page == 7 => {
                    self.command = Some(if self.settings_selection == 2 {
                        "port ".into()
                    } else {
                        "threads ".into()
                    })
                }
                KeyCode::Enter if self.page == 7 => {
                    if self.settings_selection == 2 {
                        return Ok(Some(format!("port {}", self.port_draft)));
                    }
                    return Ok(Some(format!(
                        "threads {}",
                        if self.settings_selection == 0 {
                            "all".into()
                        } else {
                            self.thread_draft.to_string()
                        }
                    )));
                }
                KeyCode::PageUp => self.scroll = self.scroll.saturating_add(10),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
                KeyCode::Home => self.scroll = u16::MAX,
                KeyCode::End => self.scroll = 0,
                KeyCode::Left if self.page == 0 => {
                    self.selection = self.selection.saturating_sub(1)
                }
                KeyCode::Right if self.page == 0 => self.selection = (self.selection + 1).min(2047),
                KeyCode::Up if self.page == 0 => self.selection = self.selection.saturating_sub(64),
                KeyCode::Down if self.page == 0 => self.selection = (self.selection + 64).min(2047),
                KeyCode::Up if self.page == 1 => self.control = self.control.saturating_sub(1),
                KeyCode::Down if self.page == 1 => {
                    self.control = (self.control + 1).min(CONTROLS.len() - 1)
                }
                KeyCode::Up if self.page == 2 => self.task = self.task.saturating_sub(1),
                KeyCode::Down if self.page == 2 => self.task = (self.task + 1).min(TASKS.len() - 1),
                KeyCode::Left if self.page == 3 => {
                    self.debug_page = self.debug_page.saturating_sub(1);
                    self.scroll = u16::MAX;
                }
                KeyCode::Right if self.page == 3 => {
                    self.debug_page = (self.debug_page + 1).min(DEBUG.len() - 1);
                    self.scroll = u16::MAX;
                }
                KeyCode::Enter if self.page == 1 => match self.control {
                    0 => {
                        return Ok(Some(format!(
                            "network {}",
                            if online { "off" } else { "on" }
                        )));
                    }
                    1 => {
                        return Ok(Some(format!(
                            "mining {}",
                            if mining { "off" } else { "on" }
                        )));
                    }
                    2 => return Ok(Some("export".into())),
                    3 => self.edit_color(owned),
                    4 => self.command = Some(format!("transfer {} ", self.selection)),
                    5 => {
                        self.page = 6;
                        self.reset_scroll();
                    }
                    6 => self.open_settings(threads, port),
                    _ => return Ok(Some("quit".into())),
                },
                KeyCode::Enter if self.page == 2 => {
                    self.message = match self.runner.start(self.task, data_dir) {
                        Ok(()) => "Diagnostic started; x cancels; full output saved to file".into(),
                        Err(error) => format!("Diagnostic rejected: {error}"),
                    };
                }
                _ => {}
            }
        }
        Ok(None)
    }
    fn open_settings(&mut self, threads: MiningThreads, port: u16) {
        self.page = 7;
        self.port_draft = port.max(1);
        self.settings_selection = usize::from(threads != MiningThreads::All);
        self.thread_draft = threads.resolved().min(MAX_MINING_THREADS);
        self.reset_scroll();
    }
    fn active_parcel(&self) -> usize {
        if self.page == 6 {
            self.owned_selection.unwrap_or(self.selection)
        } else {
            self.selection
        }
    }
    pub fn command_result(&mut self, command: &str, result: &Result<bool>, mining: bool) {
        if command.split_whitespace().next() == Some("peer") {
            self.network.command_result(result);
            self.message = match result {
                Ok(_) => "Peer configuration saved".into(),
                Err(error) => format!("Peer change rejected: {error}"),
            };
            return;
        }
        if command.split_whitespace().next() == Some("port") {
            self.message = match result {
                Ok(_) => "TCP port saved".into(),
                Err(error) => format!("Port setting rejected: {error}"),
            };
            if result.is_ok()
                && let Some(port) = command
                    .split_whitespace()
                    .nth(1)
                    .and_then(|value| value.parse().ok())
            {
                self.port_draft = port;
            }
            return;
        }
        if command.split_whitespace().next() == Some("threads") {
            self.message = match result {
                Ok(_) => "Mining thread preference saved".into(),
                Err(error) => format!("Thread setting rejected: {error}"),
            };
            if result.is_ok()
                && let Some(threads) = command
                    .split_whitespace()
                    .nth(1)
                    .and_then(|value| value.parse::<MiningThreads>().ok())
            {
                self.settings_selection = usize::from(threads != MiningThreads::All);
                self.thread_draft = threads.resolved().min(MAX_MINING_THREADS);
            }
            return;
        }
        let Ok(tx) = crate::cli::transaction(command, 0) else {
            return;
        };
        let crate::protocol::Action::Paint { color } = tx.action else {
            return;
        };
        match result {
            Ok(_) => {
                self.editor = None;
                self.ownership = false;
                self.last_paint = Some((usize::from(tx.parcel), color));
                self.message = if mining {
                    "Color queued • showing pending preview • waiting for block confirmation"
                } else {
                    "Color queued • pending preview • m starts mining; a peer can confirm it too"
                }
                .into();
            }
            Err(error) => {
                self.message = format!("Color rejected: {error}");
                if let Some(editor) = &mut self.editor {
                    editor.reject(&error.to_string());
                }
            }
        }
    }
    fn edit_color(&mut self, owned: &BTreeMap<usize, Acquisition>) {
        let id = self.active_parcel();
        if owned.contains_key(&id) {
            self.editor = Some(ColorEditor::new(id));
        } else {
            self.message = "Select a parcel owned by this identity to edit its color".into();
        }
    }
    fn move_owned(&mut self, owned: &BTreeMap<usize, Acquisition>, amount: isize) {
        let ids: Vec<_> = owned.keys().copied().collect();
        if ids.is_empty() {
            self.owned_selection = None;
            return;
        }
        let index = ids
            .iter()
            .position(|id| Some(*id) == self.owned_selection)
            .unwrap_or(0);
        self.owned_selection = Some(ids[index.saturating_add_signed(amount).min(ids.len() - 1)]);
    }
    fn reset_scroll(&mut self) {
        self.scroll = if matches!(self.page, 3 | 5) {
            u16::MAX
        } else {
            0
        };
    }
    pub fn needs_debug(&self) -> bool {
        self.page == 3
    }
    pub fn draw(&mut self, view: View<'_>) -> Result<()> {
        let Self {
            terminal,
            runner,
            debug_page,
            page,
            selection,
            owned_selection,
            editor,
            show_owned,
            task,
            control,
            settings_selection,
            thread_draft,
            port_draft,
            network,
            command,
            ownership,
            scroll,
            message,
            last_paint,
        } = self;
        terminal.draw(|frame| {
            let area = frame.area();
            if area.width < 36 || area.height < 10 {
                frame.render_widget(Paragraph::new("Pixel Chain\nResize terminal to at least 36×10.\nq quits; o network; m mining.").wrap(Wrap { trim: false }), area);
                return;
            }
            let regions = Layout::vertical([Constraint::Length(2), Constraint::Length(1), Constraint::Min(1), Constraint::Length(4)]).split(area);
            let online = view.status["online"].as_bool().unwrap_or(false);
            let mining = view.status["mining"].as_bool().unwrap_or(false);
            let title = Line::from(vec![
                Span::styled(" PIXEL CHAIN ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(if online { " ONLINE " } else { " OFFLINE " }, Style::default().fg(if online { Color::Green } else { Color::Yellow })),
                Span::styled(if mining { " MINING " } else { " MINING STOPPED " }, Style::default().fg(if mining { Color::Green } else { Color::DarkGray })),
            ]);
            frame.render_widget(Paragraph::new(vec![title, Line::from(format!(" Height {}  •  Sessions {}  •  Pending {}  •  Owned {}  •  Threads {}", view.status["height"], view.status["connections"], view.status["mempool"], view.status["owned"].as_array().map_or(0, Vec::len), view.status["mining_threads"]))]), regions[0]);
            frame.render_widget(Tabs::new(PAGES.iter().copied()).padding("", "").select(*page).highlight_style(Style::default().fg(Color::Black).bg(ACCENT)), regions[1]);
            match *page {
                0 => canvas(frame, regions[2], &view, *selection, *ownership, *show_owned),
                1 => controls(frame, regions[2], *control, online, mining),
                2 => diagnostics(frame, regions[2], runner, *task, scroll, online),
                3 => {
                    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(regions[2]);
                    frame.render_widget(Tabs::new(DEBUG.iter().copied()).select(*debug_page).highlight_style(Style::default().fg(ACCENT)).block(Block::default().title("←/→ debug submenu")), rows[0]);
                    text_panel(frame, rows[1], "Live debug state • PgUp/PgDn scroll", &view.debug, scroll);
                }
                4 => {
                    let text = view.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                    text_panel(frame, regions[2], "Node log • full history in node.log • PgUp/PgDn scroll", &text, scroll);
                }
                5 => text_panel(frame, regions[2], "Keyboard reference", HELP, scroll),
                6 => owned_menu(frame, regions[2], &view, *owned_selection),
                8 => network.draw(frame, regions[2], &view.status),
                _ => settings_panel(frame, regions[2], &view.status, *settings_selection, *thread_draft, *port_draft),
            }
            let (text, title) = match command {
                Some(line) => (format!(":{line}█"), "Command • Enter submits • Esc cancels"),
                None => {
                    let progress = last_paint.map_or_else(|| message.clone(), |paint| paint_feedback(&view, paint));
                    (view.logs.back().map_or_else(|| progress.clone(), |line| format!("{progress}\n{line}")), "q quit • : command • m mining • o network • ? help")
                },
            };
            frame.render_widget(Paragraph::new(text).block(panel(title)).style(Style::default().fg(if command.is_some() { ACCENT } else { Color::Gray })), regions[3]);
            if let Some(editor) = editor { editor.draw(frame, &view); }
            network.draw_editor(frame);
        })?;
        Ok(())
    }
}
fn paint_feedback(view: &View<'_>, (id, color): (usize, crate::protocol::Color)) -> String {
    if let Some(paint) = view
        .pending
        .get(&id)
        .filter(|_| view.owned.contains_key(&id))
    {
        format!(
            "Parcel {id}: PENDING #{} preview • {}",
            paint.color,
            if view.status["mining"] == true {
                "waiting for block confirmation"
            } else {
                "mining stopped; m starts mining, or wait for a peer"
            }
        )
    } else if !view.owned.contains_key(&id) {
        format!("Parcel {id} is no longer yours; no local color preview")
    } else if view.state.parcels[id].color == color {
        format!("Parcel {id}: color #{color} CONFIRMED")
    } else {
        format!(
            "Parcel {id}: edit no longer pending • confirmed #{} • re-submit if needed",
            view.state.parcels[id].color
        )
    }
}
impl Drop for App {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::DarkGray))
}
fn menu(frame: &mut Frame, area: Rect, title: &str, items: &[&str], selected: usize) {
    let list = List::new(items.iter().map(|s| ListItem::new(*s)))
        .block(panel(title))
        .highlight_style(Style::default().bg(Color::DarkGray).fg(ACCENT))
        .highlight_symbol("› ");
    frame.render_stateful_widget(
        list,
        area,
        &mut ListState::default().with_selected(Some(selected)),
    );
}
fn text_panel(frame: &mut Frame, area: Rect, title: &str, text: &str, scroll: &mut u16) {
    let visible = area.height.saturating_sub(2) as usize;
    let lines = text.lines().count();
    let bottom = lines.saturating_sub(visible);
    *scroll = (*scroll).min(bottom.min(u16::MAX as usize) as u16);
    let offset = bottom
        .saturating_sub(*scroll as usize)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(text).block(panel(title)).scroll((offset, 0)),
        area,
    );
}
fn controls(frame: &mut Frame, area: Rect, selected: usize, online: bool, mining: bool) {
    let rows = Layout::vertical([Constraint::Length(10), Constraint::Min(1)]).split(area);
    menu(
        frame,
        rows[0],
        "Controls • ↑/↓ select • Enter apply",
        CONTROLS,
        selected,
    );
    frame.render_widget(Paragraph::new(format!(
        "Network: {}\nMining: {}\n\nOffline closes the TCP listener, sessions, queued sends, and reconnect loops. Local mining, painting, and inspection stay available. Reconnecting resumes synchronization and gossip.\n\nMining OFF cancels and joins the worker. Networking continues independently.\n\nOffline affects this node. Test jobs may start isolated loopback nodes; cancel them with x in Tests.\n\nThe chain is in memory; quit loses local chain state. Keep a peer alive for recovery.",
        if online { "ONLINE" } else { "OFFLINE" }, if mining { "ON" } else { "STOPPED" }
    )).block(panel("Control behavior")).wrap(Wrap { trim: false }), rows[1]);
}
fn diagnostics(
    frame: &mut Frame,
    area: Rect,
    runner: &Runner,
    task: usize,
    scroll: &mut u16,
    online: bool,
) {
    let regions = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(43), Constraint::Percentage(57)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).split(area)
    };
    menu(
        frame,
        regions[0],
        "Tests / benchmarks • ↑/↓ • Enter run • x stop",
        TASKS,
        task,
    );
    let rows = Layout::vertical([Constraint::Length(5), Constraint::Min(1)]).split(regions[1]);
    let summary = format!(
        "{}\nWorkspace: {}\nLog: {}\n{}",
        runner.status,
        runner.root.display(),
        runner
            .log_path
            .as_ref()
            .map_or_else(|| "(none yet)".into(), |p| p.display().to_string()),
        if online {
            "Node stays responsive while diagnostics run."
        } else {
            "Node OFFLINE. Tests can create isolated loopback peers."
        }
    );
    frame.render_widget(
        Paragraph::new(summary).block(Block::default().title("Job status").borders(Borders::TOP)),
        rows[0],
    );
    let text = runner.output.iter().cloned().collect::<Vec<_>>().join("\n");
    text_panel(
        frame,
        rows[1],
        "Diagnostic output • PgUp/PgDn • End follows",
        &text,
        scroll,
    );
}
const HELP: &str = "Tab / Shift-Tab or 1–9   Switch pages\n↑/↓/←/→ on Canvas         Select parcel (viewport follows on small terminals)\nv                        Toggle RGB / ownership colors\np / t                    Color picker / transfer selected parcel\nb / 7                    Open owned parcels and acquisition history\nh                        Show/hide your parcel outlines on the canvas\ns / 8                    Settings: mining threads and TCP port\n9                        Network: saved IP addresses, aliases, connection status\nNetwork page             a adds; Enter/e edits; d removes; o online/offline\nColor picker             Type a name or hex; ↑/↓ select; Tab completes; Enter sends; Esc cancels\nm                        Start or stop mining independently\no                        Go offline or reconnect independently\ne                        Export status.json, state.json and canvas.ppm\n:                        Open command input; Esc cancels\nq / Ctrl-C               Stop jobs and mining, close sockets, restore terminal\n\nControls submenu: ↑/↓ then Enter to apply.\nTests submenu: ↑/↓ then Enter; x cancels the active diagnostic.\nDebug submenu: ←/→ selects Status, Peers, Chain, Mempool, Sync, Config.\nPgUp / PgDn scroll output; Home oldest; End follows latest.\n\nCommands:\nstatus | identity | owned | mining on|off | threads COUNT|all | port PORT | network on|off\npeer add IP[:PORT] [ALIAS] | peer edit OLD_IP[:PORT] IP[:PORT] [ALIAS] | peer remove IP[:PORT]\npaint PARCEL COLOR_NAME|RRGGBB | transfer PARCEL PUBLIC_KEY | export [PATH.ppm] | quit\n\nDiagnostic jobs keep this UI responsive and save full output under data-dir/diagnostics.\nCargo, Python 3 and OpenSSL are needed for the full validation suite.\nSet PIXEL_WORKSPACE if the executable has moved away from its source tree.\nTests can start isolated local nodes even while this node is offline.\nMining benchmarks use real 23-bit proof of work and can take several minutes.\n\nIdentity persists, but chain storage is in memory. Restart with surviving peers for recovery.";

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn canvas_uses_live_rgb_and_handles_small_viewports() {
        let mut state = State::default();
        state.parcels[1].color.0 = [255, 128, 0];
        state.parcels[65].color.0 = [0, 128, 255];
        for (width, height) in [(120, 24), (80, 24), (36, 10)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    canvas(
                        frame,
                        frame.area(),
                        &View {
                            status: Value::Null,
                            state: &state,
                            owned: &BTreeMap::new(),
                            pending: &BTreeMap::new(),
                            logs: &VecDeque::new(),
                            debug: String::new(),
                        },
                        if width == 120 { 0 } else { 2047 },
                        false,
                        true,
                    )
                })
                .unwrap();
            if width == 120 {
                let cells = terminal.backend().buffer();
                assert_eq!(cells[(6, 2)].bg, Color::Rgb(255, 128, 0));
                assert_eq!(cells[(6, 5)].bg, Color::Rgb(0, 128, 255));
            }
        }
    }

    #[test]
    fn home_then_page_down_moves_away_from_the_beginning() {
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        let text = (0..100)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut scroll = u16::MAX;
        terminal
            .draw(|frame| text_panel(frame, frame.area(), "test", &text, &mut scroll))
            .unwrap();
        assert_eq!(scroll, 92);
        assert_eq!(terminal.backend().buffer()[(1, 1)].symbol(), "0");
        scroll = scroll.saturating_sub(10);
        terminal
            .draw(|frame| text_panel(frame, frame.area(), "test", &text, &mut scroll))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(1, 1)].symbol(), "1");
        assert_eq!(terminal.backend().buffer()[(2, 1)].symbol(), "0");
    }

    #[test]
    fn map_outlines_each_owned_parcel_without_covering_its_color() {
        let receipt = Acquisition {
            height: 1,
            timestamp: 0,
            block: crate::protocol::Bytes::default(),
            source: Source::Mining,
        };
        let owned = BTreeMap::from([
            (0, receipt.clone()),
            (64, receipt.clone()),
            (2, receipt.clone()),
            (67, receipt),
        ]);
        let state = State::default();
        let view = View {
            status: Value::Null,
            state: &state,
            owned: &owned,
            pending: &BTreeMap::new(),
            logs: &VecDeque::new(),
            debug: String::new(),
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|frame| canvas(frame, frame.area(), &view, 0, false, true))
            .unwrap();
        let cells = terminal.backend().buffer();
        assert_eq!(cells[(1, 1)].symbol(), "┏");
        assert_eq!(cells[(1, 1)].fg, Color::Yellow);
        assert_eq!(cells[(2, 2)].bg, Color::Rgb(0, 0, 0));
        assert_eq!(
            cells[(2, 2)].symbol(),
            "›",
            "selection stays inside the outline"
        );
        assert_eq!(cells[(9, 1)].symbol(), "┏");
        assert_eq!(cells[(13, 4)].symbol(), "┏");
        assert_eq!(cells[(5, 1)].fg, Color::DarkGray);
        terminal
            .draw(|frame| canvas(frame, frame.area(), &view, 0, false, false))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(9, 1)].symbol(), "┌");
        assert_eq!(terminal.backend().buffer()[(9, 1)].fg, Color::DarkGray);
    }

    #[test]
    fn pending_color_is_visible_inside_outline_and_confirmation_updates_feedback() {
        let receipt = Acquisition {
            height: 1,
            timestamp: 0,
            block: Default::default(),
            source: Source::Mining,
        };
        let owned = BTreeMap::from([(2, receipt)]);
        let red = crate::colors::parse("red").unwrap();
        let blue = crate::colors::parse("blue").unwrap();
        let pending = BTreeMap::from([(
            2,
            crate::mempool::PendingPaint {
                color: red,
                transaction: Default::default(),
            },
        )]);
        let mut state = State::default();
        state.parcels[2].color = blue;
        let view = View {
            status: Value::Null,
            state: &state,
            owned: &owned,
            pending: &pending,
            logs: &VecDeque::new(),
            debug: String::new(),
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|frame| canvas(frame, frame.area(), &view, 2, false, true))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(10, 2)].bg, colors::rgb(red));
        assert_eq!(terminal.backend().buffer()[(9, 1)].fg, Color::Yellow);
        assert_eq!(
            view.state.parcels[2].color, blue,
            "preview must not change consensus state"
        );
        assert!(paint_feedback(&view, (2, red)).contains("PENDING"));
        assert!(paint_feedback(&view, (2, red)).contains("mining stopped"));
        let no_longer_owned = View {
            owned: &BTreeMap::new(),
            ..view
        };
        assert_eq!(no_longer_owned.display_color(2), blue);
        state.parcels[2].color = red;
        let confirmed = View {
            status: Value::Null,
            state: &state,
            owned: &owned,
            pending: &BTreeMap::new(),
            logs: &VecDeque::new(),
            debug: String::new(),
        };
        assert!(paint_feedback(&confirmed, (2, red)).contains("CONFIRMED"));
    }
}
