use rand::{OsRng, Rng};
use parity_crypto as crypto;
use crypto::Keccak256;
use smallvec::SmallVec;
use std::num::NonZeroU32;
use std::convert::{TryInto, TryFrom};
use serde::{Deserialize, Serialize};
use super::error::{WalletError, Result};
use crate::derive::{ExtendedSpendingKey, Derivation};

/// Serializable and deserializable bytes
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct SerdeBytes(
    #[serde(with = "serde_bytes")]
    pub Vec<u8>
);

impl From<Vec<u8>> for SerdeBytes {
    fn from(v: Vec<u8>) -> Self {
        SerdeBytes(v)
    }
}

impl From<SmallVec<[u8; 32]>> for SerdeBytes {
    fn from(v: SmallVec<[u8; 32]>) -> Self {
        SerdeBytes(v.into_vec())
    }
}

impl From<[u8; 32]> for SerdeBytes {
    fn from(v: [u8; 32]) -> Self {
        SerdeBytes(v.to_vec())
    }
}

impl From<[u8; 16]> for SerdeBytes {
    fn from(v: [u8; 16]) -> Self {
        SerdeBytes(v.to_vec())
    }
}

impl From<&[u8]> for SerdeBytes {
    fn from(v: &[u8]) -> Self {
        SerdeBytes(v.to_vec())
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyFile {
    // /// Keyfile ID
    // pub id: [u8; 16],

    /// Unique Keyfile name which is used for filename
    pub keyfile_name: String,

    /// User defined account name
    pub account_name: String,

    /// Keyfile version
    pub version: u32,

    /// Encrypted private key
    pub enc_key: KeyCiphertext,

    // /// Optional address
    // pub address: Option<SerdeBytes>,
}

impl KeyFile {
    pub fn new(
        keyfile_name: String,
        account_name: String,
        version: u32,
        password: &[u8],
        iters: u32,
        xsk: &ExtendedSpendingKey
    ) -> Result<Self> {
        let enc_key = KeyCiphertext::encrypt(xsk, password, iters)?;

        Ok(KeyFile {
            keyfile_name,
            account_name,
            version,
            enc_key,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCiphertext {
    pub ciphertext: SerdeBytes,
    pub mac: SerdeBytes,
    pub salt: SerdeBytes,
    pub iv: SerdeBytes,
    pub iters: u32,
}

impl KeyCiphertext {
    /// Encrypt plain bytes data
    /// Currently using `parity-crypto`.
    pub fn encrypt(
        xsk: &ExtendedSpendingKey,
        password: &[u8],
        iters: u32,
    ) -> Result<Self>
    {
        let rng = &mut OsRng::new().expect("should be able to construct RNG");
        let salt: [u8; 32] = rng.gen();
        let iv: [u8; 16] = rng.gen();

        let (derived_left, derived_right) = crypto::derive_key_iterations(password, &salt, iters);

        let xsk_bytes: Vec<u8> = xsk.try_into()?;

        let mut ciphertext: SmallVec<[u8; 32]> = SmallVec::from_vec(vec![0; xsk_bytes.len()]);

        crypto::aes::encrypt_128_ctr(&derived_left, &iv, &xsk_bytes[..], &mut *ciphertext).map_err(crypto::Error::from)?;

        let mac = crypto::derive_mac(&derived_right, &*ciphertext).keccak256();

        Ok(KeyCiphertext {
            ciphertext: ciphertext.into(),
            mac: mac.into(),
            salt: salt.into(),
            iv: iv.into(),
            iters,
        })
    }

    pub fn decrypt(&self, password: &[u8]) -> Result<ExtendedSpendingKey> {
        let (derived_left, derived_right) = crypto::derive_key_iterations(password, &self.salt.0[..], self.iters);
        let mac = crypto::derive_mac(&derived_right, &self.ciphertext.0).keccak256();

        if !crypto::is_equal(&mac, &self.mac.0) {
            return Err(WalletError::InvalidPassword)
        }

        let mut plain: SmallVec<[u8; 32]> = SmallVec::from_vec(vec![0; self.ciphertext.0.len()]);

        crypto::aes::decrypt_128_ctr(&derived_left, &self.iv.0, &self.ciphertext.0, &mut plain)
            .map_err(crypto::Error::from)?;

        let xsk = ExtendedSpendingKey::read(&mut &plain.to_vec()[..])?;
        
        Ok(xsk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_with_correct_password() {
        let plain = b"test";
        let password = b"abcd";
        let iters = 1024;

        let ciphertext = KeyCiphertext::encrypt(plain, password, iters).unwrap();
        let decrypted = ciphertext.decrypt(password).unwrap();

        assert_eq!(decrypted, plain);
    }

    #[test]
    fn test_plain_with_wrong_password() {
        let plain = b"test";
        let password_enc = b"abcd";
        let password_dec = b"wxyz";
        let iters = 1024;

        let ciphertext = KeyCiphertext::encrypt(plain, password_enc, iters).unwrap();
        let decrypted = ciphertext.decrypt(password_dec);

        assert_matches!(decrypted, Err(WalletError::InvalidPassword));
    }
}
