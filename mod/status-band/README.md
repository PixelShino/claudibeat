# status-band

The data source for claudibeat. A Claude Code plugin that runs inside every Claude Desktop chat and writes what claudibeat shows. It draws nothing in Claude itself.

What it does in each chat:

- writes the chat's state (working, waiting for you, done, the question it asked) to `~/.claude/status-band/sessions/`;
- one chat, the leader, lists all chats into `index.json` and keeps `account.json` with your plan limits;
- with Plane configured: keeps `board.json` and carries out the board's requests from claudibeat (give a task to a chat, start a new chat, change a task's state).

The file formats are in [docs/DESIGN.md](docs/DESIGN.md) (Russian); claudibeat's README has the short version.

## Install

The claudibeat installer does this for you. By hand:

1. Copy this folder to `%USERPROFILE%\.claude\mods\status-band`.
2. Add that folder to `CLAUDE_CODE_PLUGIN_DIRS` in `%USERPROFILE%\.claude\settings.json`; several folders are separated by `;`:

   ```json
   { "env": { "CLAUDE_CODE_PLUGIN_DIRS": "C:/Users/you/.claude/mods/status-band" } }
   ```

3. Open a new chat in Claude Desktop. Chats that were already open keep running without the mod.

## Plane (optional)

Without Plane the board stays off and everything else works. To turn it on, fill the plugin settings `planeUrl`, `planeWorkspace` and `planeApiKey`, or put `{"base": "...", "workspace": "...", "key": "..."}` into `%USERPROFILE%\.secrets\plane.json`. The key is never written to the shared folder, logs or replies.

## Check

```powershell
claude plugin validate $HOME\.claude\mods\status-band
claude plugin test $HOME\.claude\mods\status-band
```
