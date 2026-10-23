use crate::data_location::DataLocation;

pub const REAL_IDENTIFIER: &str = "se.pensjo.taskcascade";
pub const DEV_IDENTIFIER: &str = "se.pensjo.taskcascade.dev";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub identifier: &'static str,
    pub title: &'static str,
}

pub fn identity_for(location: &DataLocation) -> Identity {
    if location.is_real {
        Identity {
            identifier: REAL_IDENTIFIER,
            title: "TaskCascade",
        }
    } else {
        Identity {
            identifier: DEV_IDENTIFIER,
            title: "TaskCascade (dev)",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::data_location::{self, LocationInputs};

    fn inputs(local_app_data: &Path) -> LocationInputs {
        LocationInputs {
            data_dir_override: None,
            create_opt_in: false,
            local_app_data: Some(local_app_data.to_owned()),
            debug_build: false,
            dev_default: local_app_data.join("dev-data"),
        }
    }

    fn assert_real(location: &DataLocation) {
        assert_eq!(
            identity_for(location),
            Identity {
                identifier: REAL_IDENTIFIER,
                title: "TaskCascade",
            }
        );
    }

    fn assert_dev(location: &DataLocation) {
        assert_eq!(
            identity_for(location),
            Identity {
                identifier: DEV_IDENTIFIER,
                title: "TaskCascade (dev)",
            }
        );
    }

    #[test]
    fn real_location_gets_the_real_identity() {
        let root = tempfile::tempdir().unwrap();
        let location = data_location::resolve(&inputs(root.path())).unwrap();
        assert!(location.is_real);
        assert_real(&location);
    }

    #[test]
    fn debug_default_gets_the_development_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut inputs = inputs(root.path());
        inputs.debug_build = true;
        let location = data_location::resolve(&inputs).unwrap();
        assert_eq!(location.directory, inputs.dev_default);
        assert!(!location.is_real);
        assert_dev(&location);
    }

    #[test]
    fn release_scratch_override_gets_the_development_identity() {
        let root = tempfile::tempdir().unwrap();
        let scratch = root.path().join("release-scratch");
        let mut inputs = inputs(root.path());
        inputs.data_dir_override = Some(scratch.clone().into());
        let location = data_location::resolve(&inputs).unwrap();
        assert_eq!(location.directory, scratch);
        assert!(!location.is_real);
        assert!(!location.may_create);
        assert_dev(&location);
    }

    #[test]
    fn aliases_of_the_real_directory_get_the_real_identity() {
        for exists in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let real = data_location::real_directory(root.path());
            if exists {
                std::fs::create_dir(&real).unwrap();
            }
            let mut inputs = inputs(root.path());
            inputs.debug_build = true;
            for alias in data_location::real_directory_aliases(root.path()) {
                inputs.data_dir_override = Some(alias.into());
                let location = data_location::resolve(&inputs).unwrap();
                assert!(location.is_real);
                assert!(!location.may_create);
                assert_real(&location);
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn junction_alias_of_the_real_directory_gets_the_real_identity() {
        let root = tempfile::tempdir().unwrap();
        let alias = data_location::real_directory_junction(root.path());
        let mut inputs = inputs(root.path());
        inputs.debug_build = true;
        inputs.data_dir_override = Some(alias.clone().into());
        let location = data_location::resolve(&inputs).unwrap();
        assert!(location.is_real);
        assert!(!location.may_create);
        assert_real(&location);
        std::fs::remove_dir(alias).unwrap();
    }
}
