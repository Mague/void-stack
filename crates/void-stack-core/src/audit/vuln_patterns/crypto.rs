//! Weak cryptography and insecure deserialization pattern detectors.

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{adjust_severity, is_comment, FileInfo};

// ── Insecure Deserialization ─────────────────────────────────

pub(crate) fn scan_insecure_deserialization(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_pickle = Regex::new(r#"pickle\.(loads?|Unpickler)\s*\("#).unwrap();
    let py_yaml_unsafe = Regex::new(r#"yaml\.load\s*\([^)]*\)"#).unwrap();
    let py_yaml_safe = Regex::new(r#"yaml\.load\s*\([^)]*Loader\s*=\s*yaml\.SafeLoader"#).unwrap();
    let py_marshal = Regex::new(r#"marshal\.loads?\s*\("#).unwrap();
    let py_jsonpickle = Regex::new(r#"jsonpickle\.decode\s*\("#).unwrap();
    let js_unserialize = Regex::new(r#"\bunserialize\s*\(\s*[a-zA-Z_]"#).unwrap();

    for file in files {
        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => {
                    if py_pickle.is_match(line) || py_marshal.is_match(line) || py_jsonpickle.is_match(line) {
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
                findings.push(SecurityFinding {
                    id: format!("deser-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::InsecureDeserialization,
                    title: "Deserializaci\u{00f3}n insegura".into(),
                    description: format!(
                        "Uso de deserializaci\u{00f3}n insegura (pickle/yaml.load/marshal) en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Evitar pickle/marshal para datos no confiables. Usar yaml.safe_load() en vez de yaml.load(). Preferir JSON para serializaci\u{00f3}n de datos externos.".into(),
                });
            }
        }
    }
}

// ── Weak Cryptography ────────────────────────────────────────

pub(crate) fn scan_weak_cryptography(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_weak_hash = Regex::new(r#"hashlib\.(md5|sha1)\s*\("#).unwrap();
    let py_weak_random = Regex::new(r#"\brandom\.(random|randint|choice|randrange)\s*\("#).unwrap();
    let py_weak_cipher = Regex::new(r#"(?i)(DES|RC4|Blowfish|RC2)"#).unwrap();
    let py_hardcoded_iv = Regex::new(r#"(?i)(iv|nonce)\s*=\s*b['"]\\x00"#).unwrap();
    let js_weak_hash = Regex::new(r#"createHash\s*\(\s*['"](?:md5|sha1)['"]\s*\)"#).unwrap();
    let js_math_random = Regex::new(r#"Math\.random\s*\("#).unwrap();
    let go_weak_hash = Regex::new(r#"(md5|sha1)\.New\s*\("#).unwrap();
    let rs_weak_crate = Regex::new(r#"use\s+(md5|sha1)"#).unwrap();

    let security_filename_words = ["password", "auth", "token", "secret", "key", "otp", "crypt", "hash", "sign", "verify"];

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

                findings.push(SecurityFinding {
                    id: format!("crypto-{}", findings.len()),
                    severity: adjust_severity(severity, file.is_test_file),
                    category: FindingCategory::WeakCryptography,
                    title: "Criptograf\u{00ed}a d\u{00e9}bil".into(),
                    description: format!(
                        "Uso de algoritmo criptogr\u{00e1}fico d\u{00e9}bil o inseguro en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Usar SHA-256+ para hashing. Usar bcrypt/argon2/scrypt para passwords. Usar crypto.randomBytes() o secrets.token_bytes() para aleatoriedad criptogr\u{00e1}fica.".into(),
                });
            }
        }
    }
}
