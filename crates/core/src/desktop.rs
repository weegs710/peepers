use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The `[Desktop Entry]` group of a `.desktop` file: its unlocalized keys and values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry(HashMap<String, String>);

impl Entry {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut in_entry = false;
        Self(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.starts_with('#'))
                .filter_map(|line| {
                    if line.starts_with('[') {
                        in_entry = line == "[Desktop Entry]";
                        return None;
                    }
                    let (key, value) = line.split_once('=')?;
                    (in_entry && !key.contains('['))
                        .then(|| (key.trim().to_owned(), value.trim().to_owned()))
                })
                .collect(),
        )
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// `true` only for the literal value `true`, as the specification defines booleans.
    #[must_use]
    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == Some("true")
    }

    /// Semicolon-separated list values such as `Categories`.
    #[must_use]
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .map(|value| {
                value
                    .split(';')
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// An entry found on disk, with the ID the specification derives from its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub path: PathBuf,
    pub entry: Entry,
}

/// Every `.desktop` file below `root`, with `/` in the relative path written as `-` in the ID.
#[must_use]
pub fn scan(root: &Path) -> Vec<Found> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "desktop")
                && let Ok(text) = fs::read_to_string(&path)
                && let Ok(relative) = path.strip_prefix(root)
            {
                found.push(Found {
                    id: relative.to_string_lossy().replace('/', "-"),
                    path,
                    entry: Entry::parse(&text),
                });
            }
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// The base directories for data files, most specific first.
#[must_use]
pub fn data_dirs() -> Vec<PathBuf> {
    let home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")));
    let system =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    home.into_iter()
        .chain(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .collect()
}

/// The base directories for configuration, most specific first.
#[must_use]
pub fn config_dirs() -> Vec<PathBuf> {
    let home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".config")));
    let system = env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_owned());
    home.into_iter()
        .chain(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# comment\n[Desktop Entry]\nName=Files\nName[de]=Dateien\nExec=files %U\nCategories=System;FileManager;\nNoDisplay=true\n\n[Desktop Action new]\nName=New window\n";

    #[test]
    fn only_the_main_group_and_unlocalized_keys_are_kept() {
        let entry = Entry::parse(FILE);
        assert_eq!(entry.get("Name"), Some("Files"));
        assert_eq!(entry.get("Exec"), Some("files %U"));
        assert_eq!(entry.get("Name[de]"), None);
    }

    #[test]
    fn flags_and_lists_follow_the_specification() {
        let entry = Entry::parse(FILE);
        assert!(entry.flag("NoDisplay") && !entry.flag("Hidden"));
        assert_eq!(entry.list("Categories"), ["System", "FileManager"]);
        assert!(entry.list("Keywords").is_empty());
    }

    #[test]
    fn ids_replace_directory_separators() {
        let found = scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/applications"));
        let ids: Vec<_> = found.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids, ["org-nested.desktop", "simple.desktop"]);
        assert!(found[0].entry.flag("NoDisplay"));
    }

    #[test]
    fn this_session_has_applications_installed() {
        let found: Vec<_> = data_dirs()
            .iter()
            .flat_map(|dir| scan(&dir.join("applications")))
            .collect();
        assert!(found.iter().any(|app| app.entry.get("Exec").is_some()));
    }
}
