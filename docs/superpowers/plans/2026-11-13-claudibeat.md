# claudibeat: план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** полоса в левом углу панели задач Windows 11 со счётчиками сессий Claude (ждёт / готово / в работе), памятью и лимитом; окно со списками по клику; уведомления, открывающие чат.

**Architecture:** чистые модули без Win32 (модель, сведение сессий, память, лимиты, тексты, правила уведомлений) покрыты `cargo test`. Один рендерер Direct2D/DirectWrite рисует в WIC-битмап с предумноженной альфой; эту картинку получают и PNG (`--render`), и живые окна. Полоса — слоистое окно-потомок `Shell_TrayWnd`, окно и уведомления — верхние окна.

**Tech Stack:** Rust stable, набор `stable-x86_64-pc-windows-gnu` (rustc 1.100), крейты `windows`, `serde`, `serde_json`.

**Spec:** `docs/superpowers/specs/2026-11-13-claudibeat-design.md`. Размеры и цвета берутся из макетов: `docs/mockups/base.css` (классы `.c2`, `.fly`, `.toast`, `.tip`; CSS px = DIP) и `docs/mockups/v2/strips-v2.html` (геометрия значков). Порт сведения сессий делается 1:1 с `~/.claude/mods/status-band/hooks/shared.ts` (`mergeRows`, `rowOf`, `projectOf`) и `~/.claude/mods/status-band/types/index.d.ts`.

## Global Constraints

- Сборка только `stable-x86_64-pc-windows-gnu` (override уже стоит на папке). MSVC, `rc.exe` и `windres` недоступны, поэтому ресурсы и манифест не встраиваются. DPI задаётся вызовом `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` первой строкой `main`.
- Зависимости: `windows`, `serde` (derive), `serde_json`. Любая другая — только с обоснованием в сообщении коммита.
- `#![windows_subsystem = "windows"]` в `main.rs`.
- Сети нет. `~/.claude/.credentials.json` не читать.
- В `%USERPROFILE%\.claude\status-band\` писать только `acks/<safe_key>.json` (`{"v":1,"ackAt":<мс>,"by":"claudibeat"}`) и `tray.json` (`{"v":1,"at":<мс>}`), атомарно: `<имя>.tmp`, затем переименование с заменой.
- Цвета: текст `#1b1b1b`, второстепенный `#5d5d5d`, время `#8a8a8a`, плашка `#1d1d1d`, текст на плашке `#f6f6f6`, наведение `rgba(0,0,0,0.05)`, дорожка кольца и полосок `rgba(0,0,0,0.14)`, фон окон `#f9f9f9`, разделитель `rgba(0,0,0,0.09)`. Нулевые значения рисуются с непрозрачностью `0.42`. Других цветов нет.
- Шрифт Segoe UI Variable Text (запасной Segoe UI), цифры с OpenType-признаком `tnum`, сглаживание текста grayscale.
- Тексты интерфейса по-русски, без длинных тире, десятичная запятая (`24,2 ГБ`).
- Живой запуск на панели только после явного «да» владельца на этот запуск. Визуальная проверка без живого запуска — через `--render`.
- Публикует только владелец. Коммит в конце каждой задачи. Сообщения коммитов на английском, в стиле Conventional Commits (`feat(strip): ...`, `fix(store): ...`): заголовок до 72 знаков в повелительном наклонении, тело объясняет, зачем изменение и что оно даёт пользователю, без воды. Автор коммита — владелец (его `git config`), последняя строка `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`: соавторы оба (решение владельца 13.11.2026).

## Review Focus

1. **Перезапуск Explorer** (или его падение): полоса возвращается на место не позже чем через 2 с, без второго экземпляра окна. Проверка — шаг живой проверки в Task 9.
2. **Папки status-band нет, JSON битый или полузаписан:** программа не падает. Счётчики показывают бледные нули, лимит показывает «–», при битом файле держится прошлое значение. Тесты в Task 1.
3. **Двузначные числа и падежи:** 12 ждущих, 21 сессия, 11 сессий. Ширина элементов растёт, ничего не обрезается, формы слов верные. Тесты в Task 3, снимок `--render` с 12 ждущими в Task 6.
4. **Запуск, когда уже есть ждущие сессии:** ни одного уведомления, пока состояние не изменится на глазах у программы. Тест в Task 5.
5. **У сессии нет ссылки** (`link == null`, CLI-сессия): клик по ней не падает и ничего не открывает, строка в окне не выглядит кликабельной, уведомление без строки «Открыть чат». Тест в Task 2 (`open_target`), отрисовка в Task 7.

---

