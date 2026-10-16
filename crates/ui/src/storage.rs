use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where exports and recordings go until the user names another folder.
pub fn default_folder() -> String {
    format!("{}/Documents/Peepers", env::var("HOME").unwrap_or_default())
}

/// Writes `text` to a new file in `directory`, named for `stem` and the time, and returns where
/// it went.
pub fn export(directory: &Path, stem: &str, text: &str) -> Result<PathBuf, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let path = directory.join(format!("peepers-{stem}-{seconds}.txt"));
    fs::create_dir_all(directory)
        .and_then(|()| write_new(&path, text))
        .map_err(|error| error.to_string())?;
    Ok(path)
}

/// Fails rather than replace a file that is already there.
fn write_new(path: &Path, text: &str) -> io::Result<()> {
    File::create_new(path)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_exists_is_never_replaced() {
        let path = env::current_exe()
            .unwrap()
            .with_file_name(format!("storage-test-{}.txt", std::process::id()));
        write_new(&path, "one").unwrap();
        let error = write_new(&path, "two").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_export_names_its_file_for_what_it_holds() {
        let directory = env::current_exe()
            .unwrap()
            .with_file_name(format!("storage-export-{}", std::process::id()));
        let path = export(&directory, "unit", "text").unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("peepers-unit-")
        );
        assert_eq!(path.extension().unwrap(), "txt");
        assert_eq!(fs::read_to_string(path).unwrap(), "text");
        fs::remove_dir_all(directory).unwrap();
    }
}
