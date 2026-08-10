//! Profile-level key management, encrypted objects, and portable archives.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

const KEYRING_SERVICE: &str = "org.maicenta.desktop.profile";
const PROFILE_FORMAT: &str = "maicenta-profile";
const PROFILE_VERSION: u32 = 1;
const OBJECT_MAGIC: &[u8; 8] = b"MAIOBJ01";
const ARCHIVE_MAGIC: &[u8; 16] = b"MAICENTA-VAULT01";
const ARCHIVE_AAD: &[u8] = b"maicenta-profile-archive-v1";
const OBJECT_AAD_PREFIX: &[u8] = b"maicenta-profile-object-v1\0";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const MAX_ARCHIVE_HEADER_BYTES: usize = 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_PROFILE_OBJECT_BYTES: u64 = 128 * 1024 * 1024;
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;

/// Errors produced while unlocking or moving an encrypted profile.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("profile storage failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile metadata is invalid: {0}")]
    InvalidProfile(String),
    #[error("operating-system profile key store failed: {0}")]
    KeyStore(String),
    #[error("profile encryption failed: {0}")]
    Crypto(String),
    #[error("profile export password is invalid")]
    InvalidPassword,
}

/// Random 256-bit key protecting one logical profile.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ProfileKey([u8; KEY_BYTES]);

impl ProfileKey {
    /// Constructs a profile key from exact key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw key for an encryption adapter.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

/// An unlocked local profile whose key remains in process memory.
#[derive(Clone)]
pub struct ProfileVault {
    profile_id: String,
    key: ProfileKey,
}

impl ProfileVault {
    /// Loads or creates the small OS-protected key for a database profile.
    ///
    /// Existing encrypted databases are never assigned a replacement key when
    /// the original keychain item is missing.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid metadata, inaccessible profile files, or a
    /// missing or rejected OS credential-store item.
    pub fn open(database_path: &Path) -> Result<Self, VaultError> {
        let manifest_path = profile_manifest_path(database_path);
        let manifest = if manifest_path.exists() {
            read_local_manifest(&manifest_path)?
        } else {
            let manifest = LocalProfileManifest {
                format: PROFILE_FORMAT.into(),
                version: PROFILE_VERSION,
                profile_id: random_identifier()?,
            };
            write_local_manifest(&manifest_path, &manifest)?;
            manifest
        };
        validate_local_manifest(&manifest)?;

        let entry = profile_key_entry(&manifest.profile_id)?;
        let key = match entry.get_password() {
            Ok(encoded) => decode_profile_key(&encoded)?,
            Err(keyring::Error::NoEntry) if may_create_profile_key(database_path)? => {
                let key = random_profile_key()?;
                entry
                    .set_password(&BASE64.encode(key.as_bytes()))
                    .map_err(key_store_error)?;
                key
            }
            Err(keyring::Error::NoEntry) => {
                return Err(VaultError::KeyStore(
                    "the profile key is missing; import a protected backup or restore the keychain item"
                        .into(),
                ));
            }
            Err(error) => return Err(key_store_error(error)),
        };

        Ok(Self {
            profile_id: manifest.profile_id,
            key,
        })
    }

    /// Builds an isolated test vault without accessing the OS credential store.
    #[must_use]
    pub fn for_test(profile_id: impl Into<String>, key: [u8; KEY_BYTES]) -> Self {
        Self {
            profile_id: profile_id.into(),
            key: ProfileKey::from_bytes(key),
        }
    }

    /// Returns the stable public identifier of this profile.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// Borrows the database encryption key.
    #[must_use]
    pub const fn key(&self) -> &ProfileKey {
        &self.key
    }

    /// Stores this profile key in the native credential store and writes its
    /// non-secret local manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when either the OS credential store or local manifest
    /// cannot be updated.
    pub fn install(&self, database_path: &Path) -> Result<(), VaultError> {
        profile_key_entry(&self.profile_id)?
            .set_password(&BASE64.encode(self.key.as_bytes()))
            .map_err(key_store_error)?;
        write_local_manifest(
            &profile_manifest_path(database_path),
            &LocalProfileManifest {
                format: PROFILE_FORMAT.into(),
                version: PROFILE_VERSION,
                profile_id: self.profile_id.clone(),
            },
        )
    }

