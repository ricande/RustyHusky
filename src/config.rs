//! The project's name, in one place. A new game built from the template
//! changes these three constants (and the name in `Cargo.toml`); everything
//! else – window title, menu title, folders, file names and environment
//! variables – follows.

use std::ffi::OsString;

/// Shown as the window title and the main menu title, and used for the
/// data and video folders on Windows and macOS. A product name, the same in
/// every language.
pub const APP_NAME: &str = "RustyHusky";

/// Lower-case id: the Linux data folder and the prefix of video and
/// temporary file names.
pub const APP_ID: &str = "rustyhusky";

/// Prefix of the environment variables, e.g. `RUSTYHUSKY_LANG`.
pub const ENV_PREFIX: &str = "RUSTYHUSKY";

/// The environment variable `<ENV_PREFIX>_<name>`, if set and not empty.
pub fn env(name: &str) -> Option<OsString> {
    std::env::var_os(format!("{ENV_PREFIX}_{name}")).filter(|v| !v.is_empty())
}

/// Like `env`, as a string (`None` if it is not valid Unicode).
pub fn env_str(name: &str) -> Option<String> {
    env(name)?.into_string().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_consistent() {
        assert_eq!(APP_ID, APP_ID.to_lowercase());
        assert_eq!(ENV_PREFIX, ENV_PREFIX.to_uppercase());
        assert!(!APP_NAME.is_empty() && !APP_ID.contains(char::is_whitespace));
        assert_eq!(env("SURELY_NOT_SET_ANYWHERE"), None);
    }
}
