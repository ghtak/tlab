use std::{ffi::OsString, path::PathBuf};

use tlab::Error;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub migrate: Option<PathBuf>,
    pub bootstrap_admin: Option<BootstrapAdmin>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BootstrapAdmin {
    pub email: String,
    pub nickname: String,
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = OsString>) -> tlab::Result<Self> {
        let mut options = Self::default();
        while let Some(option) = args.next() {
            if option == "bootstrap-admin" && options == Self::default() {
                let mut email = None;
                let mut nickname = None;
                while let Some(flag) = args.next() {
                    let slot = if flag == "--email" && email.is_none() {
                        &mut email
                    } else if flag == "--nickname" && nickname.is_none() {
                        &mut nickname
                    } else {
                        return Err(Error::InvalidInput("invalid bootstrap-admin arguments; use --email and --nickname, password via stdin".into()));
                    };
                    *slot = Some(
                        args.next()
                            .filter(|value| !value.to_string_lossy().starts_with('-'))
                            .and_then(|value| value.into_string().ok())
                            .ok_or_else(|| {
                                Error::InvalidInput(
                                    "bootstrap-admin option requires a value".into(),
                                )
                            })?,
                    );
                }
                options.bootstrap_admin = Some(BootstrapAdmin {
                    email: email.ok_or_else(|| {
                        Error::InvalidInput("bootstrap-admin requires --email".into())
                    })?,
                    nickname: nickname.ok_or_else(|| {
                        Error::InvalidInput("bootstrap-admin requires --nickname".into())
                    })?,
                });
            } else if option == "--migrate" && options == Self::default() {
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
            &["bootstrap-admin"],
            &["bootstrap-admin", "--email", "x"],
            &[
                "--migrate",
                "x",
                "bootstrap-admin",
                "--email",
                "x",
                "--nickname",
                "n",
            ],
            &[
                "bootstrap-admin",
                "--email",
                "x",
                "--nickname",
                "n",
                "--migrate",
                "x",
            ],
            &[
                "bootstrap-admin",
                "--email",
                "x",
                "--nickname",
                "n",
                "--password",
                "secret",
            ],
            &[
                "bootstrap-admin",
                "--email",
                "x",
                "--email",
                "y",
                "--nickname",
                "n",
            ],
        ] {
            assert!(parse(args).is_err());
        }
        assert_eq!(
            parse(&[
                "bootstrap-admin",
                "--email",
                "admin@example.test",
                "--nickname",
                "admin"
            ])
            .unwrap()
            .bootstrap_admin,
            Some(BootstrapAdmin {
                email: "admin@example.test".into(),
                nickname: "admin".into()
            })
        );
    }
}
