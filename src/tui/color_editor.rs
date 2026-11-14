use super::*;

pub(super) enum EditAction {
    Continue,
    Cancel,
    Submit(String),
}
pub(super) struct ColorEditor {
    parcel: usize,
    query: String,
    selected: usize,
    explicit_selection: bool,
    error: Option<String>,
}
impl ColorEditor {
    pub(super) fn parcel(&self) -> usize {
        self.parcel
    }
    pub(super) fn reject(&mut self, reason: &str) {
        self.error = Some(reason.into());
    }
    pub(super) fn new(parcel: usize) -> Self {
        Self {
            parcel,
            query: String::new(),
            selected: 0,
            explicit_selection: false,
            error: None,
        }
    }
    fn chosen(&self) -> Result<crate::protocol::Color> {
        let matches = colors::suggestions(&self.query);
        if self.explicit_selection
            && let Some(name) = matches.get(self.selected)
        {
            return colors::parse(name);
        }
        if let Ok(color) = colors::parse(&self.query) {
            return Ok(color);
        }
        if matches.len() == 1 {
            return colors::parse(matches[0]);
        }
        Err(crate::Error::Invalid(
            "Choose a color: type its full name or hex, or use arrows/Tab to select a suggestion",
        ))
    }
    pub(super) fn input(&mut self, key: KeyEvent) -> EditAction {
        let count = colors::suggestions(&self.query).len();
        match key.code {
            KeyCode::Esc => return EditAction::Cancel,
            KeyCode::Enter => match self.chosen() {
                Ok(color) => return EditAction::Submit(format!("paint {} {}", self.parcel, color)),
                Err(error) => self.error = Some(error.to_string()),
            },
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.explicit_selection = true;
                self.error = None;
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
                self.explicit_selection = true;
                self.error = None;
            }
            KeyCode::Tab => {
                if let Some(name) = colors::suggestions(&self.query).get(self.selected) {
                    self.query = (*name).into();
                    self.selected = 0;
                    self.explicit_selection = false;
                    self.error = None;
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                self.explicit_selection = false;
                self.error = None;
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && self.query.len() < 64 =>
            {
                self.query.push(c);
                self.selected = 0;
                self.explicit_selection = false;
                self.error = None;
            }
            _ => {}
        }
        EditAction::Continue
    }
    pub(super) fn draw(&self, frame: &mut Frame, view: &View<'_>) {
        let screen = frame.area();
        let width = screen.width.saturating_sub(2).min(70);
        let height = screen.height.saturating_sub(2).min(23);
        let area = centered(screen, width, height);
        frame.render_widget(Clear, area);
        let block =
            panel("Edit parcel color • Esc cancels").border_style(Style::default().fg(ACCENT));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let compact = inner.height < 10;
        let rows = Layout::vertical([
            Constraint::Length(if compact { 1 } else { 3 }),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(if compact { 1 } else { 3 }),
        ])
        .split(inner);
        let current = view.state.parcels[self.parcel].color;
        let preview = self.chosen().unwrap_or(view.display_color(self.parcel));
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    "Parcel {} ({}, {}) • current #{}",
                    self.parcel,
                    self.parcel % WIDTH,
                    self.parcel / WIDTH,
                    current
                )),
                Line::from(vec![
                    Span::raw("Preview "),
                    Span::styled("      ", Style::default().bg(colors::rgb(preview))),
                    Span::raw(format!(" #{}", preview)),
                ]),
                Line::from(view.pending.get(&self.parcel).map_or_else(
                    || "No pending edit".into(),
                    |paint| format!("Pending #{} • awaiting block confirmation", paint.color),
                )),
            ]),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(format!("> {}█", self.query))
                .style(Style::default().fg(ACCENT))
                .block(Block::default().title("Color name or #RRGGBB")),
            rows[1],
        );
        let matches = colors::suggestions(&self.query);
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new(if colors::parse(&self.query).is_ok() {
                    "Custom RGB color • Enter to submit"
                } else {
                    "No named matches. Enter six RGB hex digits, or edit the name."
                })
                .wrap(Wrap { trim: false }),
                rows[2],
            );
        } else {
            let items = matches.iter().map(|name| {
                let color = colors::parse(name).expect("named color");
                ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default().bg(colors::rgb(color))),
                    Span::raw(format!(" {name:<18} #{color}")),
                ]))
            });
            let list = List::new(items)
                .highlight_symbol("› ")
                .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
            frame.render_stateful_widget(
                list,
                rows[2],
                &mut ListState::default().with_selected(Some(self.selected)),
            );
        }
        let note = self.error.as_deref().unwrap_or("↑/↓ select • Tab autocomplete • Enter queue edit\nPending preview appears immediately; a mined block confirms it.");
        frame.render_widget(
            Paragraph::new(note)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(if self.error.is_some() {
                    Color::Red
                } else {
                    Color::Gray
                })),
            rows[3],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    #[test]
    fn selector_completes_names_and_submits_selected_rgb() {
        let mut editor = ColorEditor::new(181);
        for ch in "re".chars() {
            editor.input(key(KeyCode::Char(ch)));
        }
        editor.input(key(KeyCode::Down)); // rebeccapurple -> red
        editor.input(key(KeyCode::Tab));
        assert_eq!(editor.query, "red");
        assert!(
            matches!(editor.input(key(KeyCode::Enter)), EditAction::Submit(command) if command == "paint 181 ff0000")
        );
    }
    #[test]
    fn hex_input_invalid_input_and_cancel() {
        let mut editor = ColorEditor::new(5);
        for ch in "#AB12EF".chars() {
            editor.input(key(KeyCode::Char(ch)));
        }
        assert!(
            matches!(editor.input(key(KeyCode::Enter)), EditAction::Submit(command) if command == "paint 5 ab12ef")
        );
        editor.query = "unknown".into();
        assert!(matches!(
            editor.input(key(KeyCode::Enter)),
            EditAction::Continue
        ));
        assert!(editor.error.is_some());
        assert!(matches!(
            editor.input(key(KeyCode::Esc)),
            EditAction::Cancel
        ));
    }

    #[test]
    fn empty_and_ambiguous_input_cannot_silently_choose_a_different_color() {
        let mut editor = ColorEditor::new(5);
        assert!(matches!(
            editor.input(key(KeyCode::Enter)),
            EditAction::Continue
        ));
        for c in "gr".chars() {
            editor.input(key(KeyCode::Char(c)));
        }
        assert!(matches!(
            editor.input(key(KeyCode::Enter)),
            EditAction::Continue
        ));
        for c in "een".chars() {
            editor.input(key(KeyCode::Char(c)));
        }
        assert!(
            matches!(editor.input(key(KeyCode::Enter)), EditAction::Submit(command) if command == "paint 5 008000")
        );
    }
}
