//! Names that say a value may be a secret, and values that are personal data: what no reader
//! keeps (ADR 0003) and no export writes (ADR 0006).

const SECRET_WORDS: [&str; 12] = [
    "password",
    "passwd",
    "secret",
    "token",
    "credential",
    "privatekey",
    "passphrase",
    "apikey",
    "accesskey",
    "authorization",
    "connectionstring",
    "presharedkey",
];

/// Endings of names that only point at a secret or describe it (`SecretArn`, `TokenUrl`,
/// `PasswordLength`), never hold it.
const POINTERS: [&str; 11] = ["arn", "arns", "id", "ids", "name", "names", "type", "url", "endpoint", "length", "version"];

/// `MasterUserPassword`, `x-api-key`, `DB_PASSWORD`: a name whose value may be a secret.
/// Separators don't matter (`api_key`, `api-key` and `ApiKey` are the same name).
pub fn named_like_secret(name: &str) -> bool {
    let k: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    SECRET_WORDS.iter().any(|w| k.contains(w)) && !POINTERS.iter().any(|e| k.ends_with(e))
}

/// An email address or a phone number in international form (`+15555550100`).
pub fn personal_data(value: &str) -> bool {
    let v = value.trim();
    let email = v.matches('@').count() == 1
        && !v.contains(char::is_whitespace)
        && !v.contains("://")
        && v.split_once('@').is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.') && !domain.starts_with('.'));
    let phone = v.strip_prefix('+').is_some_and(|d| (7..=15).contains(&d.len()) && d.chars().all(|c| c.is_ascii_digit()));
    email || phone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_may_hold_a_secret_whatever_their_separators() {
        for k in [
            "MasterUserPassword",
            "x-api-key",
            "api_key",
            "PRIVATE_KEY",
            "access-key",
            "Authorization",
            "DB_PASSWD",
            "AuthToken",
            "ConnectionString",
        ] {
            assert!(named_like_secret(k), "{k}");
        }
        for k in ["SecretArn", "TokenUrl", "PasswordLength", "KmsKeyId", "SecretName", "QueueName", "Key"] {
            assert!(!named_like_secret(k), "{k}");
        }
    }

    #[test]
    fn emails_and_phone_numbers_are_personal_data() {
        assert!(personal_data("jane.doe@example.com"));
        assert!(personal_data("+15555550100"));
        assert!(!personal_data("arn:aws:sns:us-east-1:1:t"));
        assert!(!personal_data("https://user@host.example.com/x"));
        assert!(!personal_data("www.example.com"));
        assert!(!personal_data("+1"));
    }
}
