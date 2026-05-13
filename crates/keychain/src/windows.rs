use crate::{ACCOUNT, KEY_LEN, SERVICE};
use qpv2_core::SecureVec;
use std::ptr;
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

fn target_name() -> Vec<u16> {
    format!("{}/{}", SERVICE, ACCOUNT)
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn last_error() -> u32 {
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

fn last_error_message() -> String {
    match last_error() {
        5 => "Access denied.".to_string(),
        87 => "Invalid parameter.".to_string(),
        1168 => "Credential not found.".to_string(),
        1312 => "No such logon session.".to_string(),
        code => format!("Credential Manager error ({}).", code),
    }
}

pub fn store_key(key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_LEN {
        return Err(format!("Expected {KEY_LEN}-byte key, got {}.", key.len()));
    }

    let target = target_name();
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 && last_error() != 1168 {
        return Err(last_error_message());
    }

    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        Comment: ptr::null_mut(),
        LastWritten: windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: KEY_LEN as u32,
        CredentialBlob: key.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: ptr::null_mut(),
    };

    let ok = unsafe { CredWriteW(&cred, 0) };
    if ok == 0 {
        Err(last_error_message())
    } else {
        Ok(())
    }
}

pub fn retrieve_key() -> Result<SecureVec, String> {
    let target = target_name();
    let mut pcred: *mut CREDENTIALW = ptr::null_mut();

    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pcred) };
    if ok == 0 {
        return Err(last_error_message());
    }

    let cred = unsafe { &*pcred };
    let blob_size = cred.CredentialBlobSize as usize;
    if blob_size != KEY_LEN {
        unsafe { CredFree(pcred as *const _) };
        return Err(format!(
            "Credential returned {}-byte key, expected {KEY_LEN}.",
            blob_size
        ));
    }

    let bytes = unsafe { std::slice::from_raw_parts(cred.CredentialBlob, blob_size) }.to_vec();
    unsafe { CredFree(pcred as *const _) };

    Ok(SecureVec::from_vec(bytes))
}

pub fn delete_key() -> Result<(), String> {
    let target = target_name();
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        Err(last_error_message())
    } else {
        Ok(())
    }
}