### Task 1: Каркас, модель и чтение папки status-band

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/model.rs`, `src/store.rs`
- Test: `tests/store.rs`, `tests/fixtures/status-band/{index.json,account.json,tray.json}`, `tests/fixtures/status-band/sessions/*.json`, `tests/fixtures/status-band/acks/*.json`

**Interfaces:**
- Produces (`model.rs`, все `#[serde(rename_all = "camelCase")]`, неизвестные поля игнорируются, отсутствующие `Option` → `None`):
  - `enum SessionState { Working, Waiting, Done, Idle }` (serde lowercase)
  - `struct SessionFile { v: u32, key: String, desktop_id: Option<String>, title: Option<String>, cwd: String, link: Option<String>, state: SessionState, ask: Option<String>, state_since: i64, doing: Option<String>, heartbeat_at: i64 }`
  - `struct IndexEntry { desktop_id: String, title: String, cwd: String, is_running: bool, last_activity_at: i64, link: Option<String> }`, `struct IndexFile { v: u32, updated_at: i64, sessions: Vec<IndexEntry> }`
  - `struct AckFile { v: u32, ack_at: i64, by: String }`
  - `struct Limit { window: String, percent: f64, resets_at: Option<i64> }`, `struct AccountFile { v: u32, updated_at: i64, limits: Vec<Limit> }`
- Produces (`store.rs`):
  - `fn safe_key(key: &str) -> String`: каждый символ вне `[A-Za-z0-9_.-]` заменяется на `_`.
  - `struct Snapshot { index: Vec<IndexEntry>, files: Vec<SessionFile>, acks: HashMap<String, AckFile>, account: Option<AccountFile> }`. Ключ `acks` — имя файла без `.json`.
  - `struct Store`: `fn new(dir: PathBuf) -> Store`, `fn default_dir() -> PathBuf` (`%USERPROFILE%\.claude\status-band`), `fn read(&mut self) -> Snapshot`, `fn write_ack(&self, key: &str, now_ms: i64) -> io::Result<()>`, `fn write_tray(&self, now_ms: i64) -> io::Result<()>`.
  - Файл с `v != 1` или неразборчивый не попадает в снимок. Если этот путь раньше читался удачно, берётся прошлое значение (кэш по пути).

- [ ] **Step 1:** `cargo init --name claudibeat`. Добавить зависимости `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` и `windows` с фичами, которые понадобятся до Task 9 (`Win32_Foundation`, `Win32_Graphics_Direct2D`, `Win32_Graphics_Direct2D_Common`, `Win32_Graphics_DirectWrite`, `Win32_Graphics_Dxgi_Common`, `Win32_Graphics_Gdi`, `Win32_Graphics_Imaging`, `Win32_Graphics_Dwm`, `Win32_System_Com`, `Win32_System_Threading`, `Win32_System_Diagnostics_ToolHelp`, `Win32_System_ProcessStatus`, `Win32_System_SystemInformation`, `Win32_System_Registry`, `Win32_System_Time`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Shell`, `Win32_UI_HiDpi`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_Media_Audio`, `Wdk_System_SystemInformation`). Скопировать в фикстуры по 3–4 реальных файла из `%USERPROFILE%\.claude\status-band\` с обезличенными заголовками, плюс файл `sessions/torn.json` с содержимым `{"v":1,"key":` и `acks/zz.json` с `"v":2`.

- [ ] **Step 2: Тесты `tests/store.rs`**

```rust
#[test] fn safe_key_replaces_unsafe() { assert_eq!(safe_key("local_ab:c/d"), "local_ab_c_d"); }
#[test] fn reads_fixture() { let s = Store::new(fixture()).read(); assert_eq!(s.index.len(), 3); assert!(s.files.iter().all(|f| f.key != "")); assert!(s.account.is_some()); }
#[test] fn skips_torn_and_wrong_version() { let s = Store::new(fixture()).read(); assert!(!s.acks.contains_key("zz")); /* torn.json не в files */ }
#[test] fn missing_dir_is_empty_not_error() { let s = Store::new("Z:/nope".into()).read(); assert!(s.index.is_empty() && s.files.is_empty() && s.account.is_none()); }
#[test] fn keeps_last_good_on_torn_rewrite() { /* tmp-копия фикстуры: read, испортить index.json, read снова, index.len() не изменился */ }
#[test] fn writes_ack_atomically() { /* tmp-папка: write_ack("local_1", 5) → acks/local_1.json == {"v":1,"ackAt":5,"by":"claudibeat"}, *.tmp не осталось */ }
#[test] fn writes_tray() { /* tray.json == {"v":1,"at":7} */ }
```

- [ ] **Step 3:** `cargo test --test store`, ожидаемо FAIL (нет модулей).
- [ ] **Step 4:** Реализовать `model.rs` и `store.rs` по интерфейсам. Атомарная запись: `fs::write(tmp)`, затем `MoveFileExW(tmp, dst, MOVEFILE_REPLACE_EXISTING)`. В `main.rs` пока `fn main() {}` с атрибутом подсистемы.
- [ ] **Step 5:** `cargo test --test store`, ожидаемо PASS. `cargo clippy -- -D warnings` чисто.
- [ ] **Step 6:** Коммит `Каркас, модель и чтение status-band`.

### Task 2: Сведение сессий (порт `mergeRows`)

**Files:**
- Create: `src/merge.rs`
- Test: `tests/merge.rs`

**Interfaces:**
- Consumes: `model::*` из Task 1.
- Produces:
  - `const STALE_MS: i64 = 90_000; const IDLE_KEEP_MS: i64 = 86_400_000;`
  - `struct Row { key: String, desktop_id: Option<String>, title: String, project: String, link: Option<String>, state: SessionState, ask: Option<String>, since: i64, doing: Option<String>, needs_you: bool, last_activity_at: i64 }`
  - `fn project_of(cwd: &str) -> String`: путь со `scratch-workspaces` даёт «без папки», иначе последний сегмент.
  - `fn merge_rows(index: &[IndexEntry], files: &[SessionFile], acks: &HashMap<String, AckFile>, now: i64) -> Vec<Row>`: 1:1 с `mergeRows` + `rowOf` из `shared.ts`, при этом `selfKey = null` (claudibeat не сессия), сортировка та же. Ack ищется по `safe_key(key)`, затем по `safe_key(desktop_id)`.
  - `struct Counts { waiting: usize, done: usize, working: usize, total: usize }`, `fn counts(rows: &[Row]) -> Counts`: waiting/done считаются только при `needs_you`, working — по `state == Working`, total = `rows.len()`.
  - `struct Groups<'a> { waiting: Vec<&'a Row>, done: Vec<&'a Row>, working: Vec<&'a Row> }`, `fn groups(rows: &[Row]) -> Groups`: waiting по `since` по возрастанию (дольше всех ждущие сверху), done и working по `last_activity_at` по убыванию.
  - `fn open_target(row: &Row) -> Option<&str>`: `row.link`, если ссылка начинается с `claude://`, иначе `None`.

