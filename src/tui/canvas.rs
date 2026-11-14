use super::*;
use ratatui::widgets::BorderType;

const TILE_WIDTH: u16 = 4;
const TILE_HEIGHT: u16 = 3;

pub(super) fn canvas(
    frame: &mut Frame,
    area: Rect,
    view: &View<'_>,
    selected: usize,
    ownership: bool,
    show_owned: bool,
) {
    let regions = if area.width >= 100 {
        Layout::horizontal([Constraint::Min(8), Constraint::Length(36)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(3), Constraint::Length(8)]).split(area)
    };
    let mut block = panel("Canvas • arrows move • yellow outline = YOURS");
    let inner = block.inner(regions[0]);
    let columns = usize::from(inner.width / TILE_WIDTH).min(64);
    let rows = usize::from(inner.height / TILE_HEIGHT).min(32);
    let x0 = (selected % 64)
        .saturating_sub(columns / 2)
        .min(64 - columns);
    let y0 = (selected / 64).saturating_sub(rows / 2).min(32 - rows);
    if columns > 0 && rows > 0 {
        block = block.title(format!(
            "{} • x {}–{} y {}–{}",
            if ownership { "Ownership" } else { "Canvas" },
            x0,
            x0 + columns - 1,
            y0,
            y0 + rows - 1
        ));
    }
    frame.render_widget(block, regions[0]);
    for y in 0..rows {
        for x in 0..columns {
            let id = (y0 + y) * 64 + x0 + x;
            let mine = view.owned.contains_key(&id);
            let outlined = mine && show_owned;
            let tile = Rect::new(
                inner.x + x as u16 * TILE_WIDTH,
                inner.y + y as u16 * TILE_HEIGHT,
                TILE_WIDTH,
                TILE_HEIGHT,
            );
            let color = if ownership {
                view.state.parcels[id]
                    .owner
                    .map_or(Color::Rgb(24, 28, 36), |key| {
                        Color::Rgb(key.0[0].max(48), key.0[1].max(48), key.0[2].max(48))
                    })
            } else {
                colors::rgb(view.display_color(id))
            };
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(if outlined {
                        BorderType::Thick
                    } else {
                        BorderType::Plain
                    })
                    .border_style(Style::default().fg(if outlined {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    })),
                tile,
            );
            for offset in 1..=2 {
                frame.buffer_mut()[(tile.x + offset, tile.y + 1)]
                    .set_symbol(" ")
                    .set_bg(color);
            }
            if selected == id {
                let fg = match color {
                    Color::Rgb(r, g, b)
                        if u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114
                            > 128_000 =>
                    {
                        Color::Black
                    }
                    _ => Color::White,
                };
                frame.buffer_mut()[(tile.x + 1, tile.y + 1)]
                    .set_symbol("›")
                    .set_fg(fg)
                    .set_style(Style::default().add_modifier(Modifier::BOLD));
            }
        }
    }
    let parcel = &view.state.parcels[selected];
    let owner = parcel
        .owner
        .map_or_else(|| "Unclaimed".into(), |key| key.to_string());
    let pending = view.pending.get(&selected).map_or_else(
        || "Confirmed color".into(),
        |paint| {
            format!(
                "PENDING #{}\nPreview awaits a mined block.\n{}",
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
        "Parcel {selected} • ({}, {})\n{}\nConfirmed #{} • counter {}\n{pending}\n\nOwner: {owner}\n\nYellow thick outline = YOURS\nGray thin outline = other\n› = selected parcel\n\np color • t transfer\nh outlines • v RGB/ownership\n7 owned parcels • arrows pan",
        selected % 64,
        selected / 64,
        if view.owned.contains_key(&selected) {
            "YOURS"
        } else {
            "Not owned by this identity"
        },
        parcel.color,
        parcel.counter
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(panel("Parcel inspector"))
            .wrap(Wrap { trim: false }),
        regions[1],
    );
}
