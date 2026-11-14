use crate::{
    Error, Result,
    protocol::{Bytes, Hash, PublicKey, Transaction},
};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub fn hash(bytes: &[u8]) -> Hash {
    Bytes(Sha256::digest(bytes).into())
}
pub fn public_key(key: &SigningKey) -> PublicKey {
    Bytes(key.verifying_key().to_bytes())
}
pub fn valid_key(key: PublicKey) -> Result<VerifyingKey> {
    let key =
        VerifyingKey::from_bytes(&key.0).map_err(|_| Error::Invalid("malformed public key"))?;
    if key.is_weak() {
        return Err(Error::Invalid("weak public key"));
    }
    Ok(key)
}
pub fn sign(tx: &mut Transaction, key: &SigningKey) {
    tx.sender = public_key(key);
    tx.signature = Bytes(key.sign(&tx.signing_bytes()).to_bytes());
}
pub fn verify(tx: &Transaction) -> Result<()> {
    valid_key(tx.sender)?
        .verify_strict(
            &tx.signing_bytes(),
            &ed25519_dalek::Signature::from_bytes(&tx.signature.0),
        )
        .map_err(|_| Error::Invalid("invalid signature"))
}
/// Atomic create_new prevents accidentally replacing an existing identity.
pub fn load_or_create_key(path: &Path) -> Result<SigningKey> {
    if path.exists() {
        return load_key(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).map_err(|_| Error::Invalid("OS randomness unavailable"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(hex::encode(seed).as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return load_key(path),
        Err(error) => return Err(error.into()),
    }
    Ok(SigningKey::from_bytes(&seed))
}
pub fn load_key(path: &Path) -> Result<SigningKey> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            return Err(Error::Invalid(
                "key file must have owner-only permissions (chmod 600)",
            ));
        }
    }
    if fs::metadata(path)?.len() > 65 {
        return Err(Error::Invalid("key file too long"));
    }
    let text = fs::read_to_string(path)?;
    // The seed format permits one trailing LF, not arbitrary whitespace.
    let seed: Bytes<32> = text.strip_suffix('\n').unwrap_or(&text).parse()?;
    Ok(SigningKey::from_bytes(&seed.0))
}
