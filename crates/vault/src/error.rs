use std::{error::Error, fmt, io};

/// Errors that stop an encrypted vault from opening or completing an operation.
#[derive(Debug)]
pub enum VaultError {
    AlreadyOpen,
    AlreadyInitialized,
    ArchiveInterrupted,
    CipherUnavailable,
    EntropyUnavailable,
    ExistingVaultWithoutKeyMetadata,
    ExtractionInterrupted,
    LineageInterrupted,
    InvalidKeyOrCorrupt,
    KeyProtectionFailed,
    UnlockFailed,
    UnsupportedKeyMetadata(u16),
    UnsupportedPlatform,
    UnsupportedSchema(i64),
    MigrationInterrupted(i64),
    Io(io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyOpen => formatter.write_str("vault already has an active writer"),
            Self::AlreadyInitialized => formatter.write_str("vault key metadata already exists"),
            Self::ArchiveInterrupted => {
                formatter.write_str("archive database commit was interrupted")
            }
            Self::CipherUnavailable => {
                formatter.write_str("the SQLite binding does not expose SQLCipher")
            }
            Self::EntropyUnavailable => {
                formatter.write_str("operating system random generation failed")
            }
            Self::ExistingVaultWithoutKeyMetadata => {
                formatter.write_str("an existing encrypted vault has no key metadata")
            }
            Self::ExtractionInterrupted => {
                formatter.write_str("extraction database commit was interrupted")
            }
            Self::LineageInterrupted => {
                formatter.write_str("block lineage database commit was interrupted")
            }
            Self::InvalidKeyOrCorrupt => {
                formatter.write_str("vault key is incorrect or encrypted data is corrupt")
            }
            Self::KeyProtectionFailed => {
                formatter.write_str("vault key protection could not be completed")
            }
            Self::UnlockFailed => formatter.write_str("vault key could not be unlocked"),
            Self::UnsupportedKeyMetadata(version) => {
                write!(
                    formatter,
                    "vault key metadata version {version} is not supported"
                )
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("vault key initialization requires Windows")
            }
            Self::UnsupportedSchema(version) => {
                write!(formatter, "vault schema version {version} is not supported")
            }
            Self::MigrationInterrupted(version) => {
                write!(
                    formatter,
                    "vault migration to version {version} was interrupted"
                )
            }
            Self::Io(error) => write!(formatter, "vault I/O error: {error}"),
            Self::Sqlite(error) => write!(formatter, "vault database error: {error}"),
        }
    }
}

impl Error for VaultError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::AlreadyOpen
            | Self::AlreadyInitialized
            | Self::ArchiveInterrupted
            | Self::CipherUnavailable
            | Self::EntropyUnavailable
            | Self::ExistingVaultWithoutKeyMetadata
            | Self::ExtractionInterrupted
            | Self::LineageInterrupted
            | Self::InvalidKeyOrCorrupt
            | Self::KeyProtectionFailed
            | Self::UnlockFailed
            | Self::UnsupportedKeyMetadata(_)
            | Self::UnsupportedPlatform
            | Self::UnsupportedSchema(_)
            | Self::MigrationInterrupted(_) => None,
        }
    }
}

impl From<io::Error> for VaultError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for VaultError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}
