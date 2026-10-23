use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

pub struct LocationInputs {
    pub data_dir_override: Option<OsString>,
    pub create_opt_in: bool,
    pub local_app_data: Option<PathBuf>,
    pub debug_build: bool,
    pub dev_default: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DataLocation {
    pub directory: PathBuf,
    pub database: PathBuf,
    pub is_real: bool,
    pub may_create: bool,
}

#[derive(Debug)]
pub enum LocationError {
    RelativeOverride(PathBuf),
    NoLocalAppData,
    Unresolvable {
        path: PathBuf,
        error: io::Error,
    },
    AmbiguousAlias {
        requested: PathBuf,
        real: PathBuf,
        reason: &'static str,
    },
}

impl std::fmt::Display for LocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RelativeOverride(path) => write!(
                f,
                "TASKCASCADE_DATA_DIR must be an absolute path: {}",
                path.display()
            ),
            Self::NoLocalAppData => write!(
                f,
                "LOCALAPPDATA is not set; the real TaskCascade data directory cannot be resolved."
            ),
            Self::Unresolvable { path, error } => {
                write!(f, "Cannot resolve {}: {error}", path.display())
            }
            Self::AmbiguousAlias {
                requested,
                real,
                reason,
            } => write!(
                f,
                "Ambiguous data directory {} (real directory: {}): {reason}",
                requested.display(),
                real.display()
            ),
        }
    }
}

impl std::error::Error for LocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unresolvable { error, .. } => Some(error),
            _ => None,
        }
    }
}

pub fn real_directory(local_app_data: &Path) -> PathBuf {
    local_app_data.join("TaskCascade")
}

pub fn requested_directory(inputs: &LocationInputs) -> Result<PathBuf, LocationError> {
    if let Some(value) = &inputs.data_dir_override {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(LocationError::RelativeOverride(path));
        }
        return Ok(path);
    }
    if inputs.debug_build {
        Ok(inputs.dev_default.clone())
    } else {
        inputs
            .local_app_data
            .as_deref()
            .map(real_directory)
            .ok_or(LocationError::NoLocalAppData)
    }
}

pub struct ResolvedPath {
    pub canonical: PathBuf,
    pub exists: bool,
}

fn unresolvable(path: &Path, error: io::Error) -> LocationError {
    LocationError::Unresolvable {
        path: path.to_owned(),
        error,
    }
}

fn exists(path: &Path) -> Result<bool, LocationError> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A dangling link is not a missing directory that may safely be created.
            match std::fs::symlink_metadata(path) {
                Ok(_) => Err(unresolvable(path, error)),
                Err(missing) if missing.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(other) => Err(unresolvable(path, other)),
            }
        }
        Err(error) => Err(unresolvable(path, error)),
    }
}