    /// Removes this profile's OS-protected key. Missing entries are accepted.
    ///
    /// # Errors
    ///
    /// Returns an error when the OS credential store rejects the operation.
    pub fn remove_from_key_store(&self) -> Result<(), VaultError> {
        match profile_key_entry(&self.profile_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(key_store_error(error)),
        }
    }

    /// Encrypts and atomically writes one profile object.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe paths, unavailable randomness, encryption
    /// failures, or filesystem failures.
    pub fn write_object(
        &self,
        path: &Path,
        object_key: &str,
        plaintext: &[u8],
    ) -> Result<(), VaultError> {
        validate_object_key(object_key)?;
        let nonce = random_bytes::<NONCE_BYTES>()?;
        let cipher = object_cipher(&self.key, object_key)?;
        let aad = object_aad(object_key);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::Crypto("object encryption failed".into()))?;
        let mut encoded = Vec::with_capacity(OBJECT_MAGIC.len() + nonce.len() + ciphertext.len());
        encoded.extend_from_slice(OBJECT_MAGIC);
        encoded.extend_from_slice(&nonce);
        encoded.extend_from_slice(&ciphertext);
        atomic_write(path, &encoded)
    }

    /// Reads and authenticates one encrypted profile object.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata is unsafe, the object exceeds its bound,
    /// filesystem access fails, or authentication fails.
    pub fn read_object(
        &self,
        path: &Path,
        object_key: &str,
        maximum_plaintext_bytes: u64,
    ) -> Result<Vec<u8>, VaultError> {
        validate_object_key(object_key)?;
        let overhead = u64::try_from(OBJECT_MAGIC.len() + NONCE_BYTES + TAG_BYTES)
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file()
            || metadata.len() > maximum_plaintext_bytes.saturating_add(overhead)
        {
            return Err(VaultError::InvalidProfile(
                "encrypted object size is invalid".into(),
            ));
        }
        let encoded = fs::read(path)?;
        decrypt_object_bytes(&self.key, object_key, &encoded, maximum_plaintext_bytes)
    }

    /// Converts any legacy plaintext object below a profile object directory.
    ///
    /// # Errors
    ///
    /// Returns an error for symbolic links, unsafe object names, failed reads,
    /// or failed atomic encryption.
    pub fn migrate_plaintext_objects(&self, object_root: &Path) -> Result<usize, VaultError> {
        if !object_root.exists() {
            return Ok(0);
        }
        let mut files = Vec::new();
        collect_regular_files(object_root, object_root, &mut files)?;
        let mut migrated = 0;
        for (path, object_key) in files {
            let mut prefix = [0_u8; OBJECT_MAGIC.len()];
            let mut input = File::open(&path)?;
            let read = input.read(&mut prefix)?;
            if read == OBJECT_MAGIC.len() && prefix == *OBJECT_MAGIC {
                continue;
            }
            let plaintext = fs::read(&path)?;
            self.write_object(&path, &object_key, &plaintext)?;
            migrated += 1;
        }
        Ok(migrated)
    }

    /// Authenticates every encrypted object below a profile object root.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe paths, symbolic links, oversized objects,
    /// failed reads, or failed object authentication.
    pub fn validate_objects(&self, object_root: &Path) -> Result<usize, VaultError> {
        if !object_root.exists() {
            return Ok(0);
        }
        let mut files = Vec::new();
        collect_regular_files(object_root, object_root, &mut files)?;
        for (path, object_key) in &files {
            self.read_object(path, object_key, MAX_PROFILE_OBJECT_BYTES)?;
        }
        Ok(files.len())
    }

