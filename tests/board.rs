use claudibeat::board::{BoardFile, Column, Mailbox, Request, Target, holder, project_folders, request_id, request_json};
use claudibeat::render::Rect;
use claudibeat::render::board::ghost_at;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("claudibeat-board-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_board_file_the_mod_writes_parses() {
    let text = r#"{"v":1,"updatedAt":5,"error":null,"projects":["SEO","WEB"],
        "todo":[{"key":"SEO-15","title":"Каталоги","project":"SEO","group":"unstarted","stateName":"В плане","priority":"high","description":"","url":"https://p/SEO-15/"}],
        "doing":[{"key":"WEB-3","title":"Голос","project":"WEB","group":"started","stateName":"В работе","priority":"none","description":"x","url":"u"}],
        "doneToday":[]}"#;
    let b: BoardFile = serde_json::from_str(text).unwrap();
    assert_eq!(b.column_of("WEB-3"), Some(Column::Doing));
    assert_eq!(b.card("SEO-15").map(|c| c.priority.as_str()), Some("high"));
    assert_eq!(b.filtered(Column::Todo, Some("WEB")).len(), 0);
    assert_eq!(b.filtered(Column::Todo, None).len(), 1);
}

#[test]
fn requests_are_the_contract_word_for_word() {
    let assign = request_json("cb-1-00aa", 7, &Request::Assign { task: "WEB-281".into(), target: Target::Session("local_x".into()) });
    let v: serde_json::Value = serde_json::from_str(&assign).unwrap();
    assert_eq!(v["kind"], "assign");
    assert_eq!(v["by"], "claudibeat");
    assert_eq!(v["target"]["session"], "local_x");
    let new = request_json("cb-1-00ab", 7, &Request::Assign { task: "SEO-1".into(), target: Target::NewChat { cwd: "D:\\Code\\a \"b\"".into(), project: "a".into() } });
    let v: serde_json::Value = serde_json::from_str(&new).unwrap();
    assert_eq!(v["target"]["newChat"]["cwd"], "D:\\Code\\a \"b\"");
    let mv = request_json("cb-1-00ac", 7, &Request::Move { task: "SEO-1".into(), to: Column::Done });
    let v: serde_json::Value = serde_json::from_str(&mv).unwrap();
    assert_eq!((v["kind"].as_str(), v["to"].as_str()), (Some("move"), Some("done")));
    // A drop on a column is a move, even when asked as an assign.
    let col = request_json("cb-1-00ad", 7, &Request::Assign { task: "SEO-1".into(), target: Target::Column(Column::Todo) });
    assert!(col.contains(r#""kind":"move""#) && col.contains(r#""to":"todo""#));
    assert!(request_json("cb-1-00ae", 7, &Request::Refresh).contains(r#""kind":"refresh""#));
    assert_eq!(request_id(1_794_930_000_000, 0x1_a1f3), "cb-1794930000000-a1f3");
}

#[test]
fn a_reply_ends_the_request_and_stays_for_the_leader() {
    let dir = tmp("mail");
    let m = Mailbox::new(&dir);
    m.send("cb-1-0001", 1, &Request::Refresh).unwrap();
    assert!(dir.join("requests/cb-1-0001.json").exists());
    assert!(m.reply("cb-1-0001").is_none());
    std::fs::create_dir_all(dir.join("replies")).unwrap();
    std::fs::write(dir.join("replies/cb-1-0001.json"), r#"{"v":1,"id":"cb-1-0001","at":2,"ok":true,"text":"обновлено"}"#).unwrap();
    assert_eq!(m.reply("cb-1-0001").map(|r| r.text), Some("обновлено".into()));
    m.forget("cb-1-0001");
    assert!(!dir.join("requests/cb-1-0001.json").exists());
    assert!(dir.join("replies/cb-1-0001.json").exists());
    // Long after, the sweep takes the reply.
    m.sweep(10_000_000);
    assert!(!dir.join("replies/cb-1-0001.json").exists());
}

#[test]
fn a_torn_or_foreign_reply_is_no_reply() {
    let dir = tmp("torn");
    let m = Mailbox::new(&dir);
    std::fs::create_dir_all(dir.join("replies")).unwrap();
    std::fs::write(dir.join("replies/cb-2-0001.json"), r#"{"v":1,"id":"cb-2-00"#).unwrap();
    assert!(m.reply("cb-2-0001").is_none());
    std::fs::write(dir.join("replies/cb-2-0001.json"), r#"{"v":1,"id":"other","ok":true}"#).unwrap();
    assert!(m.reply("cb-2-0001").is_none());
}

#[test]
fn folders_for_a_new_chat_freshest_first_once_each() {
    let rows = vec![
        (r"D:\Code\a".to_owned(), "a".to_owned(), 1),
        ("".to_owned(), "".to_owned(), 9),
        (r"D:\Code\b".to_owned(), "b".to_owned(), 5),
        (r"d:\code\A".to_owned(), "a".to_owned(), 3),
    ];
    let f = project_folders(&rows, 5);
    assert_eq!(f.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    assert_eq!(project_folders(&rows, 1).len(), 1);
}

#[test]
fn the_latest_assignment_holds_the_card() {
    use claudibeat::board::Assignment;
    let a = |task: &str, s: &str| Assignment { task: task.into(), session: s.into(), session_title: s.into(), at: 0 };
    let list = [a("X-1", "first"), a("X-1", "second"), a("X-2", "other")];
    assert_eq!(holder(&list, "X-1").map(|a| a.session.as_str()), Some("second"));
    assert!(holder(&list, "X-3").is_none());
}

#[test]
fn the_card_in_hand_never_leaves_the_board() {
    let card = Rect::new(20.0, 16.0, 1000.0, 600.0);
    assert_eq!(ghost_at(card, 100.0, 100.0, 248.0, 50.0), (114.0, 114.0));
    let (x, _) = ghost_at(card, 900.0, 100.0, 248.0, 50.0);
    assert!(x + 248.0 <= card.right());
    let (_, y) = ghost_at(card, 100.0, 590.0, 248.0, 50.0);
    assert!(y + 50.0 <= card.bottom());
}

#[test]
fn work_in_review_sits_with_the_done_and_still_closes_on_a_drop() {
    let text = r#"{"v":1,"updatedAt":5,"projects":["WEB"],
        "todo":[],
        "doing":[{"key":"WEB-1","title":"a","project":"WEB","stateName":"В работе"},{"key":"WEB-2","title":"b","project":"WEB","stateName":"На проверке"}],
        "doneToday":[{"key":"WEB-3","title":"c","project":"WEB","stateName":"Готово"}]}"#;
    let b: BoardFile = serde_json::from_str(text).unwrap();
    let keys = |c: Column| b.column(c).iter().map(|k| k.key.clone()).collect::<Vec<_>>();
    assert_eq!(keys(Column::Doing), ["WEB-1"]);
    assert_eq!(keys(Column::Done), ["WEB-2", "WEB-3"]);
    assert_eq!(b.column_of("WEB-2"), Some(Column::Done));
    // In Plane it is still in progress: a drop on «done» moves it there.
    assert_eq!(b.state_column("WEB-2"), Some(Column::Doing));
    assert_eq!(b.state_column("WEB-3"), Some(Column::Done));
}
