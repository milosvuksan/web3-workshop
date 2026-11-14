use baneworkshop::{crypto, protocol::Bytes};
#[test]
fn persistent_identity_and_private_permissions() {
    let path = std::env::temp_dir().join(format!("pixel-key-test-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let seed = path.join("identity.seed");
    let first = crypto::load_or_create_key(&seed).unwrap();
    let second = crypto::load_or_create_key(&seed).unwrap();
    assert_eq!(crypto::public_key(&first), crypto::public_key(&second));
    assert_ne!(crypto::public_key(&first), Bytes::default());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&seed).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(crypto::load_key(&seed).is_err());
    }
    std::fs::remove_dir_all(path).unwrap();
}
