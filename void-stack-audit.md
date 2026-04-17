# Security Audit: void-stack

**Fecha:** 2026-04-17T01:29:01.853478+00:00

## Resumen

| Severidad | Cantidad |
|-----------|----------|
| 🔴 Critical | 0 |
| 🟠 High | 0 |
| 🟡 Medium | 34 |
| 🔵 Low | 4 |
| ℹ️ Info | 55 |
| **Total** | **93** |

**Risk Score:** 34/100

## Hallazgos — Secrets, Configs y Dependencias

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-cli/src/commands/analysis/analyze.rs:172`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-proto/src/client.rs:28`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-desktop/src/commands/scan.rs:375`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/diagram/drawio/db_models.rs:215`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/vector_index/db.rs:42`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/manager/process.rs:21`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/manager/url.rs:11`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/detector/env.rs:39`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:12`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:17`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:22`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:29`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:37`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:43`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/xss.rs:48`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/config.rs:17`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/config.rs:25`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:16`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:24`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:30`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:37`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:43`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:50`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:56`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:61`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:66`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:73`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:79`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:86`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:93`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:99`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:106`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:114`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/injection.rs:120`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:14`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:21`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:27`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:34`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:40`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:46`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:52`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:59`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/network.rs:67`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs:66`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs:67`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs:70`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs:517`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:18`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:23`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:29`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:35`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:40`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:45`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:50`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:56`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:62`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:67`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:73`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:79`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:84`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs:89`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/ai/mod.rs:411`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/log_filter.rs:16`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/log_filter.rs:32`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:183`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:185`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:190`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:192`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:200`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:201`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:203`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:206`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:213`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:214`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:215`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/mod.rs:216`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:411`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:413`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:423`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:456`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:467`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:478`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:493`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:512`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:537`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:559`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .expect() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.expect()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/structural/parser.rs:584`

**Remediación:** Reemplazar .expect() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/analyzer/imports/python.rs:32`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🟡 [medium] Uso de .unwrap() en codigo de produccion

**Categoría:** Manejo de errores inseguro

'.unwrap()' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.

**Archivo:** `crates/void-stack-core/src/analyzer/imports/python.rs:47`

**Remediación:** Reemplazar .unwrap() con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.

---

### 🔵 [low] AWS Access Key

**Categoría:** Secret hardcodeado

Posible AWS Access Key encontrado en crates/void-stack-core/tests/integration_analysis.rs:180

**Archivo:** `crates/void-stack-core/tests/integration_analysis.rs:180`

**Remediación:** Rotar la clave AWS y moverla a variables de entorno o AWS Secrets Manager

---

### 🔵 [low] AWS Access Key

**Categoría:** Secret hardcodeado

Posible AWS Access Key encontrado en crates/void-stack-core/tests/integration_analysis.rs:218

**Archivo:** `crates/void-stack-core/tests/integration_analysis.rs:218`

**Remediación:** Rotar la clave AWS y moverla a variables de entorno o AWS Secrets Manager

---

## Code Vulnerability Patterns

### 🔵 [low] Posible XSS

**Categoría:** Cross-Site Scripting (XSS)

Asignación de HTML no sanitizado o eval() en crates/void-stack-desktop/frontend/src/components/DiagramPanel.tsx:66

**Archivo:** `crates/void-stack-desktop/frontend/src/components/DiagramPanel.tsx:66`

**Remediación:** Nunca asignar input del usuario a innerHTML. Usar textContent. Sanitizar HTML con DOMPurify si se necesita rich content. Evitar eval() y new Function().

---

### 🔵 [low] dangerouslySetInnerHTML

**Categoría:** Cross-Site Scripting (XSS)

Uso de dangerouslySetInnerHTML en crates/void-stack-desktop/frontend/src/components/DiagramPanel.tsx:348 — React escapa por defecto, pero revisar

**Archivo:** `crates/void-stack-desktop/frontend/src/components/DiagramPanel.tsx:348`

**Remediación:** Asegurar que el contenido está sanitizado con DOMPurify antes de usar dangerouslySetInnerHTML.

---

