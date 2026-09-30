//! Las sesiones de Claude Code, contadas por sus propios hooks.
//!
//! Claude Code no usa la GPU, pero trabaja en la misma oficina: el robot que
//! teclea, y el que levanta la mano cuando espera que apruebes algo. Sus hooks
//! (`PreToolUse`, `PostToolUse`, `Notification`, `Stop`…) mandan por stdin un
//! JSON con `session_id`, `cwd` y `hook_event_name`; el hook sólo tiene que
//! reenviarlo tal cual a `POST /v1/agents/hook`. Toda la interpretación vive
//! aquí, para que instalarlo sea una línea de curl.
//!
//! De cada sesión se cuenta en qué proyecto está (el `cwd`), qué está haciendo
//! (la herramienta y lo que dice su entrada: el fichero, la descripción del
//! comando), qué le pediste (la primera línea de tu mensaje) y cuánto lleva,
//! pero sólo si lo sabe: el porcentaje sale de su lista de tareas (`TodoWrite`
//! trae la lista entera en cada cambio). Sin lista no hay porcentaje, y no se
//! inventa uno.
//!
//! Nada de esto gasta tokens: el hook es un curl que Claude Code lanza aparte
//! (`async`), y esta ruta contesta 204 sin cuerpo, así que no hay salida que
//! pueda volver al contexto de la sesión.

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
    /// Qué está haciendo, en una frase: "editando model.ts".
    pub doing: Option<String>,
    /// Qué le pediste: la primera línea de tu último mensaje.
    pub goal: Option<String>,
    /// Su lista de tareas, si lleva una.
    pub progress: Option<AgentProgress>,
    pub last_seen_ms: u64,
    pub status_since_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentProgress {
    pub done: u32,
    pub total: u32,
    /// La tarea en curso, tal cual la escribió la sesión.
    pub current: Option<String>,
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
    /// `permission_prompt`, `idle_prompt`… (documentado en los hooks de
    /// Claude Code). Manda sobre el texto del mensaje cuando viene.
    #[serde(default)]
    pub notification_type: Option<String>,
    /// Lo que escribiste (`UserPromptSubmit`).
    #[serde(default)]
    pub prompt: Option<String>,
    /// La entrada de la herramienta (`PreToolUse`): de ahí sale el "qué hace".
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
}

/// Corta por caracteres, con puntos suspensivos.
fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Qué está haciendo, en una frase, a partir de la herramienta y su entrada.
pub fn doing_of(tool: &str, input: Option<&serde_json::Value>) -> String {
    let field = |k: &str| {
        input
            .and_then(|i| i.get(k))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
    };
    let text = match tool {
        "Edit" | "MultiEdit" | "NotebookEdit" => field("file_path")
            .or(field("notebook_path"))
            .map(|p| format!("editando {}", file_name(p))),
        "Write" => field("file_path").map(|p| format!("escribiendo {}", file_name(p))),
        "Read" => field("file_path").map(|p| format!("leyendo {}", file_name(p))),
        // La descripción que la propia sesión escribe para el comando dice
        // más que el comando; si no hay, la primera línea del comando.
        "Bash" | "PowerShell" => field("description").map(str::to_owned).or_else(|| {
            field("command").map(|c| format!("ejecutando {}", c.lines().next().unwrap_or(c)))
        }),
        "Grep" | "Glob" => field("pattern").map(|p| format!("buscando {p}")),
        "WebFetch" | "WebSearch" => Some("consultando la web".into()),
        "Agent" | "Task" => field("description").map(|d| format!("delegando: {d}")),
        _ => None,
    };
    clip(&text.unwrap_or_else(|| tool.to_owned()), 80)
}

