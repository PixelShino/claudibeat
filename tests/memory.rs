use claudibeat::memory::{MemGroups, Proc, group, sample};

fn p(pid: u32, parent: u32, name: &str, mb: u64) -> Proc {
    Proc { pid, parent, name: name.into(), private_ws: mb << 20 }
}

const MB: u64 = 1 << 20;

#[test]
fn claude_tree_includes_grandchildren() {
    let procs = [
        p(1, 0, "claude.exe", 100),
        p(2, 1, "node.exe", 10),
        p(3, 2, "cmd.exe", 1),
        p(4, 0, "chrome.exe", 50),
        p(5, 4, "chrome.exe", 5),
        p(6, 99, "python.exe", 7),
        p(7, 0, "explorer.exe", 70),
    ];
    let m = group(&procs, 1000 * MB, 2000 * MB);
    assert_eq!(m.claude, 111 * MB);
    assert_eq!(m.chrome, 55 * MB);
    assert_eq!(m.python, 7 * MB);
    assert_eq!(m.other, (1000 - 111 - 55 - 7) * MB);
}

#[test]
fn python_under_claude_counts_as_claude() {
    let procs = [p(1, 0, "claude.exe", 1), p(2, 1, "python.exe", 9), p(3, 0, "pythonw.exe", 4), p(4, 0, "python3.14.exe", 2)];
    let m = group(&procs, 100 * MB, 200 * MB);
    assert_eq!(m.claude, 10 * MB);
    assert_eq!(m.python, 6 * MB);
}

#[test]
fn chrome_under_claude_counts_as_claude() {
    let procs = [p(1, 0, "claude.exe", 1), p(2, 1, "chrome.exe", 9), p(3, 2, "chrome.exe", 4)];
    let m = group(&procs, 100 * MB, 200 * MB);
    assert_eq!((m.claude, m.chrome), (14 * MB, 0));
}

#[test]
fn pid_cycle_does_not_hang() {
    let procs = [p(1, 2, "claude.exe", 1), p(2, 1, "node.exe", 2)];
    assert_eq!(group(&procs, 10 * MB, 20 * MB).claude, 3 * MB);
}

#[test]
fn other_never_negative() {
    let procs = [p(1, 0, "claude.exe", 500)];
    assert_eq!(group(&procs, 100 * MB, 200 * MB).other, 0);
}

#[test]
fn alert_threshold() {
    let at = |used: u64| MemGroups { claude: 0, chrome: 0, python: 0, other: 0, used, total: 1000 };
    assert!(at(850).alert());
    assert!(!at(849).alert());
    assert_eq!(at(756).percent(), 76);
    assert_eq!(MemGroups::default().percent(), 0, "no total, no division by zero");
}

#[test]
#[ignore = "samples this machine"]
fn live_sample_sane() {
    let m = sample();
    assert!(m.total > 0 && m.used <= m.total && m.claude <= m.used + m.claude);
    println!("LIVE claude={} chrome={} python={} other={} used={} total={}", m.claude >> 20, m.chrome >> 20, m.python >> 20, m.other >> 20, m.used >> 20, m.total >> 20);
}
