//! Container-only launcher, separate from the ACP executable. No network or agent loop.
use openwebui_acp::{
    error::{Error, Result},
    secure_file,
};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::Command,
};

/// Kubernetes projects Secret keys through symlinks. Only this explicit,
/// container-only initializer accepts them, confined to its read-only input
/// mount. The ACP runtime continues to reject symlinks and nonprivate files.
#[cfg(unix)]
fn prepare(source: &Path, private_volume: &Path, state_volume: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    fn directory(path: &Path) -> Result<()> {
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::State("Cannot create private container directory")),
        }
        let meta = fs::symlink_metadata(path)
            .map_err(|_| Error::State("Cannot inspect private container directory"))?;
        if !meta.is_dir()
            || meta.is_symlink()
            || meta.uid() != rustix::process::geteuid().as_raw()
            || meta.mode() & 0o077 != 0
        {
            return Err(Error::State(
                "Container directory must be owned, private, and not a symlink",
            ));
        }
        Ok(())
    }

    let source_meta = fs::symlink_metadata(source)
        .map_err(|_| Error::State("Cannot inspect projected credential directory"))?;
    if !source_meta.is_dir() || source_meta.is_symlink() {
        return Err(Error::State(
            "Projected credential mount must be a real directory",
        ));
    }
    let source = source
        .canonicalize()
        .map_err(|_| Error::State("Cannot resolve projected credential mount"))?;
    let destination = private_volume.join("keys");
    directory(&destination)?;
    directory(&state_volume.join("state"))?;
    for name in ["openwebui-key", "buzz-key"] {
        let resolved = source
            .join(name)
            .canonicalize()
            .map_err(|_| Error::State("Required projected credential is missing"))?;
        if !resolved.starts_with(&source) {
            return Err(Error::State("Projected credential escapes its mount"));
        }
        let mut file = secure_file::private_options()
            .read(true)
            .open(&resolved)
            .map_err(|_| Error::State("Cannot open projected credential"))?;
        let meta = file
            .metadata()
            .map_err(|_| Error::State("Cannot inspect projected credential"))?;
        if !meta.is_file()
            || meta.len() > secure_file::MAX_SECRET_BYTES
            || meta.nlink() != 1
            || meta.mode() & 0o022 != 0
            || ![0, rustix::process::geteuid().as_raw()].contains(&meta.uid())
        {
            return Err(Error::State(
                "Projected credential has unsafe type, size, ownership, or permissions",
            ));
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        Read::by_ref(&mut file)
            .take(secure_file::MAX_SECRET_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::State("Cannot read projected credential"))?;
        if bytes.len() as u64 > secure_file::MAX_SECRET_BYTES
            || std::str::from_utf8(&bytes).map_or(true, |s| s.trim().is_empty())
        {
            return Err(Error::State(
                "Projected credential must be bounded, nonempty UTF-8",
            ));
        }
        let target = destination.join(name);
        if target.symlink_metadata().is_ok() {
            let _existing = zeroize::Zeroizing::new(secure_file::read_secret(&target)?);
        }
        let temporary = destination.join(format!(".{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut output = secure_file::private_options()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| Error::State("Cannot create private credential"))?;
            output
                .write_all(&bytes)
                .and_then(|_| output.sync_all())
                .map_err(|_| Error::State("Cannot save private credential"))?;
            fs::rename(&temporary, &target)
                .map_err(|_| Error::State("Cannot install private credential"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }
    File::open(&destination)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| Error::State("Cannot sync private credentials"))?;
    Ok(())
}

fn launch() -> Result<()> {
    let key = secure_file::read_secret(Path::new("/run/secrets/buzz-key"))?;
    // Validate both mounts before starting the upstream process.
    let _webui_key = secure_file::read_secret(Path::new("/run/secrets/openwebui-key"))?;
    let authors = std::env::var("OPENWEBUI_ACP_BUZZ_ALLOWED_AUTHORS")
        .map_err(|_| Error::Config("Set explicit requester public keys".into()))?;
    let channels = std::env::var("OPENWEBUI_ACP_BUZZ_ALLOWED_CHANNELS")
        .map_err(|_| Error::Config("Set explicit channel UUIDs".into()))?;
    let timeout = std::env::var("OPENWEBUI_ACP_TIMEOUT_SECS")
        .unwrap_or_else(|_| "600".into())
        .parse::<u64>()
        .ok()
        .filter(|n| (1..=3600).contains(n))
        .ok_or_else(|| Error::Config("Turn timeout must be between 1 and 3600 seconds".into()))?;
    let mut command = Command::new("/usr/local/bin/buzz-acp");
    command
        .env("BUZZ_PRIVATE_KEY", key.trim())
        .env("OPENWEBUI_API_KEY_FILE", "/run/secrets/openwebui-key")
        .env_remove("OPENWEBUI_API_KEY")
        .env("BUZZ_ACP_AGENT_COMMAND", "/usr/local/bin/openwebui-acp")
        .env(
            "BUZZ_ACP_AGENT_ARGS",
            "--buzz-auto-publish,true,--state-dir,/state",
        )
        .env("BUZZ_ACP_AGENTS", "1")
        .env("BUZZ_ACP_DEDUP", "queue")
        .env("BUZZ_ACP_MCP_COMMAND", "")
        .env("BUZZ_ACP_RESPOND_TO", "allowlist")
        .env("BUZZ_ACP_RESPOND_TO_ALLOWLIST", authors)
        .env("BUZZ_ACP_CHANNELS", channels)
        .env("BUZZ_ACP_MULTIPLE_EVENT_HANDLING", "queue")
        .env("BUZZ_ACP_SESSION_POLICY", "channel")
        .env("BUZZ_ACP_HEARTBEAT_INTERVAL", "0")
        .env("BUZZ_ACP_NO_MEMORY", "true")
        .env("BUZZ_ACP_IDLE_TIMEOUT", (timeout + 30).to_string())
        .env("BUZZ_ACP_MAX_TURN_DURATION", (timeout + 60).to_string());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = command.exec();
        Err(Error::State("Could not start the bundled Buzz runner"))
    }
    #[cfg(not(unix))]
    Err(Error::Config("The sidecar launcher requires Unix".into()))
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let result = match arguments.as_slice() {
        [] => launch(),
        #[cfg(unix)]
        [action] if action == "prepare" => prepare(
            Path::new("/projected-secrets"),
            Path::new("/private-secrets"),
            Path::new("/state-volume"),
        ),
        _ => Err(Error::Config(
            "Use no arguments to launch, or 'prepare' for Kubernetes initialization".into(),
        )),
    };
    if let Err(error) = result {
        eprintln!("Sidecar startup: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for name in ["projected", "private", "persistent"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::create_dir(root.path().join("projected/..version")).unwrap();
        symlink("..version", root.path().join("projected/..data")).unwrap();
        for name in ["openwebui-key", "buzz-key"] {
            let file = root.path().join("projected/..version").join(name);
            fs::write(&file, "synthetic-credential\n").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o440)).unwrap();
            symlink(
                format!("..data/{name}"),
                root.path().join("projected").join(name),
            )
            .unwrap();
        }
        root
    }

    fn run(root: &Path) -> Result<()> {
        prepare(
            &root.join("projected"),
            &root.join("private"),
            &root.join("persistent"),
        )
    }

    #[test]
    fn projected_keys_become_private_regular_files_and_restart_preserves_state() {
        let root = fixture();
        run(root.path()).unwrap();
        let state = root.path().join("persistent/state");
        fs::write(state.join("checkpoint"), "keep-me").unwrap();
        run(root.path()).unwrap();
        assert_eq!(
            fs::read_to_string(state.join("checkpoint")).unwrap(),
            "keep-me"
        );
        assert_eq!(state.metadata().unwrap().mode() & 0o777, 0o700);
        for name in ["openwebui-key", "buzz-key"] {
            let file = root.path().join("private/keys").join(name);
            assert!(!file.symlink_metadata().unwrap().is_symlink());
            assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
            assert_eq!(
                secure_file::read_secret(&file).unwrap(),
                "synthetic-credential\n"
            );
        }
    }

    #[test]
    fn projected_links_must_stay_inside_mount() {
        let root = fixture();
        let key = root.path().join("projected/openwebui-key");
        fs::remove_file(&key).unwrap();
        fs::write(root.path().join("outside"), "private-outside-marker").unwrap();
        symlink("../outside", &key).unwrap();
        let error = run(root.path()).unwrap_err().to_string();
        assert!(!error.contains("private-outside-marker"));
        assert!(!root.path().join("private/keys/openwebui-key").exists());
    }

    #[test]
    fn invalid_projected_credentials_are_rejected() {
        for case in [
            "missing",
            "empty",
            "large",
            "writable",
            "directory",
            "hardlink",
        ] {
            let root = fixture();
            let key = root.path().join("projected/..version/openwebui-key");
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
            match case {
                "missing" => fs::remove_file(&key).unwrap(),
                "empty" => fs::write(&key, " \n").unwrap(),
                "large" => {
                    fs::write(&key, vec![b'x'; secure_file::MAX_SECRET_BYTES as usize + 1]).unwrap()
                }
                "writable" => fs::set_permissions(&key, fs::Permissions::from_mode(0o660)).unwrap(),
                "directory" => {
                    fs::remove_file(&key).unwrap();
                    fs::create_dir(&key).unwrap();
                }
                "hardlink" => fs::hard_link(&key, root.path().join("second-link")).unwrap(),
                _ => unreachable!(),
            }
            assert!(run(root.path()).is_err(), "accepted {case}");
        }
    }

    #[test]
    fn destination_and_state_symlinks_are_rejected() {
        for (volume, child) in [("private", "keys"), ("persistent", "state")] {
            let root = fixture();
            symlink(
                root.path().join("projected"),
                root.path().join(volume).join(child),
            )
            .unwrap();
            assert!(run(root.path()).is_err());
        }
    }
}