/// El avance de su lista de tareas (`TodoWrite` manda la lista entera).
pub fn progress_of(input: Option<&serde_json::Value>) -> Option<AgentProgress> {
    let todos = input?.get("todos")?.as_array()?;
    if todos.is_empty() {
        return None;
    }
    fn status(t: &serde_json::Value) -> &str {
        t.get("status").and_then(|s| s.as_str()).unwrap_or("")
    }
    let done = todos.iter().filter(|t| status(t) == "completed").count() as u32;
    let current = todos
        .iter()
        .find(|t| status(t) == "in_progress")
        .and_then(|t| {
            t.get("activeForm")
                .or_else(|| t.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| clip(s, 90))
        });
    Some(AgentProgress {
        done,
        total: todos.len() as u32,
        current,
    })
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
                doing: None,
                goal: None,
                progress: None,
                last_seen_ms: now,
                status_since_ms: now,
            });
        if let Some(p) = project {
            entry.project = p;
        }
        entry.last_seen_ms = now;

        let next = match hook.hook_event_name.as_str() {
            "PreToolUse" => {
                let tool = hook.tool_name.unwrap_or_default();
                if tool == "TodoWrite" {
                    // Actualizar la lista no es "hacer": el avance cambia y
                    // lo que hace pasa a ser la tarea en curso.
                    if let Some(p) = progress_of(hook.tool_input.as_ref()) {
                        entry.doing = p.current.clone().or(entry.doing.take());
                        entry.progress = Some(p);
                    }
                } else if !tool.is_empty() {
                    entry.doing = Some(doing_of(&tool, hook.tool_input.as_ref()));
                }
                entry.tool = Some(tool).filter(|t| !t.is_empty());
                entry.message = None;
                Some(AgentStatus::Working)
            }
            "UserPromptSubmit" => {
                if let Some(first) = hook
                    .prompt
                    .as_deref()
                    .and_then(|p| p.lines().find(|l| !l.trim().is_empty()))
                {
                    entry.goal = Some(clip(first, 100));
                }
                // Una lista ya terminada es de lo anterior: un mensaje nuevo
                // empieza de cero. Una a medias puede seguir.
                if entry.progress.as_ref().is_some_and(|p| p.done >= p.total) {
                    entry.progress = None;
                }
                entry.doing = None;
                entry.message = None;
                Some(AgentStatus::Working)
            }
            "PostToolUse" => {
                entry.message = None;
                Some(AgentStatus::Working)
            }
            // Claude Code notifica dos cosas distintas: que necesita permiso
            // para una herramienta, y que lleva un rato esperando tu mensaje.
            // Sólo la primera es "levantar la mano".
            "Notification" => {
                let msg = hook.message.unwrap_or_default();
                // El tipo manda; el texto es el respaldo para versiones de
                // Claude Code que no lo manden. Buscar "permission" en una
                // frase en ingles era fragil, y la documentacion da el campo.
                let asking = match hook.notification_type.as_deref() {
                    Some(kind) => kind == "permission_prompt",
                    None => msg.to_ascii_lowercase().contains("permission"),
                };
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
            notification_type: None,
            prompt: None,
            tool_input: None,
        }
    }

    fn with_input(event: &str, tool: &str, input: serde_json::Value) -> HookPayload {
        let mut h = hook(event, Some(tool), None);
        h.tool_input = Some(input);
        h
    }

    #[test]
    fn dice_que_esta_haciendo_con_lo_que_trae_la_herramienta() {
        use serde_json::json;
        let cases = [
            (
                "Edit",
                json!({"file_path": r"F:\workspace\void-hq\apps\web\src\office\model.ts"}),
                "editando model.ts",
            ),
            (
                "Write",
                json!({"file_path": "/tmp/a.rs"}),
                "escribiendo a.rs",
            ),
            (
                "Read",
                json!({"file_path": "C:/x/README.md"}),
                "leyendo README.md",
            ),
            (
                "Bash",
                json!({"command": "cargo test -p void-stack-gpu", "description": "Run broker tests"}),
                "Run broker tests",
            ),
            (
                "Bash",
                json!({"command": "pnpm build\necho ok"}),
                "ejecutando pnpm build",
            ),
            ("Grep", json!({"pattern": "fn apply"}), "buscando fn apply"),
            ("WebSearch", json!({"query": "x"}), "consultando la web"),
            (
                "Agent",
                json!({"description": "Review the diff"}),
                "delegando: Review the diff",
            ),
            (
                "mcp__void-stack__board_list",
                json!({}),
                "mcp__void-stack__board_list",
            ),
            ("Edit", json!({}), "Edit"),
        ];
        for (tool, input, want) in cases {
            assert_eq!(doing_of(tool, Some(&input)), want, "{tool}");
        }
        let long = "x".repeat(200);
        assert_eq!(
            doing_of("Bash", Some(&json!({"description": long})))
                .chars()
                .count(),
            80
        );
    }

    #[test]
    fn el_porcentaje_sale_de_su_lista_de_tareas_y_solo_de_ahi() {
        use serde_json::json;
        let mut a = Agents::default();
        a.apply(hook("PreToolUse", Some("Edit"), None), 0);
        assert_eq!(
            a.sessions(0)[0].progress,
            None,
            "sin lista no hay porcentaje"
        );

        let todos = json!({"todos": [
            {"content": "Medir", "status": "completed", "activeForm": "Midiendo"},
            {"content": "Escribir el broker", "status": "in_progress", "activeForm": "Escribiendo el broker"},
            {"content": "Probar", "status": "pending", "activeForm": "Probando"},
            {"content": "Commitear", "status": "pending", "activeForm": "Commiteando"},
        ]});
        a.apply(with_input("PreToolUse", "TodoWrite", todos), 1);
        let s = &a.sessions(1)[0];
        assert_eq!(
            s.progress,
            Some(AgentProgress {
                done: 1,
                total: 4,
                current: Some("Escribiendo el broker".into())
            })
        );
        assert_eq!(s.doing.as_deref(), Some("Escribiendo el broker"));
        // Una herramienta después: el qué-hace cambia, el avance se queda.
        a.apply(
            with_input("PreToolUse", "Edit", json!({"file_path": "broker.rs"})),
            2,
        );
        let s = &a.sessions(2)[0];
        assert_eq!(s.doing.as_deref(), Some("editando broker.rs"));
        assert_eq!(s.progress.as_ref().map(|p| p.done), Some(1));
        // Una lista vacía no es un avance.
        assert_eq!(progress_of(Some(&json!({"todos": []}))), None);
        assert_eq!(progress_of(None), None);
    }

    #[test]
    fn lo_que_le_pediste_es_la_primera_linea_y_una_lista_acabada_se_olvida() {
        use serde_json::json;
        let mut a = Agents::default();
        let done = json!({"todos": [{"content": "a", "status": "completed"}]});
        a.apply(with_input("PreToolUse", "TodoWrite", done), 0);
        let mut p = hook("UserPromptSubmit", None, None);
        p.prompt = Some("\n  mete la oficina isometrica en void-hq\ny que se vea el clima".into());
        a.apply(p, 1);
        let s = &a.sessions(1)[0];
        assert_eq!(
            s.goal.as_deref(),
            Some("mete la oficina isometrica en void-hq")
        );
        assert_eq!(s.progress, None);
        assert_eq!(s.doing, None);
        // A medias, la lista sigue con el mensaje nuevo.
        let half = json!({"todos": [{"content": "a", "status": "completed"}, {"content": "b", "status": "pending"}]});
        a.apply(with_input("PreToolUse", "TodoWrite", half), 2);
        a.apply(hook("UserPromptSubmit", None, None), 3);
        assert_eq!(a.sessions(3)[0].progress.as_ref().map(|p| p.total), Some(2));
    }

    #[test]
    fn el_tipo_de_notificacion_manda_sobre_el_texto() {
        let mut a = Agents::default();
        let mut h = hook("Notification", None, Some("texto que no dice nada"));
        h.notification_type = Some("permission_prompt".into());
        a.apply(h, 0);
        assert_eq!(a.sessions(0)[0].status, AgentStatus::WaitingApproval);

        // Y al reves: aunque el texto hable de permisos, idle_prompt es esperar.
        let mut h = hook("Notification", None, Some("permission"));
        h.notification_type = Some("idle_prompt".into());
        a.apply(h, 1);
        assert_eq!(a.sessions(1)[0].status, AgentStatus::Idle);
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