    /// Creates a password-protected portable archive containing the encrypted
    /// database and encrypted object files.
    ///
    /// # Errors
    ///
    /// Returns an error for a weak password, unsafe profile objects,
    /// encryption failures, or destination I/O failures.
    pub fn export_archive(
        &self,
        database_path: &Path,
        object_root: &Path,
        destination: &Path,
        password: &str,
    ) -> Result<(), VaultError> {
        validate_export_password(password)?;
        self.migrate_plaintext_objects(object_root)?;
        let mut entries = vec![archive_entry(database_path, "profile.sqlite")?];
        if object_root.exists() {
            let mut objects = Vec::new();
            collect_regular_files(object_root, object_root, &mut objects)?;
            objects.sort_by(|left, right| left.1.cmp(&right.1));
            for (path, relative) in objects {
                entries.push(archive_entry(&path, &format!("objects/{relative}"))?);
            }
        }
        if entries.len() > MAX_ARCHIVE_ENTRIES {
            return Err(VaultError::InvalidProfile(
                "profile contains too many archive objects".into(),
            ));
        }

        let manifest = ArchiveManifest {
            format: PROFILE_FORMAT.into(),
            version: PROFILE_VERSION,
            profile_id: self.profile_id.clone(),
            created_at_ms: current_timestamp_ms()?,
            entries: entries
                .iter()
                .map(|entry| ArchiveManifestEntry {
                    path: entry.archive_path.clone(),
                    size: entry.size,
                })
                .collect(),
        };
        let manifest_json = serde_json::to_vec(&manifest)
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        let manifest_nonce = random_bytes::<NONCE_BYTES>()?;
        let manifest_cipher = XChaCha20Poly1305::new_from_slice(self.key.as_bytes())
            .map_err(|_| VaultError::Crypto("invalid profile key".into()))?;
        let encrypted_manifest = manifest_cipher
            .encrypt(
                XNonce::from_slice(&manifest_nonce),
                Payload {
                    msg: &manifest_json,
                    aad: ARCHIVE_AAD,
                },
            )
            .map_err(|_| VaultError::Crypto("archive manifest encryption failed".into()))?;

        let salt = random_bytes::<16>()?;
        let mut password_key = derive_password_key(password, &salt)?;
        let key_nonce = random_bytes::<NONCE_BYTES>()?;
        let key_cipher = XChaCha20Poly1305::new_from_slice(&password_key)
            .map_err(|_| VaultError::Crypto("invalid export key".into()))?;
        let wrapped_profile_key = key_cipher
            .encrypt(
                XNonce::from_slice(&key_nonce),
                Payload {
                    msg: self.key.as_bytes(),
                    aad: ARCHIVE_AAD,
                },
            )
            .map_err(|_| VaultError::Crypto("profile key wrapping failed".into()))?;
        password_key.zeroize();

        let header = ArchiveHeader {
            format: PROFILE_FORMAT.into(),
            version: PROFILE_VERSION,
            kdf: "argon2id".into(),
            memory_kib: ARGON2_MEMORY_KIB,
            iterations: ARGON2_ITERATIONS,
            parallelism: ARGON2_PARALLELISM,
            salt: BASE64.encode(salt),
            key_nonce: BASE64.encode(key_nonce),
            wrapped_profile_key: BASE64.encode(wrapped_profile_key),
            manifest_nonce: BASE64.encode(manifest_nonce),
            encrypted_manifest: BASE64.encode(encrypted_manifest),
        };
        let header_json = serde_json::to_vec(&header)
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        if header_json.len() > MAX_ARCHIVE_HEADER_BYTES {
            return Err(VaultError::InvalidProfile(
                "archive header exceeds its limit".into(),
            ));
        }

        let temporary = temporary_sibling(destination, "export")?;
        let result = (|| -> Result<(), VaultError> {
            let mut output = secure_create_new(&temporary)?;
            output.write_all(ARCHIVE_MAGIC)?;
            output.write_all(
                &u32::try_from(header_json.len())
                    .map_err(|error| VaultError::InvalidProfile(error.to_string()))?
                    .to_le_bytes(),
            )?;
            output.write_all(&header_json)?;
            for entry in &entries {
                let mut input = File::open(&entry.source_path)?;
                std::io::copy(&mut input, &mut output)?;
            }
            output.sync_all()?;
            replace_file(&temporary, destination)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// Decrypted archive staged outside the active profile.
pub struct StagedProfile {
    pub vault: ProfileVault,
    pub database_path: PathBuf,
    pub object_root: PathBuf,
}

/// Extracts and authenticates a portable profile into an empty staging folder.
///
/// # Errors
///
/// Returns an error for an invalid password, modified or unsupported archive,
/// unsafe paths, excessive metadata, or staging filesystem failures.
#[allow(clippy::too_many_lines)]
pub fn stage_archive(
    source: &Path,
    staging_root: &Path,
    password: &str,
) -> Result<StagedProfile, VaultError> {
    validate_export_password(password)?;
    if staging_root.exists() {
        return Err(VaultError::InvalidProfile(
            "profile staging directory already exists".into(),
        ));
    }
    fs::create_dir_all(staging_root)?;
    let result = (|| -> Result<StagedProfile, VaultError> {
        let mut input = File::open(source)?;
        let mut magic = [0_u8; ARCHIVE_MAGIC.len()];
        input.read_exact(&mut magic)?;
        if magic != *ARCHIVE_MAGIC {
            return Err(VaultError::InvalidProfile(
                "file is not a MAICENTA profile archive".into(),
            ));
        }
        let mut length = [0_u8; 4];
        input.read_exact(&mut length)?;
        let header_length = usize::try_from(u32::from_le_bytes(length))
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        if header_length == 0 || header_length > MAX_ARCHIVE_HEADER_BYTES {
            return Err(VaultError::InvalidProfile(
                "archive header length is invalid".into(),
            ));
        }
        let mut header_json = vec![0_u8; header_length];
        input.read_exact(&mut header_json)?;
        let header: ArchiveHeader = serde_json::from_slice(&header_json)
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        validate_archive_header(&header)?;

        let salt = decode_array::<16>(&header.salt, "archive salt")?;
        let key_nonce = decode_array::<NONCE_BYTES>(&header.key_nonce, "key nonce")?;
        let mut password_key = derive_password_key_with_params(
            password,
            &salt,
            header.memory_kib,
            header.iterations,
            header.parallelism,
        )?;
        let key_cipher = XChaCha20Poly1305::new_from_slice(&password_key)
            .map_err(|_| VaultError::Crypto("invalid export key".into()))?;
        let unwrapped = key_cipher.decrypt(
            XNonce::from_slice(&key_nonce),
            Payload {
                msg: &BASE64
                    .decode(&header.wrapped_profile_key)
                    .map_err(|_| VaultError::InvalidProfile("wrapped key is invalid".into()))?,
                aad: ARCHIVE_AAD,
            },
        );
        password_key.zeroize();
        let mut unwrapped = unwrapped.map_err(|_| VaultError::InvalidPassword)?;
        if unwrapped.len() != KEY_BYTES {
            unwrapped.zeroize();
            return Err(VaultError::InvalidProfile(
                "unwrapped profile key has an invalid length".into(),
            ));
        }
        let mut key_bytes = [0_u8; KEY_BYTES];
        key_bytes.copy_from_slice(&unwrapped);
        unwrapped.zeroize();
        let key = ProfileKey::from_bytes(key_bytes);

        let manifest_nonce = decode_array::<NONCE_BYTES>(&header.manifest_nonce, "manifest nonce")?;
        let encrypted_manifest = BASE64
            .decode(&header.encrypted_manifest)
            .map_err(|_| VaultError::InvalidProfile("encrypted manifest is invalid".into()))?;
        let manifest_cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
            .map_err(|_| VaultError::Crypto("invalid profile key".into()))?;
        let manifest_json = manifest_cipher
            .decrypt(
                XNonce::from_slice(&manifest_nonce),
                Payload {
                    msg: &encrypted_manifest,
                    aad: ARCHIVE_AAD,
                },
            )
            .map_err(|_| VaultError::InvalidProfile("archive manifest was modified".into()))?;
        let manifest: ArchiveManifest = serde_json::from_slice(&manifest_json)
            .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
        validate_archive_manifest(&manifest)?;

        for entry in &manifest.entries {
            let relative = validate_archive_path(&entry.path)?;
            let destination = staging_root.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = secure_create_new(&destination)?;
            let mut limited = (&mut input).take(entry.size);
            let copied = std::io::copy(&mut limited, &mut output)?;
            if copied != entry.size {
                return Err(VaultError::InvalidProfile(
                    "archive ended before all profile data was restored".into(),
                ));
            }
            output.sync_all()?;
        }
        let mut trailing = [0_u8; 1];
        if input.read(&mut trailing)? != 0 {
            return Err(VaultError::InvalidProfile(
                "archive contains unexpected trailing data".into(),
            ));
        }

        let database_path = staging_root.join("profile.sqlite");
        if !database_path.is_file() {
            return Err(VaultError::InvalidProfile(
                "archive does not contain a profile database".into(),
            ));
        }
        let vault = ProfileVault {
            profile_id: manifest.profile_id,
            key,
        };
        let object_root = staging_root.join("objects");
        vault.validate_objects(&object_root)?;
        Ok(StagedProfile {
            vault,
            database_path,
            object_root,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging_root);
    }
    result
}

/// Returns the non-secret sidecar manifest path for a database.
#[must_use]
pub fn profile_manifest_path(database_path: &Path) -> PathBuf {
    database_path.with_extension("profile.json")
}

fn may_create_profile_key(database_path: &Path) -> Result<bool, VaultError> {
    if !database_path.exists() || database_path.metadata()?.len() == 0 {
        return Ok(true);
    }
    let mut header = [0_u8; 16];
    File::open(database_path)?.read_exact(&mut header)?;
    Ok(&header == b"SQLite format 3\0")
}

fn random_profile_key() -> Result<ProfileKey, VaultError> {
    Ok(ProfileKey(random_bytes::<KEY_BYTES>()?))
}

fn random_bytes<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|error| VaultError::Crypto(error.to_string()))?;
    Ok(bytes)
}

fn random_identifier() -> Result<String, VaultError> {
    use std::fmt::Write as _;

    let bytes = random_bytes::<16>()?;
    Ok(bytes
        .iter()
        .fold(String::with_capacity(32), |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        }))
}

fn decode_profile_key(encoded: &str) -> Result<ProfileKey, VaultError> {
    let mut decoded = BASE64
        .decode(encoded)
        .map_err(|_| VaultError::KeyStore("stored profile key is corrupt".into()))?;
    if decoded.len() != KEY_BYTES {
        decoded.zeroize();
        return Err(VaultError::KeyStore(
            "stored profile key has an invalid length".into(),
        ));
    }
    let mut key = [0_u8; KEY_BYTES];
    key.copy_from_slice(&decoded);
    decoded.zeroize();
    Ok(ProfileKey(key))
}

fn profile_key_entry(profile_id: &str) -> Result<Entry, VaultError> {
    Entry::new(KEYRING_SERVICE, profile_id).map_err(key_store_error)
}

#[allow(clippy::needless_pass_by_value)]
fn key_store_error(error: keyring::Error) -> VaultError {
    VaultError::KeyStore(error.to_string())
}

fn object_cipher(key: &ProfileKey, object_key: &str) -> Result<XChaCha20Poly1305, VaultError> {
    let hkdf = Hkdf::<Sha256>::new(Some(OBJECT_AAD_PREFIX), key.as_bytes());
    let mut derived = [0_u8; KEY_BYTES];
    hkdf.expand(object_key.as_bytes(), &mut derived)
        .map_err(|_| VaultError::Crypto("object key derivation failed".into()))?;
    let cipher = XChaCha20Poly1305::new_from_slice(&derived)
        .map_err(|_| VaultError::Crypto("invalid object key".into()))?;
    derived.zeroize();
    Ok(cipher)
}

fn object_aad(object_key: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(OBJECT_AAD_PREFIX.len() + object_key.len());
    aad.extend_from_slice(OBJECT_AAD_PREFIX);
    aad.extend_from_slice(object_key.as_bytes());
    aad
}

fn decrypt_object_bytes(
    key: &ProfileKey,
    object_key: &str,
    encoded: &[u8],
    maximum_plaintext_bytes: u64,
) -> Result<Vec<u8>, VaultError> {
    if encoded.len() < OBJECT_MAGIC.len() + NONCE_BYTES + TAG_BYTES
        || &encoded[..OBJECT_MAGIC.len()] != OBJECT_MAGIC
    {
        return Err(VaultError::InvalidProfile(
            "object is not encrypted with the supported format".into(),
        ));
    }
    let nonce_start = OBJECT_MAGIC.len();
    let ciphertext_start = nonce_start + NONCE_BYTES;
    let cipher = object_cipher(key, object_key)?;
    let aad = object_aad(object_key);
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&encoded[nonce_start..ciphertext_start]),
            Payload {
                msg: &encoded[ciphertext_start..],
                aad: &aad,
            },
        )
        .map_err(|_| VaultError::InvalidProfile("object authentication failed".into()))?;
    if u64::try_from(plaintext.len())
        .map_err(|error| VaultError::InvalidProfile(error.to_string()))?
        > maximum_plaintext_bytes
    {
        return Err(VaultError::InvalidProfile(
            "decrypted object exceeds its size limit".into(),
        ));
    }
    Ok(plaintext)
}

fn validate_object_key(object_key: &str) -> Result<(), VaultError> {
    let path = Path::new(object_key);
    if path.is_absolute()
        || object_key.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VaultError::InvalidProfile(
            "profile object key is unsafe".into(),
        ));
    }
    Ok(())
}

