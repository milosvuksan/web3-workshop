use super::*;

pub(super) struct TransferMenu {
    parcel: usize,
    selected: usize,
    error: Option<String>,
}

pub(super) enum TransferAction {
    Continue,
    Cancel,
    Manual(usize),
    Submit(String),
}

impl TransferMenu {
    pub fn new(parcel: usize) -> Self {
        Self { parcel, selected: 0, error: None }
    }

    pub fn input(&mut self, key: KeyEvent, peers: &[Value], owned: bool) -> TransferAction {
        self.selected = self.selected.min(peers.len().saturating_sub(1));
        match key.code {
            KeyCode::Esc => TransferAction::Cancel,
            KeyCode::Char('m') => TransferAction::Manual(self.parcel),
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                TransferAction::Continue
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(peers.len().saturating_sub(1));
                TransferAction::Continue
            }
            KeyCode::Enter => {
                if !owned {
                    self.error = Some("This parcel is no longer owned by you.".into());
                } else if let Some(peer) = peers.get(self.selected) {
                    if peer["status"] == "Connected"
                        && let Some(identity) = peer["identity"].as_str()
                    {
                        return TransferAction::Submit(format!("transfer {} {identity}", self.parcel));
                    }
                    self.error = Some(if peer["compatibility"] == "grupa1-luka" {
                        "Grupa 1 does not exchange identities. Press m to enter a public key; this transfers a native parcel.".into()
                    } else {
                        "Connect to this node to learn its identity before transferring.".into()
                    });
                } else {
                    self.error = Some("No saved nodes. Add one on the Network page or press m for manual entry.".into());
                }
                TransferAction::Continue
            }
            _ => TransferAction::Continue,
        }
    }

    pub fn parcel(&self) -> usize {
        self.parcel
    }
    pub fn reject(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn draw(&self, frame: &mut Frame, peers: &[Value]) {
        let area = centered(frame.area(), 80, 16);
        frame.render_widget(Clear, area);
        frame.render_widget(Block::default().borders(Borders::ALL).title(format!(" Transfer parcel {} ", self.parcel)), area);
        let inner = area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(inner);
        let items: Vec<_> = peers
            .iter()
            .map(|peer| {
                ListItem::new(format!(
                    "{} • {} • {}",
                    peer["alias"].as_str().unwrap_or("Node"),
                    peer["address"].as_str().unwrap_or(""),
                    peer["status"].as_str().unwrap_or("Unknown")
                ))
            })
            .collect();
        if items.is_empty() {
            frame.render_widget(Paragraph::new("No saved nodes. Add nodes on the Network page.").wrap(Wrap { trim: false }), rows[0]);
        } else {
            frame.render_stateful_widget(
                List::new(items).highlight_symbol("› ").highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                rows[0],
                &mut ListState::default().with_selected(Some(self.selected.min(peers.len() - 1))),
            );
        }
        frame.render_widget(Paragraph::new(self.error.as_deref().unwrap_or("↑/↓ select • Enter transfer • m enter public key • Esc cancel\nOnly connected nodes with known identities can be selected. A mined block confirms the transfer.")).wrap(Wrap { trim: false }), rows[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_uses_current_connected_identity_and_rejects_unavailable_nodes() {
        let mut menu = TransferMenu::new(42);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let mut peers = vec![
            serde_json::json!({"alias":"Offline", "status":"Offline", "identity":"old"}),
            serde_json::json!({"alias":"Ready", "status":"Connected", "identity":"recipient"}),
        ];
        assert!(matches!(menu.input(key(KeyCode::Enter), &peers, true), TransferAction::Continue));
        menu.input(key(KeyCode::Down), &peers, true);
        assert!(matches!(menu.input(key(KeyCode::Enter), &peers, false), TransferAction::Continue));
        assert!(
            matches!(menu.input(key(KeyCode::Enter), &peers, true), TransferAction::Submit(command) if command == "transfer 42 recipient")
        );
        peers[1]["status"] = "Retrying".into();
        assert!(matches!(menu.input(key(KeyCode::Enter), &peers, true), TransferAction::Continue));
        assert!(matches!(menu.input(key(KeyCode::Char('m')), &peers, true), TransferAction::Manual(42)));
    }
}
