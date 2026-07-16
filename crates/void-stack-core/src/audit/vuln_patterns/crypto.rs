//! Weak cryptography and insecure deserialization pattern detectors.
//!
//! NOTE: The regex patterns in this file match security-sensitive function names
//! (pickle, marshal, md5, etc.) for static analysis detection purposes only.
//! No actual deserialization or cryptographic operations are performed.

use std::sync::OnceLock;

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity, is_comment};

// ── Static regex helpers ────────────────────────────────────

fn py_pickle_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"pickle\.(loads?|Unpickler)\s*\("#).expect("hardcoded regex"))
}

fn py_yaml_unsafe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"yaml\.load\s*\([^)]*\)"#).expect("hardcoded regex"))
}

fn py_yaml_safe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"yaml\.load\s*\([^)]*Loader\s*=\s*yaml\.SafeLoader"#).expect("hardcoded regex")
    })
}

fn py_marshal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"marshal\.loads?\s*\("#).expect("hardcoded regex"))
}

fn py_jsonpickle_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"jsonpickle\.decode\s*\("#).expect("hardcoded regex"))
}

fn js_unserialize_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\bunserialize\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn py_weak_hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"hashlib\.(md5|sha1)\s*\("#).expect("hardcoded regex"))
}

fn py_weak_random_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"\brandom\.(random|randint|choice|randrange)\s*\("#).expect("hardcoded regex")
    })
}

fn py_weak_cipher_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(DES|RC4|Blowfish|RC2)"#).expect("hardcoded regex"))
}

fn py_hardcoded_iv_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(iv|nonce)\s*=\s*b['"]\\x00"#).expect("hardcoded regex"))
}

fn js_weak_hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"createHash\s*\(\s*['"](?:md5|sha1)['"]\s*\)"#).expect("hardcoded regex")
    })
}

fn js_math_random_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"Math\.random\s*\("#).expect("hardcoded regex"))
}

fn go_weak_hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(md5|sha1)\.New\s*\("#).expect("hardcoded regex"))
}

fn rs_weak_crate_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"use\s+(md5|sha1)"#).expect("hardcoded regex"))
}

// ── Insecure Deserialization ─────────────────────────────────

pub(crate) fn scan_insecure_deserialization(
    files: &[FileInfo],
    findings: &mut Vec<SecurityFinding>,
) {
    let py_pickle = py_pickle_re();
    let py_yaml_unsafe = py_yaml_unsafe_re();
    let py_yaml_safe = py_yaml_safe_re();
    let py_marshal = py_marshal_re();
    let py_jsonpickle = py_jsonpickle_re();
    let js_unserialize = js_unserialize_re();

    for file in files {
        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => {
                    if py_pickle.is_match(line)
                        || py_marshal.is_match(line)
                        || py_jsonpickle.is_match(line)
                    {
                        true
                    } else if py_yaml_unsafe.is_match(line) && !py_yaml_safe.is_match(line) {
                        // yaml.load() without SafeLoader
                        !line.contains("safe_load")
                    } else {
                        false
                    }
                }
                "js" | "ts" | "jsx" | "tsx" => js_unserialize.is_match(line),
                _ => false,
            };

            if matched {
                findings.push(SecurityFinding::new(
                    format!("deser-{}", findings.len()),
                    adjust_severity(Severity::High, file.is_test_file),
                    FindingCategory::InsecureDeserialization,
                    "Insecure deserialization".into(),
                    format!(
                        "Use of insecure deserialization (pickle/yaml.load/marshal) in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Avoid pickle/marshal for untrusted data. Use yaml.safe_load() instead of yaml.load(). Prefer JSON for serializing external data.".into(),
                ));
            }
        }
    }
}

// ── Weak Cryptography ────────────────────────────────────────