- [ ] **Step 1: Тесты `tests/merge.rs`** (`now = 1_000_000_000`):

```rust
#[test] fn live_file_speaks() { /* файл heartbeat=now-1000, state Waiting, индекс isRunning=false → state Waiting, needs_you */ }
#[test] fn stale_file_follows_index_running() { /* heartbeat=now-STALE_MS-1, state Idle, isRunning=true → Working */ }
#[test] fn stale_waiting_keeps_word_when_app_knows_it() { /* heartbeat устарел, state Done, есть в индексе, isRunning=false → Done, since == file.state_since */ }
#[test] fn stale_loose_cli_file_goes_idle() { /* desktop_id None, нет в индексе, state Waiting, устарел → Idle → отфильтрован, если last_activity старше суток */ }
#[test] fn ack_after_since_silences() { /* ack.ack_at >= since → needs_you false; ack_at < since → true */ }
#[test] fn ack_by_desktop_id() { /* ack лежит под desktop_id, а key другой → needs_you false */ }
#[test] fn file_without_index_row_kept() { /* desktop_id Some, нет в индексе, живой → строка есть */ }
#[test] fn newest_file_per_desktop_wins() { /* два файла одного desktop_id → берётся с большим heartbeat */ }
#[test] fn idle_older_than_day_dropped() { }
#[test] fn counts_and_groups() { /* 2 waiting(ack нет), 1 done(ack есть), 3 working → Counts{waiting:2,done:0,working:3,..}; groups.waiting[0].since < groups.waiting[1].since */ }
#[test] fn open_target_only_claude_links() { /* None link → None; "claude://claude.ai/epitaxy/local_1" → Some */ }
```

- [ ] **Step 2:** `cargo test --test merge`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать `merge.rs`. Читать `shared.ts` строки 33–101 и повторять ветки `isLive`, `keepsWord`, `since` буквально.
- [ ] **Step 4:** `cargo test`, ожидаемо PASS. Сверка с полосой: временный тест `#[ignore] live_counts_print` печатает `counts` по живой папке (`cargo test -- --ignored --nocapture`). Числа должны совпасть с полосой status-band в Claude в ту же минуту, записать их в сообщение коммита.
- [ ] **Step 5:** Коммит `Сведение сессий 1:1 с status-band`.

### Task 3: Лимиты и тексты

**Files:**
- Create: `src/limits.rs`, `src/text.rs`
- Test: `tests/text.rs`

**Interfaces:**
- Consumes: `AccountFile`, `Limit` из Task 1.
- Produces:
  - `enum LimitWindow { FiveHour, Week }`, `struct LimitView { window: LimitWindow, used: u8, remaining: u8, resets_at: Option<i64> }`
  - `fn limit_views(account: Option<&AccountFile>, now: i64) -> Vec<LimitView>`: пусто, если файла нет или `updated_at` старше 6 ч. Окна `five_hour` → FiveHour, `seven_day` → Week, остальные пропускаются. `remaining = 100 - round(percent)` с ограничением 0..=100.
  - `fn strip_limit(views: &[LimitView]) -> Option<&LimitView>`: окно с минимальным `remaining`.
  - `fn plural(n: usize, one: &str, few: &str, many: &str) -> &str` (правила для русского: 1, 21 → one; 2–4, 22–24 → few; 0, 5–20, 11–14 → many).
  - `fn ago(ms: i64) -> String`: меньше 60 с «сейчас», меньше часа «N мин», меньше суток «N ч», иначе «N дн».
  - `fn reset_text(at: i64, now: i64, tz_offset_min: i32) -> String`: если тот же локальный день, то «в HH:MM», иначе «пн/вт/ср/чт/пт/сб/вс HH:MM».
  - `fn gb(bytes: u64) -> String`: «24,2» (одна цифра после запятой).
  - Подсказки: `tip_waiting(n)`: «1 сессия ждёт ответа» / «2 сессии ждут ответа». `tip_done(n)`: «1 сессия закончила, не открыта» / «2 сессии закончили, не открыты». `tip_working(n)`: «4 сессии в работе». `tip_memory(used, total)`: «Занято 24,2 из 31,8 ГБ». `tip_limit(&LimitView, now, tz)`: «Осталось 34% недельного лимита, сброс вс 18:00» / «Осталось 77% 5-часового лимита, сброс в 20:40».
  - `fn local_tz_offset_min() -> i32` (Win32 `GetTimeZoneInformation`, с учётом летнего времени). Это единственная не чистая функция модуля.

