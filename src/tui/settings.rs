use super::*;

pub(super) fn settings_panel(
    frame: &mut Frame,
    area: Rect,
    status: &Value,
    selected: usize,
    draft: usize,
) {
    let regions = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Min(1),
    ])
    .split(area);
    let summary = format!(
        "Mining thread setting: {}\nConfigured workers: {} • active: {}\nAvailable logical CPUs: {}",
        status["mining_thread_setting"].as_str().unwrap_or("1"),
        status["mining_threads"],
        status["active_mining_threads"],
        status["available_threads"]
    );
    frame.render_widget(
        Paragraph::new(summary).block(panel("Settings • mining CPU workers")),
        regions[0],
    );
    let all = format!(
        "Use all available ({} workers)",
        status["available_threads"]
    );
    let exact = format!("Use exact count: {draft}    ←/→ adjust");
    menu(
        frame,
        regions[1],
        "↑/↓ choose • Enter applies and saves",
        &[&all, &exact],
        selected,
    );
    frame.render_widget(Paragraph::new("Select all available or an exact worker count.\n\n↑/↓ selects a mode • ←/→ or +/- adjusts the exact count\nn opens a count prompt (1–1024, or all) • Enter saves\n\nChanges take effect immediately. Active mining workers restart with the new count; stopped mining stays stopped. Networking and offline mode keep their current settings.\n\nSaved in this node's data directory as settings.json. Startup --threads COUNT|all overrides the saved preference for that run.\n\nThis setting controls mining workers. The terminal and network continue running independently.").block(panel("How to change thread usage")).wrap(Wrap { trim: false }), regions[2]);
}
