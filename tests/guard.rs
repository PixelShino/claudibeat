#[test]
fn the_log_takes_a_line() {
    let dir = std::env::temp_dir().join(format!("claudibeat-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: a test of its own process; nothing else reads APPDATA here.
    unsafe { std::env::set_var("CLAUDIBEAT_LOG_DIR", &dir) };
    claudibeat::guard::log("hello");
    let text = std::fs::read_to_string(dir.join("claudibeat").join("claudibeat.log")).unwrap();
    assert!(text.contains("hello"), "{text}");
}

#[test]
fn a_process_started_from_a_claude_chat_knows_it() {
    use claudibeat::win::shell::ancestor_named;
    let t = |rows: &[(u32, u32, &str)]| rows.iter().map(|&(a, b, c)| (a, b, c.to_owned())).collect::<Vec<_>>();
    let chat = t(&[(1, 0, "System"), (10, 1, "claude.exe"), (20, 10, "powershell.exe"), (30, 20, "claudibeat.exe")]);
    assert_eq!(ancestor_named(&chat, 30, "claude.exe"), Some("claude.exe (pid 10) > powershell.exe".into()));
    // A task's svchost whose long-dead grandparent's id now belongs to Claude.
    let reused = t(&[(10, 2, "claude.exe"), (4, 10, "services.exe"), (5, 4, "svchost.exe"), (30, 5, "claudibeat.exe")]);
    assert_eq!(ancestor_named(&reused, 30, "claude.exe"), None);
    let task = t(&[(1, 0, "System"), (5, 1, "svchost.exe"), (30, 5, "claudibeat.exe")]);
    assert_eq!(ancestor_named(&task, 30, "claude.exe"), None);
    let cycle = t(&[(30, 31, "claudibeat.exe"), (31, 32, "a.exe"), (32, 31, "b.exe")]);
    assert_eq!(ancestor_named(&cycle, 30, "claude.exe"), None);
}
