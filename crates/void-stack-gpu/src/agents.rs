//! Las sesiones de Claude Code, contadas por sus propios hooks.
//!
//! Claude Code no usa la GPU, pero trabaja en la misma oficina: el robot que
//! teclea, y el que levanta la mano cuando espera que apruebes algo. Sus hooks
//! (`PreToolUse`, `PostToolUse`, `Notification`, `Stop`…) mandan por stdin un
//! JSON con `session_id`, `cwd` y `hook_event_name`; el hook sólo tiene que
//! reenviarlo tal cual a `POST /v1/agents/hook`. Toda la interpretación vive
//! aquí, para que instalarlo sea una línea de curl.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Sin noticias en este tiempo, la sesión se da por cerrada.
pub const SESSION_TTL_MS: u64 = 30 * 60_000;

/// Cuánto se enseña "terminó" antes de volver a quieto. El confeti dura poco.
pub const DONE_SHOW_MS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Working,
    /// Pidió permiso para usar una herramienta: el robot levanta la mano.
    WaitingApproval,
    /// Terminó de contestar y espera tu siguiente mensaje.
    Idle,
    Done,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentSession {
    pub session_id: String,
    /// El proyecto: el último trozo del `cwd`.
    pub project: String,
    pub status: AgentStatus,
    /// La herramienta en curso, o la última que usó.
    pub tool: Option<String>,
    /// Lo que dijo la notificación, cuando la hubo.
    pub message: Option<String>,
    pub last_seen_ms: u64,
    pub status_since_ms: u64,
}

/// Lo que manda un hook de Claude Code. Sólo se leen los campos que importan;
/// el resto se ignora para no romperse cuando Claude Code añada más.
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

pub fn project_of(cwd: &str) -> String {
    cwd.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("?")
        .to_owned()
}

#[derive(Default)]
pub struct Agents {
    sessions: BTreeMap<String, AgentSession>,
}

impl Agents {
    pub fn apply(&mut self, hook: HookPayload, now: u64) {
        if hook.hook_event_name == "SessionEnd" {
            self.sessions.remove(&hook.session_id);
            return;
        }
        let project = hook.cwd.as_deref().map(project_of);
        let entry = self
            .sessions
            .entry(hook.session_id.clone())
            .or_insert_with(|| AgentSession {
                session_id: hook.session_id.clone(),
                project: project.clone().unwrap_or_else(|| "?".into()),
                status: AgentStatus::Idle,
                tool: None,
                message: None,
                last_seen_ms: now,
                status_since_ms: now,
            });
        if let Some(p) = project {
            entry.project = p;
        }
        entry.last_seen_ms = now;

        let next = match hook.hook_event_name.as_str() {
            "PreToolUse" => {
                entry.tool = hook.tool_name;
                entry.message = None;
                Some(AgentStatus::Working)
            }
            "PostToolUse" | "UserPromptSubmit" => {
                entry.message = None;
                Some(AgentStatus::Working)
            }
            // Claude Code notifica dos cosas distintas: que necesita permiso
            // para una herramienta, y que lleva un rato esperando tu mensaje.
            // Sólo la primera es "levantar la mano".
            "Notification" => {
                let msg = hook.message.unwrap_or_default();
                let asking = msg.to_ascii_lowercase().contains("permission");
                entry.message = Some(msg);
                Some(if asking {
                    AgentStatus::WaitingApproval
                } else {
                    AgentStatus::Idle
                })
            }
            "Stop" | "SubagentStop" => Some(AgentStatus::Done),
            _ => None,
        };
        if let Some(status) = next
            && status != entry.status
        {
            entry.status = status;
            entry.status_since_ms = now;
        }
    }

    /// Las sesiones vivas, con "terminó" convertido en quieto pasado un rato.
    pub fn sessions(&mut self, now: u64) -> Vec<AgentSession> {
        self.sessions
            .retain(|_, s| now.saturating_sub(s.last_seen_ms) <= SESSION_TTL_MS);
        for s in self.sessions.values_mut() {
            if s.status == AgentStatus::Done && now.saturating_sub(s.status_since_ms) > DONE_SHOW_MS
            {
                s.status = AgentStatus::Idle;
                s.status_since_ms = now;
            }
        }
        self.sessions.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(event: &str, tool: Option<&str>, message: Option<&str>) -> HookPayload {
        HookPayload {
            session_id: "s1".into(),
            cwd: Some(r"F:\workspace\void-hq".into()),
            hook_event_name: event.into(),
            tool_name: tool.map(str::to_owned),
            message: message.map(str::to_owned),
        }
    }

    #[test]
    fn el_proyecto_sale_del_cwd() {
        assert_eq!(project_of(r"F:\workspace\void-hq"), "void-hq");
        assert_eq!(project_of("/home/mague/iunci.app/"), "iunci.app");
        assert_eq!(project_of(""), "?");
    }

    #[test]
    fn usar_una_herramienta_es_trabajar_y_se_ve_cual() {
        let mut a = Agents::default();
        a.apply(hook("PreToolUse", Some("Bash"), None), 0);
        let s = &a.sessions(0)[0];
        assert_eq!(s.status, AgentStatus::Working);
        assert_eq!(s.tool.as_deref(), Some("Bash"));
        assert_eq!(s.project, "void-hq");
    }

    #[test]
    fn pedir_permiso_levanta_la_mano() {
        let mut a = Agents::default();
        a.apply(
            hook(
                "Notification",
                None,
                Some("Claude needs your permission to use Bash"),
            ),
            0,
        );
        assert_eq!(a.sessions(0)[0].status, AgentStatus::WaitingApproval);
    }

    #[test]
    fn esperar_tu_mensaje_no_es_pedir_permiso() {
        let mut a = Agents::default();
        a.apply(
            hook(
                "Notification",
                None,
                Some("Claude is waiting for your input"),
            ),
            0,
        );
        assert_eq!(a.sessions(0)[0].status, AgentStatus::Idle);
    }

    #[test]
    fn terminar_celebra_un_rato_y_luego_se_queda_quieto() {
        let mut a = Agents::default();
        a.apply(hook("Stop", None, None), 0);
        assert_eq!(a.sessions(1_000)[0].status, AgentStatus::Done);
        assert_eq!(a.sessions(DONE_SHOW_MS + 1)[0].status, AgentStatus::Idle);
    }

    #[test]
    fn una_sesion_callada_media_hora_desaparece() {
        let mut a = Agents::default();
        a.apply(hook("PreToolUse", Some("Read"), None), 0);
        assert!(a.sessions(SESSION_TTL_MS + 1).is_empty());
    }

    #[test]
    fn cerrar_la_sesion_la_quita_ya() {
        let mut a = Agents::default();
        a.apply(hook("PreToolUse", Some("Read"), None), 0);
        a.apply(hook("SessionEnd", None, None), 1);
        assert!(a.sessions(1).is_empty());
    }

    #[test]
    fn el_json_de_verdad_de_un_hook_se_entiende() {
        // Tal cual lo manda Claude Code, con campos que no usamos.
        let raw = r#"{"session_id":"abc","transcript_path":"x.jsonl","cwd":"F:\\workspace\\OSAC",
            "hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"a"}}"#;
        let payload: HookPayload = serde_json::from_str(raw).unwrap();
        let mut a = Agents::default();
        a.apply(payload, 0);
        assert_eq!(a.sessions(0)[0].project, "OSAC");
    }
}
