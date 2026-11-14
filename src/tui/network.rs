use super::*;
use crate::settings::{CompatibilityProfile, SavedPeer, peer_address_for};

#[derive(Default)]
pub(super) struct NetworkPage {
    selected: usize,
    editor: Option<PeerEditor>,
}
struct PeerEditor {
    original: Option<std::net::SocketAddr>,
    address: String,
    alias: String,
    field: usize,
    compatibility: CompatibilityProfile,
    dropdown: bool,
    error: String,
}
impl NetworkPage {
    pub fn handles(&self, key: KeyEvent) -> bool {
        self.editor.is_some()
            || matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Enter | KeyCode::Delete | KeyCode::Char('a' | 'e' | 'd'))
    }
    pub fn input(&mut self, key: KeyEvent, peers: &[SavedPeer]) -> Option<String> {
        self.selected = self.selected.min(peers.len().saturating_sub(1));
        if let Some(editor) = &mut self.editor {
            match key.code {
                KeyCode::Esc if editor.dropdown => editor.dropdown = false,
                KeyCode::Esc => self.editor = None,
                KeyCode::Tab | KeyCode::BackTab => {
                    editor.field = (editor.field + if key.code == KeyCode::BackTab { 2 } else { 1 }) % 3;
                    editor.dropdown = false;
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down if editor.field == 2 => {
                    let profiles = CompatibilityProfile::ALL;
                    let index = profiles.iter().position(|p| *p == editor.compatibility).unwrap_or(0);
                    editor.compatibility = profiles
                        [(index + if matches!(key.code, KeyCode::Left | KeyCode::Up) { profiles.len() - 1 } else { 1 }) % profiles.len()];
                }
                KeyCode::Char(' ') if editor.field == 2 => editor.dropdown = !editor.dropdown,
                KeyCode::Enter if editor.dropdown => editor.dropdown = false,
                KeyCode::Up | KeyCode::Down => editor.field = (editor.field + 1) % 3,
                KeyCode::Enter => {
                    match peer_address_for(editor.address.trim(), editor.compatibility)
                        .and_then(|address| SavedPeer::new(address, &editor.alias))
                    {
                        Err(error) => editor.error = error.to_string(),
                        Ok(_) if editor.field == 0 => {
                            editor.field = 1;
                            editor.error.clear();
                        }
                        Ok(peer) => {
                            let profile = if editor.compatibility == CompatibilityProfile::Native && editor.original.is_none() {
                                String::new()
                            } else {
                                format!("--compat {} ", editor.compatibility.id())
                            };
                            return Some(match editor.original {
                                Some(old) => {
                                    format!("peer edit {old} {} {profile}{}", peer.address, peer.alias)
                                }
                                None => {
                                    format!("peer add {} {profile}{}", peer.address, peer.alias)
                                }
                            });
                        }
                    }
                }
                KeyCode::Backspace => {
                    if editor.field == 0 {
                        editor.address.pop();
                    } else if editor.field == 1 {
                        editor.alias.pop();
                    }
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if editor.field == 0 {
                        editor.address.clear();
                    } else if editor.field == 1 {
                        editor.alias.clear();
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !c.is_control() && editor.field < 2 =>
                {
                    let (text, limit) = if editor.field == 0 { (&mut editor.address, 80) } else { (&mut editor.alias, 64) };
                    if text.chars().count() < limit {
                        text.push(c);
                    }
                }
                _ => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(peers.len().saturating_sub(1)),
            KeyCode::Char('a') => {
                self.editor = Some(PeerEditor {
                    original: None,
                    address: String::new(),
                    alias: String::new(),
                    field: 0,
                    compatibility: CompatibilityProfile::Native,
                    dropdown: false,
                    error: String::new(),
                })
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                if let Some(peer) = peers.get(self.selected) {
                    self.editor = Some(PeerEditor {
                        original: Some(peer.address),
                        address: peer.address.to_string(),
                        alias: peer.alias.clone(),
                        field: 1,
                        compatibility: peer.compatibility,
                        dropdown: false,
                        error: String::new(),
                    });
                }
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                if let Some(peer) = peers.get(self.selected) {
                    return Some(format!("peer remove {}", peer.address));
                }
            }
            _ => {}
        }
        None
    }
    pub fn command_result(&mut self, result: &Result<bool>) {
        match result {
            Ok(_) => self.editor = None,
            Err(error) => {
                if let Some(editor) = &mut self.editor {
                    editor.error = error.to_string();
                }
            }
        }
    }
    pub fn draw(&self, frame: &mut Frame, area: Rect, status: &Value) {
        let regions =
            Layout::vertical([Constraint::Length(4), Constraint::Min(3), Constraint::Length(5), Constraint::Length(4)]).split(area);
        let online = status["online"].as_bool().unwrap_or(false);
        frame.render_widget(
            Paragraph::new(format!(
                "{} • Listening: {} • Discovery: {}\nConnected means a native handshake or successful profile HTTP synchronization.",
                if online { "ONLINE" } else { "OFFLINE" },
                status["listen"].as_str().unwrap_or(""),
                if status["discovery"] == true { "on" } else { "off" }
            ))
            .block(panel("Network")),
            regions[0],
        );
        let peers = status["network_peers"].as_array().map(Vec::as_slice).unwrap_or_default();
        if peers.is_empty() {
            frame.render_widget(
                Paragraph::new("No saved peers. Press a to add an IP address and alias.\nAddresses without a port use 7001.")
                    .block(panel("Saved peers")),
                regions[1],
            );
        } else {
            let items: Vec<_> = peers
                .iter()
                .map(|peer| {
                    let state = peer["status"].as_str().unwrap_or("Offline");
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::raw(format!("{}  ", peer["alias"].as_str().unwrap_or(""))),
                            Span::styled(state, Style::default().fg(if state == "Connected" { Color::Green } else { Color::Yellow })),
                        ]),
                        Line::from(format!(
                            "  {} • {}{}",
                            peer["address"].as_str().unwrap_or(""),
                            peer["compatibility_label"].as_str().unwrap_or("None (native)"),
                            peer["error"].as_str().map(|e| format!(" • {e}")).unwrap_or_default()
                        )),
                    ])
                })
                .collect();
            let mut selected = ListState::default().with_selected(Some(self.selected.min(peers.len() - 1)));
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel("Saved peers • ↑/↓ select"))
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().bg(Color::DarkGray)),
                regions[1],
                &mut selected,
            );
        }
        let sessions = status["network_sessions"].as_array().map(Vec::as_slice).unwrap_or_default();
        let mut lines: Vec<Line> = sessions
            .iter()
            .map(|session| {
                Line::from(format!(
                    "{}{} • {}",
                    session["alias"].as_str().map(|alias| format!("{alias} • ")).unwrap_or_default(),
                    session["address"].as_str().unwrap_or(""),
                    if session["ready"] == true { "Blockchain peer" } else { "Handshaking" }
                ))
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::from("No active TCP sessions."));
        }
        let startup = status["startup_peers"].as_array().map_or(0, Vec::len);
        frame.render_widget(
            Paragraph::new(lines).block(panel(&format!("Live sessions • {startup} additional startup targets"))),
            regions[2],
        );
        let foreign = &status["compatibility"];
        frame.render_widget(
            Paragraph::new(format!(
                "a Add • Enter/e Edit • d/Delete Remove • o Online/offline\nGrupa 1 - Luka: height {} • {} pending • mining {} (see help)",
                foreign["height"],
                foreign["mempool"],
                if status["compatibility_mining"] == true { "ON" } else { "off" }
            ))
            .block(panel("Peer controls")),
            regions[3],
        );
    }
    pub fn draw_editor(&self, frame: &mut Frame) {
        let Some(editor) = &self.editor else {
            return;
        };
        let area = centered(frame.area(), 76, 17);
        frame.render_widget(Clear, area);
        let block = panel(if editor.original.is_some() { "Edit peer" } else { "Add peer" });
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(if editor.dropdown { 4 } else { 3 }),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);
        for (index, (title, value)) in
            [("IP address • optional :port", &editor.address), ("Alias • optional", &editor.alias)].into_iter().enumerate()
        {
            frame.render_widget(
                Paragraph::new(format!("{value}{}", if editor.field == index { "█" } else { "" }))
                    .block(panel(title).border_style(Style::default().fg(if editor.field == index { ACCENT } else { Color::Gray }))),
                rows[index],
            );
        }
        let profile_text = if editor.dropdown {
            CompatibilityProfile::ALL
                .iter()
                .map(|p| format!("{} {}", if *p == editor.compatibility { "›" } else { " " }, p.label()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            format!("{}  ▾", editor.compatibility.label())
        };
        frame.render_widget(
            Paragraph::new(profile_text).block(
                panel("Cross compatibility • optional").border_style(Style::default().fg(if editor.field == 2 {
                    ACCENT
                } else {
                    Color::Gray
                })),
            ),
            rows[2],
        );
        frame.render_widget(
            Paragraph::new("Tab: field • ←/→: profile • Space: dropdown\nEnter: save • Ctrl-U: clear • Esc: cancel"),
            rows[3],
        );
        frame.render_widget(
            Paragraph::new(editor.error.as_str()).style(Style::default().fg(Color::Red)).wrap(Wrap { trim: false }),
            rows[4],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn type_text(page: &mut NetworkPage, text: &str) {
        for c in text.chars() {
            page.input(key(KeyCode::Char(c)), &[]);
        }
    }
    #[test]
    fn adding_validates_ip_then_accepts_an_alias_with_spaces() {
        let mut page = NetworkPage::default();
        page.input(key(KeyCode::Char('a')), &[]);
        type_text(&mut page, "invalid");
        assert!(page.input(key(KeyCode::Enter), &[]).is_none());
        assert!(!page.editor.as_ref().unwrap().error.is_empty());
        page.input(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL), &[]);
        type_text(&mut page, "192.0.2.7");
        page.input(key(KeyCode::Enter), &[]);
        type_text(&mut page, "Workshop node");
        assert_eq!(page.input(key(KeyCode::Enter), &[]).unwrap(), "peer add 192.0.2.7:7001 Workshop node");
        page.command_result(&Err(crate::Error::Invalid("save failed")));
        assert!(page.editor.is_some());
        page.command_result(&Ok(true));
        assert!(page.editor.is_none());
    }
    #[test]
    fn optional_dropdown_selects_profile_and_its_default_port_and_preserves_edits() {
        let mut page = NetworkPage::default();
        page.input(key(KeyCode::Char('a')), &[]);
        type_text(&mut page, "192.0.2.7");
        page.input(key(KeyCode::Enter), &[]);
        type_text(&mut page, "Luka laptop");
        page.input(key(KeyCode::Tab), &[]);
        page.input(key(KeyCode::Char(' ')), &[]);
        assert!(page.editor.as_ref().unwrap().dropdown);
        page.input(key(KeyCode::Down), &[]);
        page.input(key(KeyCode::Enter), &[]);
        assert_eq!(page.input(key(KeyCode::Enter), &[]).unwrap(), "peer add 192.0.2.7:8080 --compat grupa1-luka Luka laptop");
        page.command_result(&Ok(true));
        let peers = vec![crate::settings::configured_peer("192.0.2.7", &["--compat", "grupa1-luka", "Luka"]).unwrap()];
        page.input(key(KeyCode::Enter), &peers);
        assert_eq!(page.editor.as_ref().unwrap().compatibility, CompatibilityProfile::Grupa1Luka);
        assert_eq!(page.input(key(KeyCode::Enter), &peers).unwrap(), "peer edit 192.0.2.7:8080 192.0.2.7:8080 --compat grupa1-luka Luka");
        page.input(key(KeyCode::Tab), &peers);
        page.input(key(KeyCode::Left), &peers);
        assert_eq!(page.input(key(KeyCode::Enter), &peers).unwrap(), "peer edit 192.0.2.7:8080 192.0.2.7:8080 --compat native Luka");
    }
    #[test]
    fn edit_remove_and_cancel_keep_the_target_endpoint() {
        let peers = vec![SavedPeer::new("[::1]:7002".parse().unwrap(), "Local").unwrap()];
        let mut page = NetworkPage::default();
        page.input(key(KeyCode::Enter), &peers);
        assert_eq!(page.input(key(KeyCode::Enter), &peers).unwrap(), "peer edit [::1]:7002 [::1]:7002 --compat native Local");
        page.input(key(KeyCode::Esc), &peers);
        assert_eq!(page.input(key(KeyCode::Delete), &peers).unwrap(), "peer remove [::1]:7002");
    }

    #[test]
    fn peer_list_and_editor_render_at_normal_and_small_sizes() {
        use ratatui::{Terminal, backend::TestBackend};
        let status = serde_json::json!({"online":true,"listen":"0.0.0.0:7001","network_peers":[{"address":"192.0.2.1:7001","alias":"Workshop","status":"Connected","sessions":1}],"network_sessions":[],"startup_peers":[]});
        let mut page = NetworkPage::default();
        let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
        terminal.draw(|frame| page.draw(frame, frame.area(), &status)).unwrap();
        let text: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Workshop") && text.contains("Connected") && text.contains("192.0.2.1:7001"));
        page.input(key(KeyCode::Char('a')), &[]);
        for (width, height) in [(100, 25), (36, 10)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    page.draw(frame, frame.area(), &status);
                    page.draw_editor(frame);
                })
                .unwrap();
        }
    }
}
