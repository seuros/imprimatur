//! The standard security handler (ISO 32000-1, 7.6.3; ISO 32000-2, 7.6.4).
//! Most encrypted PDFs, certificates among them, only restrict printing or
//! copying: their user password is empty, and opening them takes no secret.
//! That is the only password tried; anything else is
//! [`Error::PasswordRequired`].

#[cfg(test)]
mod tests;

use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes::{Aes128, Aes256};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use super::object::{Dict, Object};
use crate::{Error, Result};

/// The 32 bytes that pad a password (Algorithm 2, step a).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Identity,
    Rc4,
    Aes128,
    Aes256,
}

#[derive(Debug)]
pub struct Crypt {
    key: Vec<u8>,
    strings: Method,
    streams: Method,
}

impl Crypt {
    /// Opens the handler with the empty user password.
    ///
    /// # Errors
    /// [`Error::PasswordRequired`] when the user password is not empty, and
    /// [`Error::UnsupportedDocument`] for other security handlers.
    pub fn new(encrypt: &Dict, id: &[u8]) -> Result<Self> {
        if !encrypt.has_name(b"Filter", b"Standard") {
            return Err(Error::UnsupportedDocument(
                "a PDF security handler other than Standard".to_owned(),
            ));
        }
        let number = |key: &[u8]| encrypt.get(key).and_then(Object::as_integer);
        let bytes = |key: &[u8]| encrypt.get(key).and_then(Object::as_bytes).unwrap_or_default();
        let (version, revision) = (number(b"V").unwrap_or(0), number(b"R").unwrap_or(2));
        let (owner, user) = (bytes(b"O"), bytes(b"U"));
        if version >= 5 {
            let key = aes256_key(revision, user, bytes(b"UE"))?;
            let method = |name: &[u8]| filter_method(encrypt, name, Method::Aes256);
            return Ok(Self { key, strings: method(b"StrF"), streams: method(b"StmF") });
        }
        let (strings, streams, length) = if version == 4 {
            let method = |name: &[u8]| filter_method(encrypt, name, Method::Rc4);
            (method(b"StrF"), method(b"StmF"), 16)
        } else {
            let bits = if version == 1 { 40 } else { number(b"Length").unwrap_or(40) };
            (Method::Rc4, Method::Rc4, usize::try_from(bits / 8).unwrap_or(5).clamp(5, 16))
        };
        #[expect(clippy::cast_possible_truncation, reason = "P is a 32-bit signed field")]
        let permissions = number(b"P").unwrap_or(0) as i32;
        let metadata = encrypt.get(b"EncryptMetadata") != Some(&Object::Bool(false));
        let mut hash = Md5::new();
        hash.update(PAD);
        hash.update(&owner[..owner.len().min(32)]);
        hash.update(permissions.to_le_bytes());
        hash.update(id);
        if revision >= 4 && !metadata {
            hash.update([0xFF; 4]);
        }
        let mut key = hash.finalize().to_vec();
        if revision >= 3 {
            for _ in 0..50 {
                key = Md5::digest(&key[..length]).to_vec();
            }
        }
        key.truncate(length);
        let valid = if revision == 2 {
            rc4(&key, &PAD) == user
        } else {
            let mut hash = Md5::new();
            hash.update(PAD);
            hash.update(id);
            let mut check = rc4(&key, &hash.finalize());
            for round in 1..=19_u8 {
                let round_key: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
                check = rc4(&round_key, &check);
            }
            user.get(..16) == Some(&check[..])
        };
        if !valid {
            return Err(Error::PasswordRequired);
        }
        Ok(Self { key, strings, streams })
    }

    /// Decrypts a string or stream of object `number`, generation
    /// `generation`.
    pub fn decrypt(&self, stream: bool, (number, generation): (u32, u16), data: &[u8]) -> Vec<u8> {
        let method = if stream { self.streams } else { self.strings };
        match method {
            Method::Identity => data.to_vec(),
            Method::Aes256 => aes_cbc::<Aes256>(&self.key, data),
            Method::Rc4 | Method::Aes128 => {
                let mut hash = Md5::new();
                hash.update(&self.key);
                hash.update(&number.to_le_bytes()[..3]);
                hash.update(generation.to_le_bytes());
                if method == Method::Aes128 {
                    hash.update(b"sAlT");
                }
                let digest = hash.finalize();
                let key = &digest[..(self.key.len() + 5).min(16)];
                if method == Method::Rc4 { rc4(key, data) } else { aes_cbc::<Aes128>(key, data) }
            }
        }
    }
}

/// A crypt filter's method (`/StmF`, `/StrF` naming an entry of `/CF`).
fn filter_method(encrypt: &Dict, key: &[u8], default: Method) -> Method {
    let Some(name) = encrypt.get(key).and_then(Object::as_name) else {
        return Method::Identity;
    };
    if name == b"Identity" {
        return Method::Identity;
    }
    let filters = encrypt.get(b"CF").and_then(Object::as_dict);
    let method = filters
        .and_then(|filters| filters.get(name))
        .and_then(Object::as_dict)
        .and_then(|filter| filter.get(b"CFM"))
        .and_then(Object::as_name);
    match method {
        Some(b"None") => Method::Identity,
        Some(b"V2") => Method::Rc4,
        Some(b"AESV2") => Method::Aes128,
        Some(b"AESV3") => Method::Aes256,
        _ => default,
    }
}

