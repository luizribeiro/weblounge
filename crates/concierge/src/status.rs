use std::{fs, path::Path};

use anyhow::Context;

use crate::config::Config;

const PROBE: &str = ".concierge-status-probe";

/// What an admin needs to see that this process runs with the service's
/// configuration, state and secrets.
pub fn report(config: &Config) -> anyhow::Result<String> {
    check_writable(&config.state_dir)?;
    let key = config.read_session_key()?;
    Ok(format!(
        "public url: {}\nstate directory: {} (writable)\nsession key: {} bytes\n",
        config.public_url,
        config.state_dir.display(),
        key.len()
    ))
}

fn check_writable(dir: &Path) -> anyhow::Result<()> {
    let probe = dir.join(PROBE);
    fs::write(&probe, b"")
        .and_then(|()| fs::remove_file(&probe))
        .with_context(|| format!("state directory {} is not writable", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SESSION_KEY_CREDENTIAL;
    use std::os::unix::fs::PermissionsExt;

    fn config(state_dir: &Path, credentials_dir: &Path) -> Config {
        Config {
            listen: "127.0.0.1:8080".parse().unwrap(),
            public_url: "https://members.weblounge.localhost:10443".into(),
            assets_dir: "/nix/store/asqgxh40xn390yzqhq97lkbqf5sslbyr-weblounge-web/members".into(),
            state_dir: state_dir.into(),
            credentials_dir: credentials_dir.into(),
        }
    }

    #[test]
    fn reports_the_url_state_directory_and_key_and_leaves_no_trace() {
        let (state, credentials) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(
            credentials.path().join(SESSION_KEY_CREDENTIAL),
            "k".repeat(64),
        )
        .unwrap();

        let report = report(&config(state.path(), credentials.path())).unwrap();

        assert_eq!(
            report,
            format!(
                "public url: https://members.weblounge.localhost:10443\n\
                 state directory: {} (writable)\n\
                 session key: 64 bytes\n",
                state.path().display()
            )
        );
        assert_eq!(fs::read_dir(state.path()).unwrap().count(), 0);
    }

    #[test]
    fn fails_on_a_state_directory_it_cannot_write() {
        let (state, credentials) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::set_permissions(state.path(), fs::Permissions::from_mode(0o500)).unwrap();

        let error = report(&config(state.path(), credentials.path())).unwrap_err();

        assert!(error.to_string().contains("is not writable"), "{error}");
    }
}
