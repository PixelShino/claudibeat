use claudibeat::win::shell::task_xml;

fn task() -> String {
    task_xml(r"DESKTOP-1\ADMIN", r"C:\Apps\R&D <x>\claudibeat.exe")
}

#[test]
fn the_task_starts_claudibeat_when_this_person_signs_in() {
    let xml = task();
    assert!(xml.contains(r"<LogonTrigger><Enabled>true</Enabled><UserId>DESKTOP-1\ADMIN</UserId><Delay>PT5S</Delay></LogonTrigger>"), "{xml}");
    // Their own desktop, without elevation: the strip has to live in their taskbar.
    assert!(xml.contains(r"<UserId>DESKTOP-1\ADMIN</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel>"), "{xml}");
}

#[test]
fn the_task_never_stops_claudibeat() {
    let xml = task();
    // The default limit is 72 hours; claudibeat runs for the whole session.
    assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"), "{xml}");
    assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
    assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
    assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
    // 4 is normal priority; the scheduler's default 7 is below normal.
    assert!(xml.contains("<Priority>4</Priority>"));
}

#[test]
fn the_path_is_escaped_for_xml() {
    let xml = task();
    assert!(xml.contains(r"<Command>C:\Apps\R&amp;D &lt;x&gt;\claudibeat.exe</Command>"), "{xml}");
    assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-16"?>"#));
}
