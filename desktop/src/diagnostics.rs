use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::data_location::DataLocation;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
struct Output {
    path: Option<PathBuf>,
    file: Option<File>,
}

#[derive(Default)]
pub struct Diagnostics {
    output: Mutex<Output>,
}

impl Diagnostics {
    pub fn initialize(
        &self,
        location: &DataLocation,
        canonical_database: &Path,
        debug_build: bool,
    ) {
        let path = location.directory.join("logs/taskcascade-desktop.log");
        let opened = open_log(&path);
        let error = opened.as_ref().err().map(ToString::to_string);
        *self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Output {
            path: Some(path),
            file: opened.ok(),
        };
        self.log(&format!(
            "startup profile={} version={} requested_database={} canonical_database={} is_real={} may_create={}",
            if debug_build { "debug" } else { "release" },
            env!("CARGO_PKG_VERSION"),
            location.database.display(),
            canonical_database.display(),
            location.is_real,
            location.may_create,
        ));
        if let Some(error) = error {
            self.log(&format!("runtime log unavailable: {error}"));
        }
    }

    pub fn log(&self, message: &str) {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for message in message.lines() {
            let line = format!(
                "{} pid={} {message}",
                chrono::Utc::now().to_rfc3339(),
                std::process::id()
            );
            let _ = writeln!(io::stderr().lock(), "{line}");
            if let Some(file) = &mut output.file {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    pub fn path_description(&self) -> String {
        self.output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .path
            .as_ref()
            .map_or_else(
                || "not initialized (data location unresolved)".to_owned(),
                |path| path.display().to_string(),
            )
    }
}

fn open_log(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
    {
        let previous = path.with_extension("log.1");
        match std::fs::remove_file(&previous) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::rename(path, previous)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(root: &Path) -> DataLocation {
        DataLocation {
            directory: root.to_owned(),
            database: root.join("taskcascade.sqlite"),
            is_real: false,
            may_create: true,
        }
    }

    #[test]
    fn log_records_location_and_rotates_one_generation() {
        let root = tempfile::tempdir().unwrap();
        let location = location(root.path());
        let path = root.path().join("logs/taskcascade-desktop.log");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        let previous = path.with_extension("log.1");
        std::fs::write(&previous, "old generation").unwrap();
        let file = File::create(&path).unwrap();
        file.set_len(MAX_LOG_BYTES).unwrap();
        drop(file);
        let diagnostics = Diagnostics::default();
        diagnostics.initialize(&location, &location.database, true);
        diagnostics.log("shutdown test\nsecond line");
        let log = std::fs::read_to_string(path).unwrap();
        let first = log.lines().next().unwrap();
        assert!(first.contains("startup profile=debug version="));
        assert!(first.contains(&format!(
            "requested_database={}",
            location.database.display()
        )));
        assert!(first.contains(&format!(
            "canonical_database={}",
            location.database.display()
        )));
        assert!(first.contains("is_real=false may_create=true"));
        for line in log.lines() {
            assert!(
                chrono::DateTime::parse_from_rfc3339(line.split_whitespace().next().unwrap())
                    .is_ok()
            );
            assert!(line.contains(&format!("pid={}", std::process::id())));
        }
        assert_eq!(previous.metadata().unwrap().len(), MAX_LOG_BYTES);
    }

    #[test]
    fn unavailable_log_is_best_effort() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("logs"), "a file blocks the log directory").unwrap();
        let location = location(root.path());
        let diagnostics = Diagnostics::default();
        diagnostics.initialize(&location, &location.database, true);
        diagnostics.log("still running");
        assert!(!location.database.exists());
        assert!(diagnostics
            .path_description()
            .contains("taskcascade-desktop.log"));
    }
}
