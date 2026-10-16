use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::desktop::{self, Entry, Found};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    System,
}

/// A program the desktop session starts at login, from an XDG autostart entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Startup {
    pub id: String,
    pub name: String,
    pub command: Option<String>,
    pub comment: Option<String>,
    pub enabled: bool,
    pub scope: Scope,
    pub path: PathBuf,
}

/// Autostart entries with the user's own overriding system ones of the same name.
#[must_use]
pub fn read() -> Vec<Startup> {
    let user = desktop::config_dirs().into_iter().next();
    let mut seen = HashSet::new();
    let mut entries: Vec<Startup> = desktop::config_dirs()
        .iter()
        .flat_map(|dir| desktop::scan(&dir.join("autostart")))
        .filter(|found| seen.insert(found.id.clone()))
        .map(|found| {
            let scope = if user
                .as_ref()
                .is_some_and(|user| found.path.starts_with(user))
            {
                Scope::User
            } else {
                Scope::System
            };
            startup(found, scope)
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

fn startup(found: Found, scope: Scope) -> Startup {
    let entry = &found.entry;
    Startup {
        name: entry
            .get("Name")
            .map_or_else(|| found.id.clone(), str::to_owned),
        command: entry.get("Exec").map(str::to_owned),
        comment: entry.get("Comment").map(str::to_owned),
        enabled: enabled(entry),
        scope,
        path: found.path,
        id: found.id,
    }
}

/// `Hidden=true` removes an entry, which is how a user turns off a system one; the GNOME key does the same.
fn enabled(entry: &Entry) -> bool {
    !entry.flag("Hidden") && entry.get("X-GNOME-Autostart-enabled") != Some("false")
}

/// Turns an entry on or off by writing the user's own copy of it, as the specification describes.
///
/// # Errors
///
/// Fails when the user's autostart directory cannot be written, which is the case when the
/// entry is managed declaratively and the file is a read-only link into a store.
pub fn set_enabled(startup: &Startup, enabled: bool) -> io::Result<()> {
    let directory = desktop::config_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
        .join("autostart");
    fs::create_dir_all(&directory)?;
    let text = fs::read_to_string(&startup.path)?;
    fs::write(directory.join(&startup.id), with_hidden(&text, !enabled))
}

/// The file with its `Hidden` key set, replacing any existing one in the main group.
fn with_hidden(text: &str, hidden: bool) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("Hidden="))
        .collect();
    let key = format!("Hidden={hidden}");
    match lines
        .iter()
        .position(|line| line.trim() == "[Desktop Entry]")
    {
        Some(group) => lines.insert(group + 1, &key),
        None => return format!("[Desktop Entry]\n{key}\n"),
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_or_gnome_disabled_entries_are_off() {
        assert!(enabled(&Entry::parse("[Desktop Entry]\nName=x\n")));
        assert!(!enabled(&Entry::parse("[Desktop Entry]\nHidden=true\n")));
        assert!(!enabled(&Entry::parse(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n"
        )));
    }

    #[test]
    fn hiding_replaces_an_existing_key_in_the_main_group() {
        let text = "[Desktop Entry]\nName=x\nHidden=false\n[Desktop Action a]\nName=a\n";
        let hidden = with_hidden(text, true);
        assert_eq!(
            hidden,
            "[Desktop Entry]\nHidden=true\nName=x\n[Desktop Action a]\nName=a\n"
        );
        assert!(!enabled(&Entry::parse(&hidden)));
        assert!(enabled(&Entry::parse(&with_hidden(&hidden, false))));
    }

    #[test]
    fn a_file_without_a_group_becomes_a_minimal_entry() {
        assert_eq!(
            with_hidden("garbage", true),
            "[Desktop Entry]\nHidden=true\n"
        );
    }

    #[test]
    fn this_session_reads_without_failing() {
        let entries = read();
        let ids: HashSet<_> = entries.iter().map(|entry| &entry.id).collect();
        assert_eq!(ids.len(), entries.len());
    }
}