fn collect_regular_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<(), VaultError> {
    for item in fs::read_dir(directory)? {
        let item = item?;
        let path = item.path();
        let file_type = item.file_type()?;
        if file_type.is_symlink() {
            return Err(VaultError::InvalidProfile(format!(
                "profile object {} is a symbolic link",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_regular_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
            let key = relative
                .to_str()
                .ok_or_else(|| VaultError::InvalidProfile("object path is not UTF-8".into()))?
                .replace('\\', "/");
            validate_object_key(&key)?;
            files.push((path, key));
        } else {
            return Err(VaultError::InvalidProfile(format!(
                "profile object {} is not a regular file",
                path.display()
            )));
        }
    }
    Ok(())
}

fn derive_password_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_BYTES], VaultError> {
    derive_password_key_with_params(
        password,
        salt,
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
    )
}

fn derive_password_key_with_params(
    password: &str,
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<[u8; KEY_BYTES], VaultError> {
    if !(16 * 1024..=1024 * 1024).contains(&memory_kib)
        || iterations == 0
        || iterations > 10
        || parallelism == 0
        || parallelism > 16
    {
        return Err(VaultError::InvalidProfile(
            "archive key-derivation parameters are unsafe".into(),
        ));
    }
    let params = Params::new(memory_kib, iterations, parallelism, Some(KEY_BYTES))
        .map_err(|error| VaultError::Crypto(error.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0_u8; KEY_BYTES];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut output)
        .map_err(|error| VaultError::Crypto(error.to_string()))?;
    Ok(output)
}

fn validate_export_password(password: &str) -> Result<(), VaultError> {
    if password.chars().count() < 12 {
        return Err(VaultError::InvalidProfile(
            "export password must contain at least 12 characters".into(),
        ));
    }
    if password.len() > 1024 {
        return Err(VaultError::InvalidProfile(
            "export password exceeds its size limit".into(),
        ));
    }
    Ok(())
}

fn archive_entry(source_path: &Path, archive_path: &str) -> Result<ArchiveSource, VaultError> {
    validate_archive_path(archive_path)?;
    let metadata = fs::symlink_metadata(source_path)?;
    if !metadata.file_type().is_file() {
        return Err(VaultError::InvalidProfile(format!(
            "archive source {} is not a regular file",
            source_path.display()
        )));
    }
    Ok(ArchiveSource {
        source_path: source_path.to_owned(),
        archive_path: archive_path.into(),
        size: metadata.len(),
    })
}

fn validate_archive_path(path: &str) -> Result<&Path, VaultError> {
    let relative = Path::new(path);
    let valid_root = path == "profile.sqlite" || path.starts_with("objects/");
    if !valid_root
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VaultError::InvalidProfile(
            "archive contains an unsafe entry path".into(),
        ));
    }
    Ok(relative)
}

fn validate_archive_header(header: &ArchiveHeader) -> Result<(), VaultError> {
    if header.format != PROFILE_FORMAT
        || header.version != PROFILE_VERSION
        || header.kdf != "argon2id"
    {
        return Err(VaultError::InvalidProfile(
            "archive format or version is unsupported".into(),
        ));
    }
    Ok(())
}

fn validate_archive_manifest(manifest: &ArchiveManifest) -> Result<(), VaultError> {
    if manifest.format != PROFILE_FORMAT
        || manifest.version != PROFILE_VERSION
        || manifest.profile_id.len() != 32
        || !manifest
            .profile_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.entries.is_empty()
        || manifest.entries.len() > MAX_ARCHIVE_ENTRIES
        || manifest.entries[0].path != "profile.sqlite"
    {
        return Err(VaultError::InvalidProfile(
            "archive manifest is invalid".into(),
        ));
    }
    for entry in &manifest.entries {
        validate_archive_path(&entry.path)?;
    }
    Ok(())
}

fn decode_array<const N: usize>(encoded: &str, name: &str) -> Result<[u8; N], VaultError> {
    let decoded = BASE64
        .decode(encoded)
        .map_err(|_| VaultError::InvalidProfile(format!("{name} is invalid")))?;
    decoded
        .try_into()
        .map_err(|_| VaultError::InvalidProfile(format!("{name} has an invalid length")))
}

fn atomic_write(path: &Path, body: &[u8]) -> Result<(), VaultError> {
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::InvalidProfile("profile path has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_sibling(path, "write")?;
    let result = (|| -> Result<(), VaultError> {
        let mut output = secure_create_new(&temporary)?;
        output.write_all(body)?;
        output.sync_all()?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), VaultError> {
    #[cfg(unix)]
    {
        fs::rename(source, destination).map_err(VaultError::Io)
    }
    #[cfg(not(unix))]
    {
        if !destination.exists() {
            return fs::rename(source, destination).map_err(VaultError::Io);
        }
        let backup = temporary_sibling(destination, "backup")?;
        fs::rename(destination, &backup)?;
        if let Err(error) = fs::rename(source, destination) {
            let _ = fs::rename(&backup, destination);
            return Err(VaultError::Io(error));
        }
        fs::remove_file(backup)?;
        Ok(())
    }
}

fn secure_create_new(path: &Path) -> Result<File, VaultError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(VaultError::Io)
}

fn temporary_sibling(path: &Path, purpose: &str) -> Result<PathBuf, VaultError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| VaultError::InvalidProfile("profile path is not UTF-8".into()))?;
    Ok(path.with_file_name(format!(".{name}.{purpose}.{}", random_identifier()?)))
}

fn current_timestamp_ms() -> Result<i64, VaultError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| VaultError::InvalidProfile(error.to_string()))?
        .as_millis();
    i64::try_from(millis).map_err(|error| VaultError::InvalidProfile(error.to_string()))
}