- [ ] **Step 1: Тесты `tests/text.rs`**

```rust
#[test] fn plural_forms() { for (n, w) in [(1,"сессия"),(2,"сессии"),(5,"сессий"),(11,"сессий"),(12,"сессий"),(21,"сессия"),(22,"сессии"),(0,"сессий")] { assert_eq!(plural(n,"сессия","сессии","сессий"), w); } }
#[test] fn tips() { assert_eq!(tip_waiting(1), "1 сессия ждёт ответа"); assert_eq!(tip_waiting(12), "12 сессий ждут ответа"); assert_eq!(tip_done(21), "21 сессия закончила, не открыта"); assert_eq!(tip_working(4), "4 сессии в работе"); }
#[test] fn memory_tip() { assert_eq!(tip_memory(26_000_000_000, 34_144_000_000), "Занято 24,2 из 31,8 ГБ"); } // ГБ = 2^30
#[test] fn ago_steps() { assert_eq!(ago(30_000),"сейчас"); assert_eq!(ago(7*60_000),"7 мин"); assert_eq!(ago(3*3_600_000),"3 ч"); }
#[test] fn reset_week_and_today() { /* now = 2026-11-13 18:11 МСК (UTC+180), at = 2026-11-15 18:00 → "вс 18:00"; at = сегодня 20:40 → "в 20:40" */ }
#[test] fn limits_pick_and_stale() { /* five_hour 23 и seven_day 66 → strip_limit.remaining == 34 (Week); updated_at = now-7ч → пусто */ }
```

- [ ] **Step 2:** `cargo test --test text`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать `limits.rs`, `text.rs`. День недели вычисляется из числа дней от эпохи с учётом смещения, без сторонних крейтов.
- [ ] **Step 4:** `cargo test`, ожидаемо PASS.
- [ ] **Step 5:** Коммит `Лимиты и тексты`.

### Task 4: Память по группам

**Files:**
- Create: `src/memory.rs`
- Test: `tests/memory.rs`

**Interfaces:**
- Produces:
  - `struct Proc { pid: u32, parent: u32, name: String /* в нижнем регистре, с .exe */, private_ws: u64 }`
  - `struct MemGroups { claude: u64, chrome: u64, python: u64, other: u64, used: u64, total: u64 }`, `fn percent(&self) -> u8`, `fn alert(&self) -> bool` (`percent >= 85`).
  - `fn group(procs: &[Proc], used: u64, total: u64) -> MemGroups`: claude = все `claude.exe` и все их потомки (обход в ширину по детям, множество посещённых защищает от повторного использования PID). chrome = `chrome.exe` и потомки вне claude. python = имена `python*.exe` / `pythonw*.exe` вне двух групп. other = `used` минус сумма трёх групп (`saturating_sub`).
  - `fn sample() -> MemGroups`: процессы с родителем и частным рабочим набором, `used`/`total` из `GlobalMemoryStatusEx`. API для частного набора выбирает исполнитель, например `NtQuerySystemInformation(SystemProcessInformation)`, поле `WorkingSetPrivateSize`.

- [ ] **Step 1: Тесты `tests/memory.rs`**

```rust
#[test] fn claude_tree_includes_grandchildren() { /* claude(1) → node(2) → cmd(3); chrome(4) → chrome(5); python(6, parent=99) → claude=sum(1,2,3), chrome=sum(4,5), python=6 */ }
#[test] fn python_under_claude_counts_as_claude() { }
#[test] fn pid_cycle_does_not_hang() { /* a.parent=b, b.parent=a, a=claude */ }
#[test] fn other_never_negative() { /* used меньше суммы групп → other == 0 */ }
#[test] fn alert_threshold() { /* used/total = 0.85 → alert; 0.849 → нет */ }
#[test] #[ignore] fn live_sample_sane() { let m = sample(); assert!(m.total > 0 && m.used <= m.total && m.claude <= m.used); println!("{m:?}"); }
```

- [ ] **Step 2:** `cargo test --test memory`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать `memory.rs`.
- [ ] **Step 4:** `cargo test`, ожидаемо PASS. `cargo test --test memory -- --ignored --nocapture` печатает живые группы. Сверка: сумма частного рабочего набора процесса `claude.exe` с его деревом в PowerShell (`Get-CimInstance Win32_PerfFormattedData_PerfProc_Process` → `WorkingSetPrivate`, дерево по `Win32_Process.ParentProcessId`). Расхождение не больше 10 %, цифры записать в коммит.
- [ ] **Step 5:** Коммит `Память по группам`.

