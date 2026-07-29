//! Minimal Windows DPAPI adapter.
//!
//! This is the only module allowed to contain `unsafe` code. Each call keeps
//! the input borrowed, copies the Windows-owned output immediately, scrubs
//! plaintext output before `LocalFree`, and never exposes an OS error oracle.

#![allow(unsafe_code)]

use std::{ffi::c_void, ptr, slice};

use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use zeroize::Zeroizing;

use crate::VaultError;

pub(crate) fn protect_current_user(plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let input = borrowed_blob(plaintext).map_err(|()| VaultError::KeyProtectionFailed)?;
    let mut output = empty_blob();
    // SAFETY: `input` borrows a live slice for the duration of the call; every
    // optional pointer is null; `output` is initialized and owned by DPAPI on
    // success. Omitting CRYPTPROTECT_LOCAL_MACHINE selects CurrentUser scope.
    let succeeded = unsafe {
        CryptProtectData(
            &raw const input,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output,
        )
    };
    if succeeded == 0 {
        drop(LocalAllocation::new(output));
        return Err(VaultError::KeyProtectionFailed);
    }

    let allocation = LocalAllocation::new(output).ok_or(VaultError::KeyProtectionFailed)?;
    let ciphertext = allocation.copy();
    if ciphertext.is_empty() {
        return Err(VaultError::KeyProtectionFailed);
    }
    Ok(ciphertext)
}

pub(crate) fn unprotect_current_user(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let input = borrowed_blob(ciphertext).map_err(|()| VaultError::UnlockFailed)?;
    let mut output = empty_blob();
    // SAFETY: The same pointer and output ownership guarantees as the protect
    // path apply. DPAPI authenticates the blob before returning plaintext.
    let succeeded = unsafe {
        CryptUnprotectData(
            &raw const input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output,
        )
    };
    if succeeded == 0 {
        if let Some(mut allocation) = LocalAllocation::new(output) {
            allocation.mark_secret();
        }
        return Err(VaultError::UnlockFailed);
    }

    let mut allocation = LocalAllocation::new(output).ok_or(VaultError::UnlockFailed)?;
    allocation.mark_secret();
    Ok(Zeroizing::new(allocation.copy()))
}

fn borrowed_blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, ()> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| ())?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

const fn empty_blob() -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    }
}

struct LocalAllocation {
    pointer: *mut u8,
    length: usize,
    scrub_on_drop: bool,
}

impl LocalAllocation {
    fn new(blob: CRYPT_INTEGER_BLOB) -> Option<Self> {
        let length = usize::try_from(blob.cbData).ok()?;
        (!blob.pbData.is_null()).then_some(Self {
            pointer: blob.pbData,
            length,
            scrub_on_drop: false,
        })
    }

    fn mark_secret(&mut self) {
        self.scrub_on_drop = true;
    }

    fn copy(&self) -> Vec<u8> {
        // SAFETY: DPAPI returned a non-null allocation containing `length`
        // initialized bytes, and this guard owns it until drop.
        unsafe { slice::from_raw_parts(self.pointer, self.length) }.to_vec()
    }
}

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if self.scrub_on_drop {
            // SAFETY: The guard still exclusively owns the live allocation.
            unsafe { ptr::write_bytes(self.pointer, 0, self.length) };
        }
        // SAFETY: DPAPI allocates `pbData` with LocalAlloc and requires
        // callers to release it with LocalFree exactly once.
        let result = unsafe { LocalFree(self.pointer.cast::<c_void>()) };
        debug_assert!(result.is_null(), "LocalFree failed for a DPAPI buffer");
    }
}
