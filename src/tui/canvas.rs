use super::*;

/// The image stays fixed as the selection moves. A terminal cell is roughly
/// twice as tall as it is wide, so two raster rows share a half-block cell.
pub(super) fn canvas(
    frame: &mut Frame,
    area: Rect,
    view: &View<'_>,
    selected: usize,
    ownership: bool,
    show_owned: bool,
) {
    let side_by_side = area.width >= 68;
    let regions = if side_by_side {
        Layout::horizontal([Constraint::Min(1), Constraint::Length(30)]).split(area)
    } else {
        // A compact inspector leaves the map usable on short, narrow terminals.
        let inspector_height = if area.height >= 22 { 3 } else { 2 };
        Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(inspector_height.min(area.height.saturating_sub(3))),
        ])
        .split(area)
    };
    let map = map_rect(regions[0]);
    let overview = usize::from(map.width) < WIDTH;
    let title = format!(
        "{} • {}×{} • {}",
        if ownership { "Ownership" } else { "Canvas" },
        WIDTH,
        HEIGHT,
        if overview {
            "averaged overview"
        } else {
            "full map"
        },
    );
    frame.render_widget(panel(&title), regions[0]);
    draw_map(frame, map, view, selected, ownership, show_owned);
    if side_by_side {
        inspector(frame, regions[1], view, selected, ownership, show_owned);
    } else {
        compact_inspector(frame, regions[1], view, selected);
    }
}

fn map_rect(area: Rect) -> Rect {
    let inner = panel("").inner(area);
    // Leave a column on each side for cursor guides. Cap magnification at 4×;
    // beyond that the individual parcels stop looking like a pixel canvas.
    let size = inner
        .width
        .saturating_sub(2)
        .min(inner.height.saturating_mul(2))
        .min((WIDTH * 4) as u16)
        / 2
        * 2;
    Rect::new(
        inner.x + (inner.width - size) / 2,
        inner.y + (inner.height - size / 2) / 2,
        size,
        size / 2,
    )
}

fn source_range(index: usize, samples: usize, length: usize) -> std::ops::Range<usize> {
    let start = index * length / samples;
    start..((index + 1) * length / samples).max(start + 1)
}

fn draw_map(
    frame: &mut Frame,
    map: Rect,
    view: &View<'_>,
    selected: usize,
    ownership: bool,
    show_owned: bool,
) {
    let size = usize::from(map.width);
    if size == 0 {
        return;
    }
    let mut cursor_x = (size, 0);
    let mut cursor_y = (size, 0);
    for y in (0..size).step_by(2) {
        for x in 0..size {
            let sample = |row| {
                let mut total = [0u32; 3];
                let mut count = 0;
                let mut mine = false;
                let mut cursor = false;
                for py in source_range(row, size, HEIGHT) {
                    for px in source_range(x, size, WIDTH) {
                        let id = py * WIDTH + px;
                        let rgb = if ownership {
                            view.state.parcels[id].owner.map_or([24, 28, 36], |key| {
                                [key.0[0].max(48), key.0[1].max(48), key.0[2].max(48)]
                            })
                        } else {
                            view.display_color(id).0
                        };
                        for channel in 0..3 {
                            total[channel] += u32::from(rgb[channel]);
                        }
                        count += 1;
                        mine |= show_owned && view.owned.contains_key(&id);
                        cursor |= selected == id;
                    }
                }
                (
                    Color::Rgb(
                        (total[0] / count) as u8,
                        (total[1] / count) as u8,
                        (total[2] / count) as u8,
                    ),
                    mine,
                    cursor,
                )
            };
            let top = sample(y);
            let bottom = sample(y + 1);
            if top.2 || bottom.2 {
                cursor_x = (cursor_x.0.min(x), cursor_x.1.max(x));
                let row = if top.2 { y } else { y + 1 };
                cursor_y = (cursor_y.0.min(row), cursor_y.1.max(row));
            }
            let cell = &mut frame.buffer_mut()[(map.x + x as u16, map.y + (y / 2) as u16)];
            cell.set_symbol("▀").set_fg(top.0).set_bg(bottom.0);
            // A thin colored underline keeps both parcel colors intact. At
            // compact sizes it indicates ownership within this pair/bin.
            if top.2 || bottom.2 || top.1 || bottom.1 {
                cell.set_style(
                    Style::default()
                        .add_modifier(Modifier::UNDERLINED)
                        .underline_color(if top.2 || bottom.2 {
                            ACCENT
                        } else {
                            Color::Yellow
                        }),
                );
            }
        }
    }
    // Cursor guides live outside the image, making even black selections
    // visible without replacing RGB pixels. Moving them never pans the map.
    let x = map.x + ((cursor_x.0 + cursor_x.1) / 2) as u16;
    let y = map.y + ((cursor_y.0 + cursor_y.1) / 4) as u16;
    for (position, symbol) in [
        ((x, map.bottom()), "▲"),
        ((map.x - 1, y), "▸"),
        ((map.right(), y), "◂"),
    ] {
        frame.buffer_mut()[position]
            .set_symbol(symbol)
            .set_fg(ACCENT);
    }
}

