# status-band

Невидимый поставщик данных для claudibeat (с 17.11.2026): мод ничего не рисует в окне Claude — ни полосы, ни нижней строки, ни строки под полем, не показывает тосты, не играет звук и не ставит точку в сайдбаре. Всё это показывает claudibeat.

Что мод делает в каждой сессии:

- пишет её состояние в `~/.claude/status-band/sessions/`; лидер пишет `index.json`, `board.json`, `tasks.json`, `assignments.json`, любой — `account.json`;
- выполняет заявки доски claudibeat из `requests/` (назначить задачу Plane в чат или в новый чат, сменить статус, обновить доску) и отвечает в `replies/`.

Модель получает инструмент `mcp__status-band__backlog` (`list / show / add / take / move / comment`): на «чекни задачи» она сама смотрит задачник.

## Как подключён

Папка мода указана в `CLAUDE_CODE_PLUGIN_DIRS` (`~/.claude/settings.json` → `env`), мод грузится в каждой новой сессии. Сессии обмениваются состоянием через `~/.claude/status-band/` — формат в [docs/DESIGN.md](docs/DESIGN.md).

## Настройки

| Поле | По умолчанию |
|---|---|
| `planeUrl` | пусто — `base` из `~/.secrets/plane.json` (тот же файл могут читать и другие инструменты) |
| `planeWorkspace` | пусто — `workspace` оттуда же |
| `planeApiKey` | пусто — `key` оттуда же, затем `PLANE_API_KEY` сервера `plane` из `~/.claude.json` |
| `inboxProject` | `INBOX` — куда падают задачи без `#метки` |

## Проверка

```bash
claude plugin validate ~/.claude/mods/status-band
claude plugin test ~/.claude/mods/status-band
```
