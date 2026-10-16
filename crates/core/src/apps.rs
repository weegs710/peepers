use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use crate::desktop::{self, Found};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
    pub command: Option<String>,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub path: PathBuf,
    pub scope: Scope,
    /// Entries that exist for file-type handlers and helpers rather than for launching.
    pub hidden: bool,
}

/// Installed applications as the desktop sees them: the first entry for each ID in the data
/// directories, so a user's own entry shadows a system one.
#[must_use]
pub fn read() -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps: Vec<App> = desktop::data_dirs()
        .iter()
        .flat_map(|dir| desktop::scan(&dir.join("applications")))
        .filter(|found| seen.insert(found.id.clone()))
        .filter_map(app)
        .collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn app(found: Found) -> Option<App> {
    let entry = &found.entry;
    (entry.get("Type") == Some("Application") && !entry.flag("Hidden")).then(|| App {
        name: entry
            .get("Name")
            .map_or_else(|| found.id.clone(), str::to_owned),
        comment: entry.get("Comment").map(str::to_owned),
        command: entry.get("Exec").map(str::to_owned),
        icon: entry.get("Icon").map(str::to_owned),
        categories: entry.list("Categories"),
        scope: scope(&found.path),
        hidden: entry.flag("NoDisplay"),
        path: found.path,
        id: found.id,
    })
}

/// User scope is the home directory, user profiles and per-user package environments.
fn scope(path: &Path) -> Scope {
    let home = env::var_os("HOME").map(PathBuf::from);
    if home.is_some_and(|home| path.starts_with(home)) || path.starts_with("/etc/profiles/per-user")
    {
        Scope::User
    } else {
        Scope::System
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Found {
        Found {
            id: "x.desktop".to_owned(),
            path: PathBuf::from("/usr/share/applications/x.desktop"),
            entry: desktop::Entry::parse(text),
        }
    }

    #[test]
    fn applications_carry_their_launch_details() {
        let app = app(found(
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nCategories=A;B;\nNoDisplay=true\n",
        ))
        .unwrap();
        assert_eq!(
            (app.name.as_str(), app.command.as_deref(), app.hidden),
            ("X", Some("x"), true)
        );
        assert_eq!(app.categories, ["A", "B"]);
        assert_eq!(app.scope, Scope::System);
    }

    #[test]
    fn links_and_withdrawn_entries_are_not_applications() {
        assert!(app(found("[Desktop Entry]\nType=Link\nName=X\n")).is_none());
        assert!(
            app(found(
                "[Desktop Entry]\nType=Application\nName=X\nHidden=true\n"
            ))
            .is_none()
        );
    }

    #[test]
    fn entries_without_a_name_fall_back_to_their_id() {
        assert_eq!(
            app(found("[Desktop Entry]\nType=Application\n"))
                .unwrap()
                .name,
            "x.desktop"
        );
    }

    #[test]
    fn home_and_per_user_profiles_are_user_scope() {
        assert_eq!(
            scope(Path::new(
                "/etc/profiles/per-user/me/share/applications/a.desktop"
            )),
            Scope::User
        );
        assert_eq!(
            scope(Path::new("/usr/share/applications/a.desktop")),
            Scope::System
        );
    }

    #[test]
    fn this_session_lists_apps_once_each_in_name_order() {
        let apps = read();
        assert!(!apps.is_empty());
        let ids: HashSet<_> = apps.iter().map(|app| &app.id).collect();
        assert_eq!(ids.len(), apps.len());
        assert!(
            apps.windows(2)
                .all(|pair| pair[0].name.to_lowercase() <= pair[1].name.to_lowercase())
        );
    }
}