pub fn filesystem_identity(path: &Path) -> Result<ResolvedPath, LocationError> {
    if exists(path)? {
        return Ok(ResolvedPath {
            canonical: std::fs::canonicalize(path).map_err(|error| unresolvable(path, error))?,
            exists: true,
        });
    }
    let absolute = std::path::absolute(path).map_err(|error| unresolvable(path, error))?;
    let mut ancestor = absolute.as_path();
    while !exists(ancestor)? {
        ancestor = ancestor.parent().ok_or_else(|| {
            unresolvable(
                path,
                io::Error::new(io::ErrorKind::NotFound, "no existing ancestor"),
            )
        })?;
    }
    let mut canonical =
        std::fs::canonicalize(ancestor).map_err(|error| unresolvable(path, error))?;
    for component in absolute
        .strip_prefix(ancestor)
        .map_err(|error| unresolvable(path, io::Error::other(error)))?
        .components()
    {
        match component {
            std::path::Component::ParentDir => {
                canonical.pop();
            }
            std::path::Component::CurDir => {}
            other => canonical.push(other),
        }
    }
    Ok(ResolvedPath {
        canonical,
        exists: false,
    })
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str().to_string_lossy().to_lowercase()
            == right.as_os_str().to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn strictly_contains(parent: &Path, child: &Path) -> bool {
    child
        .ancestors()
        .skip(1)
        .any(|ancestor| paths_equal(parent, ancestor))
}

pub fn creation_policy(is_real: bool, debug_build: bool, opt_in: bool) -> bool {
    opt_in || (debug_build && !is_real)
}

pub fn resolve(inputs: &LocationInputs) -> Result<DataLocation, LocationError> {
    let directory = requested_directory(inputs)?;
    let real = inputs
        .local_app_data
        .as_deref()
        .map(real_directory)
        .ok_or(LocationError::NoLocalAppData)?;
    let requested_identity = filesystem_identity(&directory)?;
    let real_identity = filesystem_identity(&real)?;
    let is_real = if requested_identity.exists && real_identity.exists {
        same_file::is_same_file(&directory, &real)
            .map_err(|error| unresolvable(&directory, error))?
    } else {
        paths_equal(&requested_identity.canonical, &real_identity.canonical)
    };
    let database = directory.join("taskcascade.sqlite");
    if !is_real {
        let reason = if strictly_contains(&real_identity.canonical, &requested_identity.canonical) {
            Some("the requested directory is nested inside the real directory")
        } else if strictly_contains(&requested_identity.canonical, &real_identity.canonical) {
            Some("the requested directory contains the real directory")
        } else {
            let real_database = real.join("taskcascade.sqlite");
            if exists(&database)?
                && exists(&real_database)?
                && same_file::is_same_file(&database, &real_database)
                    .map_err(|error| unresolvable(&database, error))?
            {
                Some("the requested database is an alias of the real database")
            } else {
                None
            }
        };
        if let Some(reason) = reason {
            return Err(LocationError::AmbiguousAlias {
                requested: directory,
                real,
                reason,
            });
        }
    }
    Ok(DataLocation {
        database,
        directory,
        is_real,
        may_create: creation_policy(is_real, inputs.debug_build, inputs.create_opt_in),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(root: &Path) -> LocationInputs {
        LocationInputs {
            data_dir_override: None,
            create_opt_in: false,
            local_app_data: Some(root.to_owned()),
            debug_build: false,
            dev_default: root.join("dev-data"),
        }
    }

    #[test]
    fn defaults_overrides_and_policy() {
        let root = tempfile::tempdir().unwrap();
        let mut inputs = inputs(root.path());
        let location = resolve(&inputs).unwrap();
        assert_eq!(location.directory, real_directory(root.path()));
        assert!(location.is_real);
        assert!(!location.may_create);
        inputs.create_opt_in = true;
        assert!(resolve(&inputs).unwrap().may_create);
        inputs.create_opt_in = false;
        inputs.debug_build = true;
        let location = resolve(&inputs).unwrap();
        assert_eq!(location.directory, inputs.dev_default);
        assert!(!location.is_real);
        assert!(location.may_create);
        for debug in [false, true] {
            inputs.debug_build = debug;
            inputs.data_dir_override = Some(root.path().join("override").into());
            let location = resolve(&inputs).unwrap();
            assert_eq!(location.directory, root.path().join("override"));
            assert!(!location.is_real);
            assert_eq!(location.may_create, debug);
        }
        inputs.data_dir_override = Some("relative".into());
        assert!(matches!(
            requested_directory(&inputs),
            Err(LocationError::RelativeOverride(_))
        ));
        inputs.data_dir_override = None;
        inputs.local_app_data = None;
        inputs.debug_build = false;
        assert!(matches!(
            resolve(&inputs),
            Err(LocationError::NoLocalAppData)
        ));
        inputs.debug_build = true;
        assert_eq!(requested_directory(&inputs).unwrap(), inputs.dev_default);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn creation_policy_truth_table() {
        for (is_real, debug, opt_in, expected) in [
            (false, false, false, false),
            (false, false, true, true),
            (false, true, false, true),
            (false, true, true, true),
            (true, false, false, false),
            (true, false, true, true),
            (true, true, false, false),
            (true, true, true, true),
        ] {
            assert_eq!(
                creation_policy(is_real, debug, opt_in),
                expected,
                "is_real={is_real} debug={debug} opt_in={opt_in}"
            );
        }
    }

    #[test]
    fn real_directory_aliases_existing_and_missing() {
        for exists in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let real = real_directory(root.path());
            if exists {
                std::fs::create_dir(&real).unwrap();
            }
            let mut aliases = vec![
                real.join("..").join("TaskCascade"),
                root.path().join(".").join("TaskCascade"),
                PathBuf::from(format!("{}{}", real.display(), std::path::MAIN_SEPARATOR)),
            ];
            #[cfg(windows)]
            {
                aliases.push(PathBuf::from(
                    real.as_os_str().to_string_lossy().to_uppercase(),
                ));
                aliases.push(PathBuf::from(format!(r"\\?\{}", real.display())));
            }
            let mut inputs = inputs(root.path());
            inputs.debug_build = true;
            for alias in aliases.drain(..) {
                inputs.data_dir_override = Some(alias.clone().into());
                let location = resolve(&inputs).unwrap();
                assert!(location.is_real, "{} (exists={exists})", alias.display());
                assert!(!location.may_create);
                assert_eq!(location.directory, alias);
                assert_eq!(location.database, alias.join("taskcascade.sqlite"));
                assert!(paths_equal(
                    &filesystem_identity(&location.directory).unwrap().canonical,
                    &filesystem_identity(&real).unwrap().canonical
                ));
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn junction_is_real() {
        let root = tempfile::tempdir().unwrap();
        let real = real_directory(root.path());
        std::fs::create_dir(&real).unwrap();
        let alias = root.path().join("alias");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&real)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut inputs = inputs(root.path());
        inputs.debug_build = true;
        inputs.data_dir_override = Some(alias.clone().into());
        let location = resolve(&inputs).unwrap();
        assert!(location.is_real);
        assert!(!location.may_create);
        std::fs::remove_dir(alias).unwrap();
    }

    #[test]
    fn nested_and_containing_directories_are_rejected() {
        for exists in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let real = real_directory(root.path());
            if exists {
                std::fs::create_dir(&real).unwrap();
            }
            let mut inputs = inputs(root.path());
            for alias in [real.join("scratch"), root.path().to_owned()] {
                inputs.data_dir_override = Some(alias.into());
                assert!(matches!(
                    resolve(&inputs),
                    Err(LocationError::AmbiguousAlias { .. })
                ));
            }
        }
    }

    #[test]
    fn hard_link_to_real_database_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let real = real_directory(root.path());
        let scratch = root.path().join("scratch");
        std::fs::create_dir(&real).unwrap();
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(
            real.join("taskcascade.sqlite"),
            b"synthetic identity fixture",
        )
        .unwrap();
        std::fs::hard_link(
            real.join("taskcascade.sqlite"),
            scratch.join("taskcascade.sqlite"),
        )
        .unwrap();
        let mut inputs = inputs(root.path());
        inputs.data_dir_override = Some(scratch.into());
        assert!(matches!(
            resolve(&inputs),
            Err(LocationError::AmbiguousAlias { .. })
        ));
    }

    #[test]
    fn unrelated_scratch_may_create_in_debug() {
        let root = tempfile::tempdir().unwrap();
        let mut inputs = inputs(root.path());
        inputs.debug_build = true;
        inputs.data_dir_override = Some(root.path().join("scratch").into());
        let location = resolve(&inputs).unwrap();
        assert!(!location.is_real);
        assert!(location.may_create);
    }
}
