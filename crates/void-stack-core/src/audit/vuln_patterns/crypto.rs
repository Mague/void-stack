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
                    "Deserializaci\u{00f3}n insegura".into(),
                    format!(
                        "Uso de deserializaci\u{00f3}n insegura (pickle/yaml.load/marshal) en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Evitar pickle/marshal para datos no confiables. Usar yaml.safe_load() en vez de yaml.load(). Preferir JSON para serializaci\u{00f3}n de datos externos.".into(),
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
                "go" => {
                    if go_weak_hash.is_match(line) {
                        is_security_file
                            || line.to_lowercase().contains("password")
                            || line.to_lowercase().contains("hash")
                    } else {
                        false
                    }
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
                    "Criptograf\u{00ed}a d\u{00e9}bil".into(),
                    format!(
                        "Uso de algoritmo criptogr\u{00e1}fico d\u{00e9}bil o inseguro en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Usar SHA-256+ para hashing. Usar bcrypt/argon2/scrypt para passwords. Usar crypto.randomBytes() o secrets.token_bytes() para aleatoriedad criptogr\u{00e1}fica.".into(),
                ));
            }
        }
    }
}