### Task 5: Правила уведомлений и стопка

**Files:**
- Create: `src/notify.rs`
- Test: `tests/notify.rs`

**Interfaces:**
- Consumes: `Row` из Task 2.
- Produces:
  - `enum ToastKind { Waiting, Done, DoneBatch, MoreWaiting }`, `struct ToastEvent { kind: ToastKind, keys: Vec<String>, at: i64 }`
  - `struct Notifier`: `fn new() -> Notifier`, `fn observe(&mut self, rows: &[Row], now: i64) -> Vec<ToastEvent>`, `fn opened(&mut self, key: &str, now: i64)`.
    - Первый вызов `observe` только запоминает состояние и ничего не выдаёт.
    - Событие возникает, когда ключ становится `needs_you` или меняет `state` при `needs_you`.
    - Не чаще раза в 10 с на ключ. Ничего для ключа, открытого через `opened` в последние 5 с.
    - `Waiting` выдаётся сразу. `Done` копятся 2 с от первого: один ключ даёт `Done`, несколько — один `DoneBatch` со всеми ключами.
  - `struct ToastStack { items: Vec<ToastEvent> }` (вместимость 3, порядок — время появления), `fn push(&mut self, ev: ToastEvent, waiting_overflow: usize)`, `fn remove(&mut self, key_or_index)`. Правила:
    - если места нет, вытесняется самое старое `Done`/`DoneBatch`;
    - если все три места заняты `Waiting`, третье становится `MoreWaiting` с числом невместившихся ждущих;
    - `Done` и `DoneBatch` живут 8 с (таймер ведёт окно), `Waiting` и `MoreWaiting` висят.

- [ ] **Step 1: Тесты `tests/notify.rs`**

```rust
#[test] fn first_observe_is_silent() { /* rows с 2 waiting → observe == [] */ }
#[test] fn new_waiting_fires_immediately() { }
#[test] fn throttle_10s_per_key() { /* waiting → ack → снова waiting через 5 с → тишина; через 11 с → событие */ }
#[test] fn opened_suppresses_5s() { }
#[test] fn done_batched_within_2s() { /* 3 done за 1,5 с → после now+2с ровно один DoneBatch с 3 ключами */ }
#[test] fn single_done_after_2s() { }
#[test] fn stack_evicts_oldest_done() { /* [Done, Waiting, Waiting] + Waiting → [Waiting, Waiting, Waiting] */ }
#[test] fn stack_full_of_waiting_turns_summary() { /* 3 Waiting + Waiting → третий == MoreWaiting{count 2} */ }
```

- [ ] **Step 2:** `cargo test --test notify`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать `notify.rs`.
- [ ] **Step 4:** `cargo test`, ожидаемо PASS.
- [ ] **Step 5:** Коммит `Правила уведомлений`.

### Task 6: Рендерер, полоса в PNG и пробное встраивание в панель

Это пробный прототип из раздела «Главный технический риск» спецификации. Он идёт раньше окна сессий и уведомлений: если прозрачность на панели не заработает, план меняется до того, как написан остальной интерфейс.

**Files:**
- Create: `src/render/mod.rs`, `src/render/icons.rs`, `src/render/strip.rs`, `src/cli.rs`, `src/win/mod.rs`, `src/win/layered.rs`, `src/win/taskbar.rs`, `examples/spike.rs`
- Modify: `src/main.rs` (разбор `--render <dir>`)