fn compact_inspector(frame: &mut Frame, area: Rect, view: &View<'_>, selected: usize) {
    let parcel = &view.state.parcels[selected];
    let pending = view.pending.contains_key(&selected) && view.owned.contains_key(&selected);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" #{selected} ({}, {}) ", selected % WIDTH, selected / WIDTH),
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                "██",
                Style::default().fg(colors::rgb(view.display_color(selected))),
            ),
            Span::raw(format!(
                " #{} {}",
                view.display_color(selected),
                if pending {
                    "PENDING"
                } else if view.owned.contains_key(&selected) {
                    "YOURS"
                } else if parcel.owner.is_none() {
                    "Unclaimed"
                } else {
                    "Claimed"
                }
            )),
        ]),
        Line::from(" ↑↓←→ • p color • t transfer • v/h"),
        Line::from(" Cyan guides: selected • yellow underline: owned here"),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn inspector(
    frame: &mut Frame,
    area: Rect,
    view: &View<'_>,
    selected: usize,
    ownership: bool,
    show_owned: bool,
) {
    let block = panel("Selected parcel");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 8 {
        compact_inspector(frame, inner, view, selected);
        return;
    }
    let regions = Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).split(inner);
    let parcel = &view.state.parcels[selected];
    let mine = view.owned.contains_key(&selected);
    let pending = view.pending.get(&selected).filter(|_| mine);
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Parcel {selected} • ({}, {})",
                selected % WIDTH,
                selected / WIDTH
            ),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            if mine {
                "YOURS"
            } else if parcel.owner.is_none() {
                "Unclaimed"
            } else {
                "Owned by another identity"
            },
            Style::default().fg(if mine { Color::Yellow } else { Color::Gray }),
        )),
        Line::from(vec![
            Span::styled(
                "██ ",
                Style::default().fg(colors::rgb(view.display_color(selected))),
            ),
            Span::raw(format!("#{}", view.display_color(selected))),
            Span::styled(
                if pending.is_some() {
                    " PENDING"
                } else {
                    " confirmed"
                },
                Style::default().fg(if pending.is_some() {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ),
        ]),
        Line::from(format!("Counter {}", parcel.counter)),
    ];
    if pending.is_some() {
        lines.push(Line::from(format!("Confirmed #{}", parcel.color)));
        if regions[0].height >= 8 {
            lines.push(Line::from(if view.status["mining"] == true {
                "Awaiting a mined block"
            } else {
                "m mine • or wait for a peer"
            }));
        }
    }
    if regions[0].height >= 10 {
        if lines.len() + 5 <= usize::from(regions[0].height) {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            "Owner",
            Style::default().fg(Color::DarkGray),
        )));
        let owner = parcel.owner.map_or_else(
            || "Unclaimed • mine to acquire".into(),
            |key| key.to_string(),
        );
        lines.push(Line::from(owner));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), regions[0]);
    let controls = vec![
        Line::from("↑↓←→ select • p color"),
        Line::from("t transfer • 7 owned parcels"),
        Line::from(format!(
            "v {} • h markers {}",
            if ownership { "ownership" } else { "RGB" },
            if show_owned { "on" } else { "off" }
        )),
        Line::from(vec![
            Span::styled("Cyan cursor", Style::default().fg(ACCENT)),
            Span::raw(" • "),
            Span::styled("yellow owned", Style::default().fg(Color::Yellow)),
        ]),
    ];
    frame.render_widget(Paragraph::new(controls), regions[1]);
}