#[derive(Serialize, Deserialize)]
struct LocalProfileManifest {
    format: String,
    version: u32,
    profile_id: String,
}

fn read_local_manifest(path: &Path) -> Result<LocalProfileManifest, VaultError> {
    let bytes = fs::read(path)?;
    if bytes.len() > 64 * 1024 {
        return Err(VaultError::InvalidProfile(
            "local profile manifest is too large".into(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| VaultError::InvalidProfile(error.to_string()))
}

fn validate_local_manifest(manifest: &LocalProfileManifest) -> Result<(), VaultError> {
    if manifest.format != PROFILE_FORMAT
        || manifest.version != PROFILE_VERSION
        || manifest.profile_id.len() != 32
        || !manifest
            .profile_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(VaultError::InvalidProfile(
            "local profile manifest is invalid or unsupported".into(),
        ));
    }
    Ok(())
}

fn write_local_manifest(path: &Path, manifest: &LocalProfileManifest) -> Result<(), VaultError> {
    let body = serde_json::to_vec_pretty(manifest)
        .map_err(|error| VaultError::InvalidProfile(error.to_string()))?;
    atomic_write(path, &body)
}

struct ArchiveSource {
    source_path: PathBuf,
    archive_path: String,
    size: u64,
}

#[derive(Serialize, Deserialize)]
struct ArchiveHeader {
    format: String,
    version: u32,
    kdf: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
    key_nonce: String,
    wrapped_profile_key: String,
    manifest_nonce: String,
    encrypted_manifest: String,
}

#[derive(Serialize, Deserialize)]
struct ArchiveManifest {
    format: String,
    version: u32,
    profile_id: String,
    created_at_ms: i64,
    entries: Vec<ArchiveManifestEntry>,
}

#[derive(Serialize, Deserialize)]
struct ArchiveManifestEntry {
    path: String,
    size: u64,
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{ProfileVault, stage_archive};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let serial = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "maicenta-vault-{}-{serial}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn encrypts_and_authenticates_profile_objects() {
        let path = temp_path("object.bin");
        let vault = ProfileVault::for_test("0123456789abcdef0123456789abcdef", [7; 32]);
        vault
            .write_object(&path, "attachments/example.bin", b"secret attachment")
            .expect("encrypt object");
        assert_ne!(
            fs::read(&path).expect("encrypted bytes"),
            b"secret attachment"
        );
        assert_eq!(
            vault
                .read_object(&path, "attachments/example.bin", 1024)
                .expect("decrypt object"),
            b"secret attachment"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn exports_and_stages_a_password_protected_profile() {
        let root = temp_path("export-root");
        let database = root.join("maicenta.sqlite");
        let objects = root.join("maicenta.objects");
        let object = objects.join("attachments/example.bin");
        fs::create_dir_all(object.parent().expect("parent")).expect("objects");
        fs::write(&database, b"encrypted database bytes").expect("database");
        fs::write(&object, b"encrypted object bytes").expect("object");
        let archive = temp_path("backup.maicenta-profile");
        let staging = temp_path("staging");
        let vault = ProfileVault::for_test("0123456789abcdef0123456789abcdef", [9; 32]);
        vault
            .export_archive(
                &database,
                &objects,
                &archive,
                "correct horse battery staple",
            )
            .expect("export");
        let imported = stage_archive(&archive, &staging, "correct horse battery staple")
            .expect("stage import");
        assert_eq!(imported.vault.profile_id(), vault.profile_id());
        assert_eq!(
            fs::read(imported.database_path).expect("database"),
            b"encrypted database bytes"
        );
        assert_eq!(
            imported
                .vault
                .read_object(
                    &imported.object_root.join("attachments/example.bin"),
                    "attachments/example.bin",
                    1024,
                )
                .expect("object"),
            b"encrypted object bytes"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(staging);
        let _ = fs::remove_file(archive);
    }

    #[test]
    fn rejects_a_wrong_archive_password() {
        let root = temp_path("wrong-root");
        let database = root.join("maicenta.sqlite");
        fs::create_dir_all(&root).expect("root");
        fs::write(&database, b"encrypted database bytes").expect("database");
        let archive = temp_path("wrong.maicenta-profile");
        let staging = temp_path("wrong-staging");
        let vault = ProfileVault::for_test("0123456789abcdef0123456789abcdef", [3; 32]);
        vault
            .export_archive(
                &database,
                &root.join("objects"),
                &archive,
                "valid password phrase",
            )
            .expect("export");
        assert!(stage_archive(&archive, &staging, "different password").is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(staging);
        let _ = fs::remove_file(archive);
    }
}
