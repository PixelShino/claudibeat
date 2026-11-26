//! `claudibeat --setup`: everything a new user needs in one step. Puts the
//! status-band mod (built into the exe) where Claude loads it, adds it to
//! `CLAUDE_CODE_PLUGIN_DIRS`, turns on starting with Windows and starts the
//! strip. Run again after an update, it refreshes the mod it installed.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The mod's files, as they are in `mod/status-band` when the exe is built.
const MOD_FILES: &[(&str, &str)] = &[
    (".claude-plugin/plugin.json", include_str!("../mod/status-band/.claude-plugin/plugin.json")),
    ("hooks/hooks.json", include_str!("../mod/status-band/hooks/hooks.json")),
    ("hooks/register.tsx", include_str!("../mod/status-band/hooks/register.tsx")),
    ("hooks/shared.ts", include_str!("../mod/status-band/hooks/shared.ts")),
    ("hooks/plane.ts", include_str!("../mod/status-band/hooks/plane.ts")),
    ("hooks/board-requests.ts", include_str!("../mod/status-band/hooks/board-requests.ts")),
    ("types/index.d.ts", include_str!("../mod/status-band/types/index.d.ts")),
    ("tsconfig.json", include_str!("../mod/status-band/tsconfig.json")),
    ("README.md", include_str!("../mod/status-band/README.md")),
];

/// Left in a mod folder claudibeat wrote, so a later setup may replace it.
/// A folder without it is someone's own copy and stays as it is.
const MARK: &str = ".installed-by-claudibeat";
const PLUGIN_DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";

#[derive(Debug, PartialEq, Eq)]
pub enum ModResult {
    Installed,
    Updated,
    /// A copy that claudibeat did not write is there; it is left alone.
    KeptOwn,
}

/// Write the mod into `dir` unless someone's own copy lives there.
pub fn install_mod(dir: &Path) -> std::io::Result<ModResult> {
    let existed = dir.join(".claude-plugin").join("plugin.json").is_file();
    if existed && !dir.join(MARK).is_file() {
        return Ok(ModResult::KeptOwn);
    }
    for (rel, text) in MOD_FILES {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    std::fs::write(dir.join(MARK), env!("CARGO_PKG_VERSION"))?;
    Ok(if existed { ModResult::Updated } else { ModResult::Installed })
}

/// Add `dir` to `env.CLAUDE_CODE_PLUGIN_DIRS` in Claude's `settings.json`,
/// keeping everything else in the file. Returns whether the file changed.
pub fn add_plugin_dir(settings: &Path, dir: &Path) -> std::io::Result<bool> {
    let text = match std::fs::read_to_string(settings) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_owned(),
        Err(e) => return Err(e),
    };
    let mut root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(std::io::Error::other)?;
    let Some(obj) = root.as_object_mut() else { return Err(std::io::Error::other("settings.json is not an object")) };
    let env = obj.entry("env").or_insert_with(|| Value::Object(Map::new()));
    let Some(env) = env.as_object_mut() else { return Err(std::io::Error::other("settings.json: env is not an object")) };
    let want = dir.to_string_lossy().replace('\\', "/");
    let now = env.get(PLUGIN_DIRS).and_then(Value::as_str).unwrap_or("").to_owned();
    let same = |p: &str| p.trim().replace('\\', "/").trim_end_matches('/').eq_ignore_ascii_case(want.trim_end_matches('/'));
    if now.split(';').any(same) {
        return Ok(false);
    }
    let joined = if now.trim().is_empty() { want } else { format!("{};{want}", now.trim_end_matches(';')) };
    env.insert(PLUGIN_DIRS.to_owned(), Value::String(joined));
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let out = serde_json::to_string_pretty(&root).map_err(std::io::Error::other)?;
    std::fs::write(settings, out + "\n")?;
    Ok(true)
}

fn claude_dir() -> PathBuf {
    crate::win::shell::profile_dir().join(".claude")
}

/// The whole setup; the lines it returns say what happened, one step a line.
pub fn run() -> Vec<String> {
    let claude = claude_dir();
    let dir = claude.join("mods").join("status-band");
    let mut said = Vec::new();
    match install_mod(&dir) {
        Ok(ModResult::Installed) => said.push(format!("mod installed: {}", dir.display())),
        Ok(ModResult::Updated) => said.push(format!("mod updated: {}", dir.display())),
        Ok(ModResult::KeptOwn) => said.push(format!("mod: your own copy in {} is left as it is", dir.display())),
        Err(e) => said.push(format!("mod: could not write {}: {e}", dir.display())),
    }
    match add_plugin_dir(&claude.join("settings.json"), &dir) {
        Ok(true) => said.push(format!("Claude settings: {PLUGIN_DIRS} now includes the mod; open a new chat to load it")),
        Ok(false) => said.push("Claude settings: the mod was already there".to_owned()),
        Err(e) => said.push(format!("Claude settings: not changed ({e}); add the mod folder to {PLUGIN_DIRS} by hand")),
    }
    crate::win::shell::autostart_set(true);
    said.push(if crate::win::shell::autostart_get() { "starts with Windows: on".to_owned() } else { "starts with Windows: could not turn on".to_owned() });
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("claudibeat-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_mod_goes_in_and_an_own_copy_stays() {
        let d = scratch("mod");
        let m = d.join("status-band");
        assert_eq!(install_mod(&m).unwrap(), ModResult::Installed);
        assert!(m.join("hooks/register.tsx").is_file());
        assert_eq!(install_mod(&m).unwrap(), ModResult::Updated);
        std::fs::remove_file(m.join(MARK)).unwrap();
        std::fs::write(m.join("hooks/register.tsx"), "mine").unwrap();
        assert_eq!(install_mod(&m).unwrap(), ModResult::KeptOwn);
        assert_eq!(std::fs::read_to_string(m.join("hooks/register.tsx")).unwrap(), "mine");
    }

    #[test]
    fn the_plugin_dir_is_added_once_and_the_rest_is_kept() {
        let d = scratch("settings");
        let s = d.join("settings.json");
        std::fs::write(&s, r#"{"model":"x","env":{"A":"1","CLAUDE_CODE_PLUGIN_DIRS":"C:/m/one"}}"#).unwrap();
        let dir = Path::new(r"C:\m\status-band");
        assert!(add_plugin_dir(&s, dir).unwrap());
        assert!(!add_plugin_dir(&s, dir).unwrap(), "a second run changes nothing");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["model"], "x");
        assert_eq!(v["env"]["A"], "1");
        assert_eq!(v["env"][PLUGIN_DIRS], "C:/m/one;C:/m/status-band");
        let fresh = d.join("new.json");
        assert!(add_plugin_dir(&fresh, dir).unwrap());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&fresh).unwrap()).unwrap();
        assert_eq!(v["env"][PLUGIN_DIRS], "C:/m/status-band");
    }
}
