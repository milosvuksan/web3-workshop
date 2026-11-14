use super::*;

pub(super) fn settings_panel(
    frame: &mut Frame,
    area: Rect,
    status: &Value,
    selected: usize,
    draft: usize,
    port: u16,
    difficulty: u16,
) {
    let regions = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Min(1),
    ])
    .split(area);
    let summary = format!(
        "Mining thread setting: {}\nConfigured workers: {} • active: {}\nAvailable logical CPUs: {}\nTCP listen address: {}
Local difficulty: {} leading zero bits (network minimum: 23)",
        status["mining_thread_setting"].as_str().unwrap_or("1"),
        status["mining_threads"],
        status["active_mining_threads"],
        status["available_threads"],
        status["listen"],
        status["mining_difficulty"]
    );
    frame.render_widget(
        Paragraph::new(summary).block(panel("Settings • mining and network")),
        regions[0],
    );
    let all = format!(
        "Use all available ({} workers)",
        status["available_threads"]
    );
    let exact = format!("Use exact count: {draft}    ←/→ adjust");
    let tcp = format!("TCP port: {port}    ←/→ adjust • n enters a port");
    let difficulty = format!("Local zero bits: {difficulty}    ←/→ adjust • n enters a value");
    menu(
        frame,
        regions[1],
        "↑/↓ choose • Enter applies and saves",
        &[&all, &exact, &tcp, &difficulty],
        selected,
    );
    frame.render_widget(Paragraph::new("↑/↓ selects a setting • ←/→ or +/- adjusts its value\nn opens a prompt • Enter applies and saves\n\nMining changes restart active workers; stopped mining stays stopped.
Local difficulty: 23–256 leading zero bits. Higher values slow this miner; the network minimum remains 23. Each extra bit doubles expected work.\n\nTCP port: 1–65535 (default 7001). Online changes reopen the listener and peer connections immediately. A busy port leaves the current network running. Offline nodes stay offline.\n\nSaved in settings.json in this node's data directory. Startup --threads, --port or --listen overrides the saved preference for that run.").block(panel("How to change settings")).wrap(Wrap { trim: false }), regions[2]);
}