**Interfaces:**
- Produces:
  - `struct Rect { x: f32, y: f32, w: f32, h: f32 }` с методом `contains(x, y) -> bool`. `struct Color(u32 /* 0xAARRGGBB */)` и константы токенов из Global Constraints (`INK`, `MUTED`, `TIME`, `PILL`, `PILL_INK`, `HOVER`, `TRACK`, `SURFACE`, `HAIR`).
  - `struct Frame { width: u32, height: u32, pixels: Vec<u32> /* BGRA, премультиплицировано */ }`
  - `struct Renderer`: `fn new() -> windows::core::Result<Renderer>` (фабрики D2D, DWrite, WIC). `fn render(&self, w_dip: f32, h_dip: f32, scale: f32, bg: Option<Color>, paint: impl FnOnce(&Painter)) -> Result<Frame>`: WIC-битмап `GUID_WICPixelFormat32bppPBGRA`, `CreateWicBitmapRenderTarget`, grayscale-сглаживание текста.
  - `Painter`: `fill_round(rect, radius, color)`, `text(s, x, y, &TextStyle) -> f32 /* ширина */`, `measure(s, &TextStyle) -> (f32, f32)`, `icon(Icon, cx, cy, size, color)`, `ring(cx, cy, r, stroke, frac, color, track)`, `bar(rect, frac, color, track)`, `line(..)`.
  - `enum Icon { Wait, Done, Star, Mem, MemFull, Close }`: геометрия 1:1 с `<symbol>` в `docs/mockups/v2/strips-v2.html` (viewBox 14).
  - `fn save_png(frame: &Frame, path: &Path) -> Result<()>` через WIC.
  - `enum StripItem { Waiting, Done, Working, Memory, Limit }`, `struct StripModel { waiting: usize, done: usize, working: usize, mem_percent: u8, mem_alert: bool, limit: Option<(u8 /* remaining */, f32)> }`
  - `struct StripLayout { width: f32, items: Vec<(StripItem, Rect)> }`, `fn layout_strip(m: &StripModel, p: &Painter) -> StripLayout`, `fn paint_strip(p: &Painter, l: &StripLayout, m: &StripModel, hover: Option<StripItem>)`.
  - Размеры из `.c2`:
    - элемент высотой 32, поля по 8 слева и справа, зазор между значком и числом 6, скругление 6, шрифт 13/600;
    - разделитель 1×22, по 6 с каждой стороны, после «в работе»;
    - значки: Wait 12, Done 13, Star 13, Mem 14, кольцо 14 (r 5.5, stroke 2);
    - полоса начинается в 8 DIP от левого края окна, общая высота окна 48.
  - Плашка: элемент Waiting при `waiting > 0` залит `#1d1d1d`, значок и число `#f6f6f6`. Нули (waiting, done, working) рисуются с непрозрачностью 0.42. `mem_alert` меняет `Mem` на `MemFull`. Лимит `None` даёт бледное кольцо и «–».
  - `cli::render_samples(dir: &Path) -> Result<()>`: пишет `strip-wait.png` (1/2/4, 76%, 34%), `strip-calm.png` (0/0/2, 59%), `strip-12.png` (12/3/4, 91% alert), `strip-hover.png` (наведение на Working), `strip-nolimit.png`. Масштаб 2, подложка — `docs/mockups/taskbar-primary.png`, обрезанный до 960×48, чтобы сравнивать с макетом.
  - `layered::present(hwnd: HWND, frame: &Frame, pos: Option<POINT>)`:
    - `UpdateLayeredWindow(ULW_ALPHA)` с DC экрана;
    - пиксели с альфой 0 заменяются на `0x01000000`, чтобы вся полоса ловила мышь (как `render_custom_window` у CodeZeno, `src/window/positioning.rs:160`).
  - `taskbar::primary() -> Option<HWND>` (`Shell_TrayWnd`).
  - `taskbar::embed(hwnd, taskbar)`:
    - стиль `WS_CHILD | WS_CLIPSIBLINGS`, расширенный `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, `SetParent`;
    - после успешного `SetParent` один раз снять и вернуть `WS_EX_LAYERED` (как `embed_as_child` у CodeZeno, `src/native_interop.rs:314`);
    - `SetWindowPos(HWND_TOP)`.
  - `taskbar::place_left(hwnd, taskbar, width_px)`: x = 0, y = 0, высота = высота панели.
  - `taskbar::created_msg() -> u32` (`RegisterWindowMessageW("TaskbarCreated")`).
  - `examples/spike.rs`: окно, встроенное в панель, показывает `strip-wait` с живыми цифрами из `Store` + `merge` раз в 2 с, переживает `TaskbarCreated`, закрывается по правому клику. Это одноразовый код, в `app.rs` переносятся только модули `win/`.

- [ ] **Step 1:** Тест `tests/render.rs`: `layout_strip` для `StripModel{waiting:12,..}` даёт элемент Waiting шире, чем для `waiting:1`, и элементы не перекрываются. Рендер `Frame` 100×48 с прозрачным фоном содержит пиксели с альфой 0 и пиксели текста с альфой больше 0.
- [ ] **Step 2:** `cargo test --test render`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать рендерер, значки, полосу и `--render`.
- [ ] **Step 4:** `cargo test` PASS. `cargo run --release -- --render target/shots`. Главный агент открывает каждый PNG рядом с `docs/mockups/v2/strips-v2.png`, пишет критику (выравнивание по вертикали, вес шрифта, толщина линий значков, отступы, вид плашки) и возвращает исполнителю до совпадения.
- [ ] **Step 5:** Реализовать `win/layered.rs`, `win/taskbar.rs`, `examples/spike.rs`. `cargo build --release --example spike` чисто.
- [ ] **Step 6: Пробный запуск на живой панели — только после «да» владельца.** Запустить `target\release\examples\spike.exe`, снять нижние 48 px основного монитора (`CopyFromScreen`, без фокуса окон). Проверка:
  - текст без цветной окантовки;
  - фон прозрачный, видна Mica панели;
  - полоса не перекрывает кнопку «Пуск».

  После отдельного «да» владельца: `Stop-Process -Name explorer`, полоса вернулась. Если прозрачность не работает, перейти на запасной вариант из спецификации (непрозрачный фон по цвету пикселей панели) и сообщить владельцу до продолжения.
- [ ] **Step 7:** Коммит `Рендерер, полоса и встраивание в панель`.

### Task 7: Окно сессий и уведомления в PNG

**Files:**
- Create: `src/render/flyout.rs`, `src/render/toast.rs`, `src/render/tooltip.rs`
- Modify: `src/cli.rs`

**Interfaces:**
- Consumes: `Painter`, `Icon` (Task 6); `Row`, `Groups`, `Counts` (Task 2); `MemGroups` (Task 4); `LimitView` (Task 3); `ToastEvent` (Task 5); тексты (Task 3).
- Produces:
  - `struct FlyoutModel<'a> { groups: Groups<'a>, total: usize, expanded: [bool; 3], mem: MemGroups, limits: Vec<LimitView>, now: i64, tz: i32 }`
  - `struct FlyoutLayout { height: f32, rows: Vec<(String /* key */, Rect)>, more: Vec<(usize /* группа */, Rect)> }`, `fn layout_flyout(m, p, max_h: f32) -> FlyoutLayout`, `fn paint_flyout(p, l, m, hover_row: Option<&str>, scroll: f32)`
  - Размеры и тексты из `.fly`, правила из дизайна:
    - ширина 384;
    - не больше 3 строк на группу, пока группа не раскрыта, затем строка «и ещё N»;
    - подписи строк: «ждут» — `«ask»` в ёлочках, «готово» — `project`, «в работе» — `doing` с заглавной буквы;
    - время `ago(now - since)`;
    - строка без `open_target` рисуется без подсветки при наведении.
  - Если `height` больше `max_h`, прокручивается только блок сессий: `scroll` ограничен, память и лимиты прижаты к низу.
  - `struct ToastModel { kind: ToastKind, title: String, sub: Option<String>, can_open: bool, count: usize }`, `fn paint_toast(p, m, hover_close: bool) -> f32 /* высота */`. Размеры из `.toast`, ширина 340. Метки: «Ждёт ответа», «Готово», для `DoneBatch` «Готово» и заголовок «3 сессии закончили», для `MoreWaiting` «Ждут ответа» и «и ещё 2 ждут ответа». «Открыть чат» рисуется только при `can_open`.
  - `fn paint_tooltip(p, text) -> (f32, f32)` из `.tip`.
  - `--render` дополнительно пишет `flyout.png`, `flyout-expanded.png`, `flyout-scroll.png` (12 ждущих, высота 600), `toast-waiting.png`, `toast-done.png`, `toast-batch.png`, `toast-nolink.png`, `tooltip.png`.

- [ ] **Step 1:** Тест `tests/flyout.rs`: 5 ждущих, группа не раскрыта → в `rows` 3 ждущих и есть `more` для группы 0. Группа раскрыта → 5 строк. `max_h` маленький → высота окна равна `max_h`.
- [ ] **Step 2:** `cargo test --test flyout`, ожидаемо FAIL.
- [ ] **Step 3:** Реализовать.
- [ ] **Step 4:** `cargo test` PASS. `--render`. Главный агент сверяет `flyout.png` с `docs/mockups/v1/flyout.png`, а уведомления с `docs/mockups/v1/toast.png`, пишет критику и возвращает исполнителю до совпадения.
- [ ] **Step 5:** Коммит `Окно сессий и уведомления`.

### Task 8: Окна Win32 и действия

**Files:**
- Create: `src/win/strip_window.rs`, `src/win/popup.rs`, `src/win/shell.rs`, `src/sound.rs`, `src/settings.rs`

**Interfaces:**
- Consumes: всё отрисовываемое из Task 6–7, `layered::present` и `taskbar::*` из Task 6.
- Produces:
  - `strip_window::StripWindow`:
    - наведение через `TrackMouseEvent`;
    - подсказка через 600 мс в окне `popup`;
    - левый клик отдаёт `Action`, правый — меню `TrackPopupMenu`.
  - `enum Action { OpenChat(String /* key */), OpenFlyout, ToggleFlyout, MarkAllSeen, ToggleSound, ToggleAutostart, Quit }`
  - `popup::Popup`:
    - верхнее окно `WS_POPUP` с `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`;
    - тень и скругление рисуются в кадре (как в макете), а не через DWM: у слоистого окна DWM-тени нет;
    - обслуживает окно сессий (Esc, клик мимо через `WM_ACTIVATEAPP`/хук мыши, колесо прокрутки), уведомления (таймер 8 с с паузой при наведении, крестик) и подсказку.
  - `shell::open_url(url: &str) -> bool` (`ShellExecuteW`). `shell::autostart_get() -> bool`, `shell::autostart_set(on: bool)`: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, значение `claudibeat` = `"<путь exe>"`.
  - `settings::Settings { sound: bool }`: файл `%APPDATA%\claudibeat\settings.json`, по умолчанию `sound: true`.
  - `sound::play(kind: ToastKind)`: WAV собирается в памяти при старте, `PlaySoundW(SND_MEMORY | SND_ASYNC)`. Waiting — два мягких тона 660→880 Гц по 90 мс, Done — один тон 880 Гц 140 мс. Синус с экспоненциальным затуханием, громкость −18 dBFS.
  - Тексты меню: «Отметить всё просмотренным», «Звук уведомлений», «Запускать вместе с Windows», «Выход». У переключателей галочки.

- [ ] **Step 1:** Тест `tests/settings.rs`: чтение отсутствующего файла даёт `sound: true`; запись, затем чтение — round-trip. Тест WAV-генератора: заголовок `RIFF…WAVE`, длина данных соответствует 44100 Гц × длительность.
- [ ] **Step 2:** `cargo test`, ожидаемо FAIL, затем реализовать модули и снова `cargo test` PASS. Живой запуск в этой задаче не нужен, сборка `cargo build --release` должна пройти.
- [ ] **Step 3:** Коммит `Окна Win32, меню, звук, автозапуск`.

### Task 9: Сборка приложения и живая проверка

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`, `README.md`