/// The file key of revisions 5 and 6 (AES-256): check the empty password
/// against `U`, then unwrap `UE` with the key it derives.
fn aes256_key(revision: i64, user: &[u8], wrapped: &[u8]) -> Result<Vec<u8>> {
    let (Some(check), Some(validation_salt), Some(key_salt)) =
        (user.get(..32), user.get(32..40), user.get(40..48))
    else {
        return Err(Error::PasswordRequired);
    };
    let hash = |salt: &[u8]| {
        if revision >= 6 { hardened_hash(salt) } else { Sha256::digest(salt).to_vec() }
    };
    if hash(validation_salt).get(..32) != Some(check) || wrapped.len() < 32 {
        return Err(Error::PasswordRequired);
    }
    let cipher =
        Aes256::new_from_slice(&hash(key_salt)[..32]).map_err(|_| Error::PasswordRequired)?;
    Ok(cbc(&cipher, [0; 16], &wrapped[..32]))
}

/// CBC decryption of whole blocks; a torn last block is dropped.
fn cbc<C>(cipher: &C, iv: [u8; 16], data: &[u8]) -> Vec<u8>
where
    C: BlockCipherDecrypt + aes::cipher::BlockSizeUser<BlockSize = aes::cipher::consts::U16>,
{
    let mut out = Vec::with_capacity(data.len());
    let mut previous = iv;
    for chunk in data.as_chunks::<16>().0 {
        let mut block = Array::from(*chunk);
        cipher.decrypt_block(&mut block);
        out.extend(block.iter().zip(previous).map(|(byte, iv)| byte ^ iv));
        previous = *chunk;
    }
    out
}

/// Algorithm 2.B (ISO 32000-2, 7.6.4.3.4) for the empty password and no
/// user data: rounds of AES-128-CBC and SHA-2 until the last byte allows.
fn hardened_hash(salt: &[u8]) -> Vec<u8> {
    let mut key = Sha256::digest(salt).to_vec();
    let mut round = 0_u32;
    loop {
        let repeated: Vec<u8> = key.iter().copied().cycle().take(key.len() * 64).collect();
        let Ok(cipher) = Aes128::new_from_slice(&key[..16]) else { return key };
        let mut encrypted = Vec::with_capacity(repeated.len());
        let mut previous: [u8; 16] = key[16..32].try_into().unwrap_or_default();
        for chunk in repeated.as_chunks::<16>().0 {
            let mut block =
                Array::from(std::array::from_fn::<u8, 16, _>(|at| chunk[at] ^ previous[at]));
            cipher.encrypt_block(&mut block);
            previous.copy_from_slice(&block);
            encrypted.extend_from_slice(&block);
        }
        let remainder = encrypted[..16].iter().map(|&byte| u32::from(byte)).sum::<u32>() % 3;
        key = match remainder {
            0 => Sha256::digest(&encrypted).to_vec(),
            1 => Sha384::digest(&encrypted).to_vec(),
            _ => Sha512::digest(&encrypted).to_vec(),
        };
        round += 1;
        if round >= 64 && encrypted.last().is_some_and(|&last| u32::from(last) + 32 <= round) {
            key.truncate(32);
            return key;
        }
    }
}

/// AES-CBC with the IV in the first block and PKCS #7 padding, as PDF
/// writes it. A torn last block is dropped rather than failing the read.
fn aes_cbc<C>(key: &[u8], data: &[u8]) -> Vec<u8>
where
    C: BlockCipherDecrypt
        + KeyInit
        + aes::cipher::BlockSizeUser<BlockSize = aes::cipher::consts::U16>,
{
    let (Some((iv, data)), Ok(cipher)) = (data.split_first_chunk::<16>(), C::new_from_slice(key))
    else {
        return Vec::new();
    };
    let mut out = cbc(&cipher, *iv, data);
    if let Some(&padding) = out.last()
        && (1..=16).contains(&padding)
        && out.len() >= usize::from(padding)
        && out[out.len() - usize::from(padding)..].iter().all(|&byte| byte == padding)
    {
        out.truncate(out.len() - usize::from(padding));
    }
    out
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    #[expect(clippy::cast_possible_truncation, reason = "indices of a 256-entry table")]
    let mut state: [u8; 256] = std::array::from_fn(|index| index as u8);
    let mut j = 0_u8;
    for i in 0..256 {
        j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len().max(1)]);
        state.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0_u8, 0_u8);
    data.iter()
        .map(|&byte| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(state[usize::from(i)]);
            state.swap(usize::from(i), usize::from(j));
            byte ^ state[usize::from(state[usize::from(i)].wrapping_add(state[usize::from(j)]))]
        })
        .collect()
}