pub(crate) fn scan_weak_cryptography(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_weak_hash = py_weak_hash_re();
    let py_weak_random = py_weak_random_re();
    let py_weak_cipher = py_weak_cipher_re();
    let py_hardcoded_iv = py_hardcoded_iv_re();
    let js_weak_hash = js_weak_hash_re();
    let js_math_random = js_math_random_re();
    let go_weak_hash = go_weak_hash_re();
    let rs_weak_crate = rs_weak_crate_re();

    let security_filename_words = [
        "password", "auth", "token", "secret", "key", "otp", "crypt", "hash", "sign", "verify",
    ];

    for file in files {
        let rel_lower = file.rel_path.to_lowercase();
        let is_security_file = security_filename_words
            .iter()
            .any(|w| rel_lower.contains(w));

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => {
                    if py_weak_hash.is_match(line) {
                        // Only flag if in security context or surrounding code suggests password use
                        let context = line.to_lowercase();
                        is_security_file
                            || context.contains("password")
                            || context.contains("hash")
                            || context.contains("sign")
                            || context.contains("verify")
                            || context.contains("token")
                    } else if py_weak_random.is_match(line) && is_security_file {
                        true
                    } else {
                        py_weak_cipher.is_match(line) && line.contains("(")
                            || py_hardcoded_iv.is_match(line)
                    }
                }
                "js" | "ts" | "jsx" | "tsx" => {
                    if js_weak_hash.is_match(line) {
                        let context = line.to_lowercase();
                        is_security_file
                            || context.contains("password")
                            || context.contains("hash")
                            || context.contains("sign")
                            || context.contains("verify")
                    } else {
                        js_math_random.is_match(line) && is_security_file
                    }
                }
                "go" if go_weak_hash.is_match(line) => {
                    is_security_file
                        || line.to_lowercase().contains("password")
                        || line.to_lowercase().contains("hash")
                }
                "rs" => rs_weak_crate.is_match(line) && is_security_file,
                _ => false,
            };

            if matched {
                let severity = if is_security_file {
                    Severity::High
                } else {
                    Severity::Medium
                };

                findings.push(SecurityFinding::new(
                    format!("crypto-{}", findings.len()),
                    adjust_severity(severity, file.is_test_file),
                    FindingCategory::WeakCryptography,
                    "Weak cryptography".into(),
                    format!(
                        "Use of a weak or insecure cryptographic algorithm in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Use SHA-256+ for hashing. Use bcrypt/argon2/scrypt for passwords. Use crypto.randomBytes() or secrets.token_bytes() for cryptographic randomness.".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(path: &str, ext: &str, content: &str) -> FileInfo {
        FileInfo {
            rel_path: path.into(),
            content: content.into(),
            ext: ext.into(),
            is_test_file: false,
        }
    }

    // ── Insecure deserialization ───────────────────────────────

    #[test]
    fn test_deserialization_pickle_loads() {
        let file = make_file("loader.py", "py", "data = pickle.loads(raw_bytes)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::InsecureDeserialization
        ));
        assert!(matches!(findings[0].severity, Severity::High));
    }

    #[test]
    fn test_deserialization_marshal() {
        let file = make_file("loader.py", "py", "obj = marshal.loads(blob)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_deserialization_jsonpickle() {
        let file = make_file("loader.py", "py", "obj = jsonpickle.decode(payload)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_deserialization_yaml_load_unsafe() {
        let file = make_file("cfg.py", "py", "data = yaml.load(content)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert_eq!(findings.len(), 1, "yaml.load without SafeLoader is unsafe");
    }

    #[test]
    fn test_deserialization_yaml_safeloader_ok() {
        // Explicit SafeLoader makes yaml.load safe — must not be flagged.
        let file = make_file(
            "cfg.py",
            "py",
            "data = yaml.load(content, Loader=yaml.SafeLoader)",
        );
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_deserialization_js_unserialize() {
        let file = make_file("session.js", "js", "const obj = unserialize(cookieData)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_deserialization_skips_comments() {
        let file = make_file("loader.py", "py", "# data = pickle.loads(raw_bytes)");
        let mut findings = Vec::new();
        scan_insecure_deserialization(&[file], &mut findings);
        assert!(
            findings.is_empty(),
            "commented-out code must not be flagged"
        );
    }

    // ── Weak cryptography ──────────────────────────────────────

    #[test]
    fn test_weak_hash_md5_in_security_file_is_high() {
        // "auth" in the filename marks the file as security-relevant.
        let file = make_file(
            "auth.py",
            "py",
            "hashed = hashlib.md5(password.encode()).hexdigest()",
        );
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::WeakCryptography
        ));
        assert!(matches!(findings[0].severity, Severity::High));
    }

    #[test]
    fn test_weak_hash_md5_outside_security_file_is_medium() {
        // Still flagged (line context contains "hash"), but at lower severity.
        let file = make_file("etag.py", "py", "checksum = hashlib.md5(data).digest()");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, Severity::Medium));
    }

    #[test]
    fn test_weak_random_in_security_file() {
        // random.* for OTP generation is a weak randomness source.
        let file = make_file("otp.py", "py", "code = random.randint(100000, 999999)");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_weak_random_outside_security_file_ok() {
        // random.* in a non-security file (e.g. game logic) is fine.
        let file = make_file("game.py", "py", "roll = random.randint(1, 6)");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_weak_cipher_des() {
        let file = make_file("cipher_util.py", "py", "c = DES.new(k, DES.MODE_ECB)");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1, "DES cipher usage should be flagged");
    }

    #[test]
    fn test_hardcoded_iv() {
        let file = make_file("aes_util.py", "py", r#"iv = b"\x00\x00\x00\x00""#);
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1, "all-zero IV should be flagged");
    }

    #[test]
    fn test_js_weak_hash_md5() {
        let file = make_file(
            "sign.js",
            "js",
            "const h = crypto.createHash('md5').update(data)",
        );
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_js_math_random_in_security_file() {
        let file = make_file("token.js", "js", "const t = Math.random().toString(36)");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_js_math_random_outside_security_file_ok() {
        // Math.random for UI jitter is not a security issue.
        let file = make_file("animation.js", "js", "const d = Math.random() * 100");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_go_weak_hash_md5() {
        // "hash" in the line context triggers the Go branch.
        let file = make_file("main.go", "go", "hash := md5.New()");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_rust_weak_crate_in_security_file() {
        let file = make_file("src/auth.rs", "rs", "use md5::Md5;");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_rust_weak_crate_outside_security_file_ok() {
        // md5 for non-security purposes (e.g. content checksums) is not flagged.
        let file = make_file("src/checksum.rs", "rs", "use md5::Md5;");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_weak_crypto_skips_comments() {
        let file = make_file("auth.py", "py", "# hashed = hashlib.md5(password.encode())");
        let mut findings = Vec::new();
        scan_weak_cryptography(&[file], &mut findings);
        assert!(findings.is_empty());
    }
}
