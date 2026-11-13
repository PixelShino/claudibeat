# claudibeat

Пульс сессий Claude Desktop и памяти компьютера в левом углу панели задач Windows 11.

Что показывает: сколько сессий ждут ответа, сколько закончили и ещё не открыты, сколько в работе; сколько памяти занято и сколько из неё съедает Claude со своими MCP-серверами; сколько осталось лимита. Когда сессия закончила или ждёт ответа, всплывает уведомление, клик открывает этот чат.

Статус: первая версия, идёт живая проверка. Дизайн: [docs/superpowers/specs/2026-11-13-claudibeat-design.md](docs/superpowers/specs/2026-11-13-claudibeat-design.md), макеты: [docs/mockups](docs/mockups).

Данные о сессиях берутся из папки мода status-band (`%USERPROFILE%\.claude\status-band\`). claudibeat не ходит в сеть и не читает токены Claude.

## Сборка и запуск

Нужен Rust с набором `stable-x86_64-pc-windows-gnu`.

```powershell
cargo build --release
.	argetelease\claudibeat.exe
```

Полоса встаёт в левый край панели задач основного монитора. Второй экземпляр не запускается.

- Правый клик по полосе: «Отметить всё просмотренным», «Звук уведомлений», «Запускать вместе с Windows», «Выход».
- Настройки: `%APPDATA%\claudibeat\settings.json`. Автозапуск: значение `claudibeat` в `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, по умолчанию выключен.
- `claudibeat.exe --render <папка> [--bg <png>]` рисует полосу, окно сессий и уведомления в PNG на выдуманных данных, без окон.
- `claudibeat.exe --dump <файл>` пишет в JSON текущие счётчики, память и лимиты.

