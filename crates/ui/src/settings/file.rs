use std::fs;
use std::io;
use std::path::Path;

use toml_edit::ser::to_document;
use toml_edit::{DocumentMut, Item, Table};

use super::Settings;

/// A missing file is the defaults; a file that cannot be understood is an error.
pub(super) fn read(path: &Path) -> Result<Settings, String> {
    match fs::read_to_string(path) {
        Ok(text) => toml_edit::de::from_str::<Settings>(&text)
            .map(Settings::normalised)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error.to_string()),
    }
}

/// Puts each setting that differs from `base` into the file and leaves the rest of it as it is, so
/// comments, other keys and changes made by hand since `base` was read survive.
pub(super) fn write(path: &Path, ours: &Settings, base: &Settings) -> Result<(), String> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error| format!("the file is not valid, so it was left as it is: {error}"))?;
    if !apply(&mut document, ours, base)? {
        return Ok(());
    }
    replace(&path, &document.to_string()).map_err(|error| error.to_string())
}

/// Sets every value that differs between `ours` and `base`; true when there was one.
fn apply(document: &mut DocumentMut, ours: &Settings, base: &Settings) -> Result<bool, String> {
    let ours = to_document(ours).map_err(|error| error.to_string())?;
    let base = to_document(base).map_err(|error| error.to_string())?;
    let mut changed = false;
    for (section, table) in ours.iter() {
        let Some(table) = table.as_table_like() else {
            continue;
        };
        for (key, item) in table.iter() {
            let before = base.get(section).and_then(|section| section.get(key));
            if before.map(ToString::to_string) != Some(item.to_string()) {
                set(document, section, key, item)?;
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Replaces a value where it stands, so the comments around it stay.
fn set(document: &mut DocumentMut, section: &str, key: &str, item: &Item) -> Result<(), String> {
    let Item::Value(mut new) = item.clone() else {
        return Ok(());
    };
    let table = document
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| format!("`{section}` in the file is not a table"))?;
    match table.get_mut(key) {
        Some(old) => {
            if let Item::Value(before) = old {
                *new.decor_mut() = before.decor().clone();
            }
            *old = Item::Value(new);
        }
        None => {
            table.insert(key, Item::Value(new));
        }
    }
    Ok(())
}

/// Replaces the file in one step.
fn replace(path: &Path, text: &str) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let partial = path.with_extension("toml.partial");
    fs::write(&partial, text)?;
    fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::super::{Appearance, Scratch, Startup, Temperature};
    use super::*;

    fn settings(change: impl FnOnce(&mut Settings)) -> Settings {
        let mut settings = Settings::default();
        change(&mut settings);
        settings
    }

    fn file_in(scratch: &Scratch, text: &str) -> std::path::PathBuf {
        let path = scratch.0.join("settings.toml");
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_missing_file_is_the_defaults() {
        let scratch = Scratch::new("file-missing");
        assert_eq!(
            read(&scratch.0.join("settings.toml")),
            Ok(Settings::default())
        );
    }

    #[test]
    fn a_file_that_is_not_toml_is_an_error() {
        let scratch = Scratch::new("file-garbage");
        assert!(read(&file_in(&scratch, "this is = = not toml")).is_err());
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_an_error() {
        let scratch = Scratch::new("file-wrong-kind");
        assert!(read(&file_in(&scratch, "[units]\ntemperature = \"kelvin\"\n")).is_err());
    }

    #[test]
    fn a_partial_file_keeps_the_defaults_for_what_it_omits() {
        let scratch = Scratch::new("file-partial");
        let path = file_in(&scratch, "[units]\ntemperature = \"fahrenheit\"\n");
        let read = read(&path).unwrap();
        assert_eq!(read.units.temperature, Temperature::Fahrenheit);
        assert_eq!(read.startup, Startup::default());
        assert_eq!(read.appearance, Appearance::default());
    }

    #[test]
    fn keys_and_tables_this_version_does_not_know_are_ignored() {
        let scratch = Scratch::new("file-unknown");
        let path = file_in(
            &scratch,
            "[units]\ntemperature = \"fahrenheit\"\nsoon = 1\n\n[alerts]\nrule = []\n",
        );
        assert_eq!(
            read(&path).unwrap().units.temperature,
            Temperature::Fahrenheit
        );
    }

    #[test]
    fn a_text_size_off_the_notches_is_moved_onto_one() {
        let scratch = Scratch::new("file-notch");
        let path = file_in(&scratch, "[appearance]\ntext_size = 99\n");
        assert_eq!(read(&path).unwrap().appearance.text_size, 24);
    }

    #[test]
    fn a_first_change_makes_the_file_with_only_that_setting_in_it() {
        let scratch = Scratch::new("file-first");
        let path = scratch.0.join("peepers/settings.toml");
        let ours = settings(|settings| settings.startup.tray = false);
        write(&path, &ours, &Settings::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[startup]\ntray = false\n"
        );
    }

    #[test]
    fn no_change_leaves_no_file() {
        let scratch = Scratch::new("file-unchanged");
        let path = scratch.0.join("settings.toml");
        write(&path, &Settings::default(), &Settings::default()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_change_keeps_the_comments_around_it_and_everything_else_in_the_file() {
        let scratch = Scratch::new("file-comments");
        let before = "# mine\n\n[appearance]\n# why not\ntheme = \"Nord\" # the cold one\ntext_size = 20\n\n[alerts]\nrule = []\n";
        let path = file_in(&scratch, before);
        let base = read(&path).unwrap();
        let ours = Settings {
            appearance: Appearance {
                theme: "Dracula".to_owned(),
                ..base.appearance.clone()
            },
            ..base.clone()
        };
        write(&path, &ours, &base).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before.replace("\"Nord\"", "\"Dracula\"")
        );
    }

    #[test]
    fn a_setting_left_alone_is_not_written_over_a_hand_edit() {
        let scratch = Scratch::new("file-hand-edit");
        let path = file_in(&scratch, "[appearance]\ntheme = \"Nord\"\n");
        let ours = settings(|settings| settings.startup.tray = false);
        write(&path, &ours, &Settings::default()).unwrap();
        let after = read(&path).unwrap();
        assert_eq!(after.appearance.theme, "Nord");
        assert!(!after.startup.tray);
    }

    #[test]
    fn a_table_written_inline_is_changed_in_place() {
        let scratch = Scratch::new("file-inline");
        let path = file_in(&scratch, "units = { temperature = \"celsius\" }\n");
        let ours = settings(|settings| settings.units.temperature = Temperature::Fahrenheit);
        write(&path, &ours, &Settings::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "units = { temperature = \"fahrenheit\" }\n"
        );
    }

    #[test]
    fn a_file_that_is_not_valid_is_left_untouched() {
        let scratch = Scratch::new("file-invalid");
        let path = file_in(&scratch, "this is = = not toml");
        let ours = settings(|settings| settings.startup.tray = false);
        assert!(write(&path, &ours, &Settings::default()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "this is = = not toml");
    }

    #[test]
    fn a_section_that_is_not_a_table_is_reported_and_left_alone() {
        let scratch = Scratch::new("file-not-table");
        let path = file_in(&scratch, "units = 3\n");
        let ours = settings(|settings| settings.units.temperature = Temperature::Fahrenheit);
        let error = write(&path, &ours, &Settings::default()).unwrap_err();
        assert!(error.contains("units"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "units = 3\n");
    }

    #[test]
    fn saving_through_a_link_changes_what_it_points_at_and_keeps_the_link() {
        let scratch = Scratch::new("file-link");
        let real = scratch.0.join("real.toml");
        fs::write(&real, "[startup]\ntray = true\n").unwrap();
        let link = scratch.0.join("settings.toml");
        symlink(&real, &link).unwrap();
        let ours = settings(|settings| settings.startup.tray = false);
        write(&link, &ours, &Settings::default()).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "[startup]\ntray = false\n"
        );
    }

    #[test]
    fn a_save_leaves_nothing_beside_the_file() {
        let scratch = Scratch::new("file-clean");
        let path = scratch.0.join("settings.toml");
        write(
            &path,
            &settings(|settings| settings.startup.tray = false),
            &Settings::default(),
        )
        .unwrap();
        write(
            &path,
            &settings(|settings| settings.startup.tray = true),
            &settings(|settings| settings.startup.tray = false),
        )
        .unwrap();
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
        assert!(read(&path).unwrap().startup.tray);
    }
}
