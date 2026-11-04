use std::{ffi::OsString, path::PathBuf};

use tlab::Error;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub migrate: Option<PathBuf>,
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = OsString>) -> tlab::Result<Self> {
        let mut options = Self::default();
        while let Some(option) = args.next() {
            if option == "--migrate" && options.migrate.is_none() {
                let path = args
                    .next()
                    .filter(|value| !value.is_empty() && !value.to_string_lossy().starts_with('-'))
                    .ok_or_else(|| Error::InvalidInput("--migrate requires a directory".into()))?;
                options.migrate = Some(path.into());
            } else {
                return Err(Error::InvalidInput(
                    "unknown or repeated command line argument".into(),
                ));
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_arguments() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        assert_eq!(parse(&[]).unwrap(), Options::default());
        assert_eq!(
            parse(&["--migrate", "migration"]).unwrap().migrate,
            Some(PathBuf::from("migration"))
        );
        for args in [
            &["--migrate"][..],
            &["--unknown"],
            &["--migrate", "x", "extra"],
            &["--migrate", "--migrate"],
            &["--migrate", "x", "--migrate", "y"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
