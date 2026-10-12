//! Synthetic native-runner credentials. No operator configuration is loaded.
use nostr::{Keys, nips::nip19::ToBech32};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};

fn write(path: &Path, value: &str) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("create disposable credential file");
    file.write_all(value.as_bytes()).unwrap();
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".work/native");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)
        .unwrap();
    if root.join("identities.json").exists() {
        for name in [
            "owner-key",
            "agent-key",
            "relay-key",
            "denied-key",
            "auth-tag",
        ] {
            assert!(
                root.join(name).is_file(),
                "Incomplete disposable identity set"
            );
        }
        println!("Reusing the existing disposable identity set.");
        return;
    }
    let owner = Keys::generate();
    let agent = Keys::generate();
    let relay = Keys::generate();
    let denied = Keys::generate();
    let mut public = serde_json::Map::new();
    for (name, keys) in [
        ("owner", &owner),
        ("agent", &agent),
        ("relay", &relay),
        ("denied", &denied),
    ] {
        public.insert(name.into(), keys.public_key().to_hex().into());
        write(
            &root.join(format!("{name}-key")),
            &keys.secret_key().to_bech32().unwrap(),
        );
    }
    write(
        &root.join("auth-tag"),
        &buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap(),
    );
    write(
        &root.join("identities.json"),
        &serde_json::to_string(&public).unwrap(),
    );
    println!("Synthetic nsec identities and owner attestation created; no credentials printed.");
}
