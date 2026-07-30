use sha2::{Digest, Sha256};

pub fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_secs() as i64
}

pub fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_millis() as i64
}

pub fn credential_hash(account: &str, credential: &str) -> String {
    let digest = Sha256::digest(format!("{account}\0{credential}"));
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_hash_is_stable_and_account_scoped() {
        assert_eq!(
            credential_hash("a", "secret"),
            credential_hash("a", "secret")
        );
        assert_ne!(
            credential_hash("a", "secret"),
            credential_hash("b", "secret")
        );
    }
}