**Interfaces:**
- Consumes: всё выше.
- Produces:
  - `main` в таком порядке: DPI, мьютекс `Local\claudibeat` (второй экземпляр молча выходит), разбор `--render <dir>` / `--dump <file>`, затем `app::run()`.
  - `app::run()`:
    - Рендерер, `Store`, `Notifier`, `ToastStack`, окна; один поток с циклом сообщений.
    - `SetTimer` раз в 2 с: перечитать папку, пересобрать модель, при изменении перерисовать. Плюс `FindFirstChangeNotificationW` на папку status-band для мгновенной реакции.
    - Память опрашивается раз в 3 с. `write_tray` раз в 10 с.
    - На `TaskbarCreated` выполнить `embed` и `place_left` заново.
  - Действия:
    - `OpenChat(key)`: `open_url`, `write_ack`, `notifier.opened`, закрыть соответствующее уведомление.
    - Клик по «ждёт» или «готово» при ровно одной сессии — `OpenChat`, иначе `ToggleFlyout`.
    - `MarkAllSeen`: `write_ack` для всех `needs_you`.
  - Полноэкранный режим: перед показом уведомления проверить `SHQueryUserNotificationState`. При `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN` и `QUNS_PRESENTATION_MODE` уведомление ждёт без звука, пока состояние не сменится. Полоса при автоскрытии и полноэкранном режиме прячется вместе с панелью сама, потому что она её потомок.
  - `--dump <file>` пишет JSON `{counts, mem, limits}` (для проверки без окон).
  - README: сборка (`cargo build --release`), запуск, `--render`, где лежат настройки, как выйти.

