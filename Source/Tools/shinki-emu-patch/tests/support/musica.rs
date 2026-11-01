use blowfish::{
    Blowfish,
    cipher::{BlockCipherEncrypt, KeyInit},
};

pub const KEY_HEX: &str = "7075626c69632d666978747572652d6b6579";

fn encrypt(bytes: &mut [u8]) {
    let cipher: Blowfish = Blowfish::new_from_slice(b"public-fixture-key").unwrap();
    for block in bytes.as_chunks_mut::<8>().0 {
        block[..4].reverse();
        block[4..].reverse();
        cipher.encrypt_block((&mut *block).into());
        block[..4].reverse();
        block[4..].reverse();
    }
}

pub fn archive(role: &str, name: &str, payload: &[u8]) -> Vec<u8> {
    archive_entries(role, &[(name, payload)])
}

/// Real version-zero PAZ, independently encrypted with a public test key.
pub fn archive_entries(role: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut index = (entries.len() as u32).to_le_bytes().to_vec();
    if role == "mov" {
        index.extend(0u8..=255);
    }
    let mut offsets = Vec::new();
    for (name, payload) in entries {
        index.extend_from_slice(name.as_bytes());
        index.push(0);
        offsets.push(index.len());
        index.extend_from_slice(&[0; 8]);
        let aligned = payload.len().div_ceil(8) * 8;
        for value in [payload.len(), payload.len(), aligned, 0] {
            index.extend_from_slice(&(value as u32).to_le_bytes());
        }
    }
    index.resize(index.len().div_ceil(8) * 8, 0);
    let mut start = 4 + index.len();
    let mut data = Vec::new();
    for ((_, payload), offset) in entries.iter().zip(offsets) {
        index[offset..offset + 8].copy_from_slice(&(start as u64).to_le_bytes());
        let mut bytes = payload.to_vec();
        bytes.resize(payload.len().div_ceil(8) * 8, 0);
        start += bytes.len();
        if role != "mov" {
            encrypt(&mut bytes);
        }
        data.extend(bytes);
    }
    let size = index.len() as u32;
    encrypt(&mut index);
    let mut bytes = size.to_le_bytes().to_vec();
    bytes.extend(index);
    bytes.extend(data);
    bytes
}

pub fn key_role(role: &str) -> String {
    format!(
        "[archive_keys.{role}]\nindex_key_hex='{KEY_HEX}'\ndata_key_hex='{}'\n",
        if role == "mov" { "" } else { KEY_HEX }
    )
}
