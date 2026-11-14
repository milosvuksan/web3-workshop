use super::*;

pub(super) fn owned_selection(
    owned: &BTreeMap<usize, Acquisition>,
    previous: Option<usize>,
) -> Option<usize> {
    previous
        .filter(|id| owned.contains_key(id))
        .or_else(|| {
            owned
                .range(previous.unwrap_or(0)..)
                .next()
                .map(|(id, _)| *id)
        })
        .or_else(|| owned.keys().next().copied())
}
pub(super) fn owned_menu(frame: &mut Frame, area: Rect, view: &View<'_>, selected: Option<usize>) {
    if view.owned.is_empty() {
        frame.render_widget(Paragraph::new("No parcels are currently owned by this identity.\n\nMine a block or receive a confirmed transfer.\nPending transfers appear here after confirmation.").block(panel("Owned parcels")).wrap(Wrap { trim: false }), area);
        return;
    }
    let regions = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area)
    };
    let items = view.owned.iter().map(|(id, receipt)| {
        let color = view.display_color(*id);
        let name = colors::name(color).unwrap_or("custom");
        ListItem::new(vec![
            Line::from(vec![
                Span::raw(format!("{id:4} ({:2},{:2}) ", id % WIDTH, id / WIDTH)),
                Span::styled("  ", Style::default().bg(colors::rgb(color))),
                Span::raw(format!(
                    " #{color} {name}{}",
                    if view.pending.contains_key(id) {
                        " · pending"
                    } else {
                        ""
                    }
                )),
            ]),
            Line::from(format!(
                "     {} • {}",
                receipt.method(),
                timestamp_utc(receipt.timestamp)
            )),
        ])
    });
    let selected_index = view.owned.keys().position(|id| Some(*id) == selected);
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("Owned • ↑/↓ select • Enter color • g map"))
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        regions[0],
        &mut ListState::default().with_selected(selected_index),
    );
    if let Some(id) = selected.and_then(|id| view.owned.contains_key(&id).then_some(id)) {
        let receipt = &view.owned[&id];
        let source = match receipt.source {
            Source::Mining => "Mining reward".into(),
            Source::Transfer {
                sender,
                transaction,
            } => format!("Transfer\nFrom: {sender}\nTransaction: {transaction}"),
        };
        let color_status = view.pending.get(&id).map_or_else(
            || "Confirmed color".into(),
            |paint| {
                format!(
                    "PENDING #{}\nWaiting for block confirmation.\n{}",
                    paint.color,
                    if view.status["mining"] == true {
                        "Mining is active."
                    } else {
                        "Mining stopped: m starts it."
                    }
                )
            },
        );
        let text = format!(
            "Parcel {id} • x={} y={}\nConfirmed #{}\n{color_status}\n\nReceived: {}\nVia: {source}\nHeight: {}\nBlock: {}\n\nTime is the confirming block timestamp.\n\nEnter / p: edit color\nt: transfer • g: locate on canvas",
            id % WIDTH,
            id / WIDTH,
            view.state.parcels[id].color,
            timestamp_utc(receipt.timestamp),
            receipt.height,
            receipt.block
        );
        frame.render_widget(
            Paragraph::new(text)
                .block(panel("Acquisition details"))
                .wrap(Wrap { trim: false }),
            regions[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Bytes;
    fn receipt() -> Acquisition {
        Acquisition {
            height: 1,
            timestamp: 1,
            block: Bytes::default(),
            source: Source::Mining,
        }
    }
    #[test]
    fn selection_tracks_parcel_ids_across_transfers_and_reorgs() {
        let mut owned = BTreeMap::from([(5, receipt()), (10, receipt()), (20, receipt())]);
        assert_eq!(owned_selection(&owned, Some(10)), Some(10));
        owned.remove(&5);
        assert_eq!(owned_selection(&owned, Some(10)), Some(10));
        owned.remove(&10);
        assert_eq!(owned_selection(&owned, Some(10)), Some(20));
        owned.clear();
        assert_eq!(owned_selection(&owned, Some(20)), None);
    }
}