- [ ] **Step 1:** `cargo build --release`, `cargo test`, `cargo clippy -- -D warnings`: всё чисто. `target\release\claudibeat.exe --dump target\dump.json`: числа совпадают с полосой status-band в Claude и с живыми группами памяти из Task 4.
- [ ] **Step 2: Живая проверка — только после «да» владельца.** Запустить `target\release\claudibeat.exe`. Снять нижние 48 px основного монитора (PowerShell `CopyFromScreen`, без фокуса окон) и сравнить с `docs/mockups/v2/strips-v2.png`. Проверить по списку:
  - полоса стоит слева и не перекрывает кнопки;
  - наведение и подсказка работают;
  - клик по «готово» с одной сессией открывает чат, и счётчик падает в течение 2 с;
  - окно сессий открывается и закрывается по Esc;
  - правое меню работает.
- [ ] **Step 3: Перезапуск Explorer — отдельное «да» владельца.** `Stop-Process -Name explorer` (Explorer поднимется сам): полоса вернулась не позже чем через 2 с, процесс claudibeat один. Если Explorer не поднялся сам, `Start-Process explorer.exe`.
- [ ] **Step 4:** Уведомление: попросить владельца дождаться окончания любой сессии или проверить на тестовой сессии. Уведомление появилось слева, звук прозвучал один раз, клик открыл чат.
- [ ] **Step 5:** Два прогона полировки (`impeccable audit` → правки → снова снимки), каждый со снимками и письменной критикой. Коммит `feat: ship claudibeat v1`.

### Task 10: README и подготовка к открытому коду

Делается после Task 9 и финального ревью. Публикует владелец сам, после полной выкатки и проверки.

**Files:**
- Modify: `README.md`, `Cargo.toml` (`authors`, `description`, `license`, `repository`)
- Create: `LICENSE` (MIT), `NOTICE` (атрибуция кода встраивания CodeZeno Claude-Code-Usage-Monitor, MIT), `docs/media/*.png|gif`

- [ ] **Step 1:** README на английском (плюс короткий раздел на русском):
  - бейджи: Windows 11, Rust, лицензия MIT, размер exe, «no network»;
  - главный скриншот полосы на настоящей панели и GIF «уведомление, клик, открылся чат»;
  - что показывает, установка, сборка, настройки, как это работает (контракт status-band);
  - измеренные метрики: размер exe, RAM в простое, CPU в простое, задержка от события до уведомления;
  - FAQ, дисклеймер «not affiliated with Anthropic».
  - Раздел Authors: Dmitry Goldobin ([@PixelShino](https://github.com/PixelShino)) и Claude (Anthropic) как соавторы. Те же два автора в `Cargo.toml` `authors` (без адресов почты).
- [ ] **Step 2:** Метрики измерить, а не придумать:
  - память: `Get-Process claudibeat` (частный рабочий набор) за 10 минут простоя;
  - CPU: через `Get-Counter`;
  - размер: файл `target\release\claudibeat.exe`.
- [ ] **Step 3:** Проверка приватности перед публикацией. В `docs/` и истории есть настоящие названия сессий, пути `C:\Users\<имя>`, имена проектов владельца. Заменить снимки на снимки с демо-данными. Владельцу предложить публиковать с новой историей (squash): переписывание истории только с его согласия.
- [ ] **Step 4:** Коммит `docs: polish README for public release`.
