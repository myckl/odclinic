//! Open Dental password encoding and verification.
//!
//! Ported from `OpenDentBusiness.Authentication` (24.3 branch). A userod
//! password is stored as `HashType$Salt$Hash`; legacy MD5 hashes are stored as
//! a bare 24-character base-64 string. Both SHA3-512 and MD5 hash the
//! **UTF-16LE** bytes of `salt + inputPass` — matching .NET's
//! `Encoding.Unicode` — which is the detail that is easy to get wrong.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use md5::Digest as _;
use md5::Md5;
use sha3::{Keccak512, Sha3_512};
use subtle::ConstantTimeEq;

/// Mirrors `OpenDentBusiness.HashTypes`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashType {
    None,
    Md5,
    Md5Ecw,
    Sha3_512,
}

impl HashType {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "None" => Some(Self::None),
            "MD5" => Some(Self::Md5),
            "MD5_ECW" => Some(Self::Md5Ecw),
            "SHA3_512" => Some(Self::Sha3_512),
            _ => None,
        }
    }

    fn as_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Md5 => "MD5",
            Self::Md5Ecw => "MD5_ECW",
            Self::Sha3_512 => "SHA3_512",
        }
    }
}

/// The decoded contents of the `userod.Password` column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PasswordContainer {
    pub hash_type: HashType,
    pub salt: String,
    pub hash: String,
}

impl PasswordContainer {
    pub fn empty() -> Self {
        Self {
            hash_type: HashType::None,
            salt: String::new(),
            hash: String::new(),
        }
    }

    /// Build a container, forcing `None` when there is no hash at all (the
    /// same rule as the upstream struct constructor).
    pub fn new(hash_type: HashType, salt: String, hash: String) -> Self {
        if hash.is_empty() {
            Self::empty()
        } else {
            Self {
                hash_type,
                salt,
                hash,
            }
        }
    }

    /// `HashType$Salt$Hash`, the format written back to `userod.Password`.
    pub fn encode(&self) -> String {
        format!("{}${}${}", self.hash_type.as_name(), self.salt, self.hash)
    }
}

/// `Authentication.DecodePass`: parse the encoded column value.
pub fn decode(db_value: &str) -> PasswordContainer {
    if db_value.is_empty() {
        return PasswordContainer::empty();
    }
    let parts: Vec<&str> = db_value.split('$').collect();
    if parts.len() == 3 {
        if let Some(kind) = HashType::from_name(parts[0]) {
            return PasswordContainer::new(kind, parts[1].to_string(), parts[2].to_string());
        }
        // An unrecognised hash type is treated as a legacy plaintext/`None`
        // value, keeping the entire string as the hash.
        return PasswordContainer::new(HashType::None, String::new(), db_value.to_string());
    }
    // Bare base-64 MD5 hashes are 24 characters and end in "==".
    if db_value.len() == 24 && db_value.ends_with("==") && !db_value.contains('$') {
        return PasswordContainer::new(HashType::Md5, String::new(), db_value.to_string());
    }
    PasswordContainer::new(HashType::None, String::new(), db_value.to_string())
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn digest_hex_md5_ascii(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest_md5_unicode(text: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(utf16le(text));
    BASE64.encode(hasher.finalize())
}

fn digest_sha3(text: &str, keccak: bool) -> String {
    let bytes = utf16le(text);
    if keccak {
        let mut hasher = Keccak512::new();
        hasher.update(bytes);
        BASE64.encode(hasher.finalize())
    } else {
        let mut hasher = Sha3_512::new();
        hasher.update(bytes);
        BASE64.encode(hasher.finalize())
    }
}

/// `Authentication.GetHash`: the hash for one password and algorithm.
pub fn hash_password(input_pass: &str, salt: &str, hash_type: HashType) -> String {
    if input_pass.is_empty() {
        return String::new();
    }
    match hash_type {
        HashType::None => input_pass.to_string(),
        HashType::Md5 => digest_md5_unicode(&format!("{salt}{input_pass}")),
        HashType::Md5Ecw => digest_hex_md5_ascii(input_pass),
        HashType::Sha3_512 => digest_sha3(&format!("{salt}{input_pass}"), false),
    }
}

/// Generate a new salted SHA3-512 encoded password, as OD does for new users.
pub fn generate(input_pass: &str, hash_type: HashType) -> PasswordContainer {
    let salt = match hash_type {
        HashType::Sha3_512 => BASE64.encode(rand_bytes(64)),
        HashType::Md5 => BASE64.encode(rand_bytes(16)),
        _ => String::new(),
    };
    let hash = hash_password(input_pass, &salt, hash_type);
    PasswordContainer::new(hash_type, salt, hash)
}

fn rand_bytes(len: usize) -> Vec<u8> {
    // Use the operating system's CSPRNG. OD generates its salts with
    // `RandomNumberGenerator.GetBytes`, and a predictable salt would weaken
    // otherwise-correct SHA3-512 password hashing.
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    bytes
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// `Authentication.CheckPassword` for a userod encoded password.
pub fn verify(db_value: &str, input_pass: &str) -> bool {
    let container = decode(db_value);
    // Upstream `CheckPassword(Userod, ...)` treats every `HashTypes.None`
    // container — blank, unrecognised or legacy — as a blank password and
    // requires an empty input, regardless of the stored text.
    if container.hash_type == HashType::None {
        return input_pass.is_empty();
    }
    if container.hash.is_empty() {
        return input_pass.is_empty();
    }
    // Legacy bare MD5 values are detected by length upstream; after decode
    // they already carry hash_type MD5 with an empty salt.
    let candidate = hash_password(input_pass, &container.salt, container.hash_type);
    constant_time_eq(&candidate, &container.hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known vector from Open Dental's AuthenticationTests.cs.
    #[test]
    fn sha3_512_matches_open_dental_vector() {
        let hash = digest_sha3(" ", false);
        assert_eq!(
            hash,
            "J3k3VWdgC8ffI5EhDVNr8aNfrtYZqm+bwUDFSbTbArOodGBxByKJC8S1gRmJwNKO0omhhpjb6DWp8nBMlfm4cA=="
        );
        // Guard against accidentally using the pre-NIST Keccak variant.
        assert_ne!(digest_sha3(" ", true), hash);
    }

    #[test]
    fn md5_ecw_matches_open_dental_vector() {
        assert_eq!(digest_hex_md5_ascii(" "), "7215ee9c7d9dc229d2921a40e899ec5f");
    }

    #[test]
    fn decode_and_verify_round_trip() {
        let encoded = generate("CorrectHorse9", HashType::Sha3_512).encode();
        assert!(verify(&encoded, "CorrectHorse9"));
        assert!(!verify(&encoded, "correcthorse9"));

        // Legacy bare MD5 hash of " " — 24 chars, base64, ends in "==".
        let legacy = BASE64.encode({
            let mut h = Md5::new();
            h.update(utf16le(" "));
            h.finalize()
        });
        assert_eq!(legacy.len(), 24);
        let container = decode(&legacy);
        assert_eq!(container.hash_type, HashType::Md5);
        assert!(verify(&legacy, " "));

        // Blank passwords accept only an empty input.
        assert!(verify("", ""));
        assert!(verify("None$$", ""));
        assert!(!verify("", "x"));
        // An unrecognised hash type is decoded as None by upstream too, so
        // only an empty input can authenticate.
        assert!(verify("FUTURE$salt$hash", ""));
        assert!(!verify("FUTURE$salt$hash", "hash"));
    }
}
