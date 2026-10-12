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
    /// La lista entera, en su orden.
    pub todos: Vec<AgentTodo>,
    /// Lo que terminó en esta sesión, lo más reciente primero (hasta 30): no
    /// se pierde cuando la sesión empieza una lista nueva.
    pub finished: Vec<FinishedTodo>,
    pub last_seen_ms: u64,
    pub status_since_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentTodo {
    /// El número de `TaskCreate`, o la posición en la lista de `TodoWrite`.
    pub id: String,
    pub content: String,
    /// "Escribiendo el broker": lo que dice mientras la hace.
    pub active_form: Option<String>,
    /// `pending`, `in_progress` o `completed`.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FinishedTodo {
    pub content: String,
    pub at_ms: u64,
}

/// Cuántas tareas terminadas se recuerdan por sesión.
const FINISHED_KEEP: usize = 30;

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
    /// Lo que respondió (`PostToolUse`): `TaskCreate` da aquí el número de la
    /// tarea, `{"task": {"id": "1", "subject": …}}` (medido en una sesión real).
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
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

/// El avance de una lista de `TodoWrite`.
pub fn progress_of(input: Option<&serde_json::Value>) -> Option<AgentProgress> {
    progress_from(&todos_of(input)?)
}

/// La lista de `TodoWrite` (llega entera en cada cambio).
pub fn todos_of(input: Option<&serde_json::Value>) -> Option<Vec<AgentTodo>> {
    let todos = input?.get("todos")?.as_array()?;
    Some(
        todos
            .iter()
            .enumerate()
            .map(|(i, t)| AgentTodo {
                id: (i + 1).to_string(),
                content: clip(t.get("content").and_then(|v| v.as_str()).unwrap_or(""), 120),
                active_form: t
                    .get("activeForm")
                    .and_then(|v| v.as_str())
                    .map(|s| clip(s, 90)),
                status: t
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending")
                    .to_owned(),
            })
            .collect(),
    )
}

/// El avance, sacado de la lista.
fn progress_from(todos: &[AgentTodo]) -> Option<AgentProgress> {
    if todos.is_empty() {
        return None;
    }
    let done = todos.iter().filter(|t| t.status == "completed").count() as u32;
    let current = todos
        .iter()
        .find(|t| t.status == "in_progress")
        .map(|t| t.active_form.clone().unwrap_or_else(|| t.content.clone()));
    Some(AgentProgress {
        done,
        total: todos.len() as u32,
        current,
    })
}

/// Lo que pasó a terminada entre una lista y la siguiente.
fn newly_finished(before: &[AgentTodo], after: &[AgentTodo], now: u64) -> Vec<FinishedTodo> {
    after
        .iter()
        .filter(|t| t.status == "completed")
        .filter(|t| {
            !before
                .iter()
                .any(|b| b.content == t.content && b.status == "completed")
        })
        .map(|t| FinishedTodo {
            content: t.content.clone(),
            at_ms: now,
        })
        .collect()
}

/// Carpetas que no dicen de qué proyecto se trata: `am/apps/web` es "am".
const GENERIC: &[&str] = &[
    "web", "app", "apps", "src", "client", "server", "frontend", "backend", "api", "packages",
    "mobile",
];

/// El proyecto, sacado del `cwd`. Si la última carpeta es genérica se sube
/// hasta la primera que no lo es, y se dice las dos: una sesión en
/// `F:\workspace\am\apps\web` salía como "web" y no se reconocía (visto el
/// 2026-09-30); ahora es "am/web".
pub fn project_of(cwd: &str) -> String {
    let parts: Vec<&str> = cwd
        .split(['\\', '/'])
        .filter(|s| !s.is_empty() && !s.ends_with(':'))
        .collect();
    let Some(last) = parts.last() else {
        return "?".to_owned();
    };
    let generic = |s: &str| GENERIC.contains(&s.to_ascii_lowercase().as_str());
    if !generic(last) {
        return (*last).to_owned();
    }
    match parts.iter().rev().skip(1).find(|s| !generic(s)) {
        Some(root) => format!("{root}/{last}"),
        None => (*last).to_owned(),
    }
}

/// Tras cambiar la lista: el avance, lo que hace ahora y lo terminado.
fn record(entry: &mut AgentSession, done: Vec<FinishedTodo>) {
    entry.progress = progress_from(&entry.todos);
    if let Some(current) = entry.progress.as_ref().and_then(|p| p.current.clone()) {
        entry.doing = Some(current);
    }
    for f in done.into_iter().rev() {
        entry.finished.insert(0, f);
    }
    entry.finished.truncate(FINISHED_KEEP);
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
                todos: Vec::new(),
                finished: Vec::new(),
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
                    // Actualizar la lista no es "hacer": la lista cambia entera
                    // y lo que hace pasa a ser la tarea en curso.
                    if let Some(todos) = todos_of(hook.tool_input.as_ref()) {
                        let done = newly_finished(&entry.todos, &todos, now);
                        entry.todos = todos;
                        record(entry, done);
                    }
                } else if tool.starts_with("Task") && tool != "Task" {
                    // TaskCreate / TaskUpdate: se apuntan al terminar (PostToolUse),
                    // que es cuando se sabe el número y que salió bien.
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
                // empieza de cero (lo hecho queda en `finished`). Una a
                // medias puede seguir.
                if entry.progress.as_ref().is_some_and(|p| p.done >= p.total) {
                    entry.progress = None;
                    entry.todos.clear();
                }
                entry.doing = None;
                entry.message = None;
                Some(AgentStatus::Working)
            }
            "PostToolUse" => {
                let input = hook.tool_input.as_ref();
                let field = |k: &str| input.and_then(|i| i.get(k)).and_then(|v| v.as_str());
                match hook.tool_name.as_deref() {
                    Some("TaskCreate") => {
                        let id = hook
                            .tool_response
                            .as_ref()
                            .and_then(|r| r.get("task"))
                            .and_then(|t| t.get("id"))
                            .and_then(|v| {
                                v.as_str()
                                    .map(str::to_owned)
                                    .or_else(|| v.as_u64().map(|n| n.to_string()))
                            })
                            .unwrap_or_else(|| (entry.todos.len() + 1).to_string());
                        let content = field("subject").unwrap_or("");
                        if !content.is_empty() && !entry.todos.iter().any(|t| t.id == id) {
                            entry.todos.push(AgentTodo {
                                id,
                                content: clip(content, 120),
                                active_form: field("activeForm").map(|s| clip(s, 90)),
                                status: "pending".into(),
                            });
                            record(entry, Vec::new());
                        }
                    }
                    Some("TaskUpdate") => {
                        let id = input.and_then(|i| i.get("taskId")).and_then(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .or_else(|| v.as_u64().map(|n| n.to_string()))
                        });
                        if let Some(id) = id {
                            let before = entry.todos.clone();
                            if field("status") == Some("deleted") {
                                entry.todos.retain(|t| t.id != id);
                            } else if let Some(t) = entry.todos.iter_mut().find(|t| t.id == id) {
                                if let Some(st) = field("status") {
                                    t.status = st.to_owned();
                                }
                                if let Some(sub) = field("subject") {
                                    t.content = clip(sub, 120);
                                }
                                if let Some(af) = field("activeForm") {
                                    t.active_form = Some(clip(af, 90));
                                }
                            }
                            let done = newly_finished(&before, &entry.todos, now);
                            record(entry, done);
                        }
                    }
                    _ => {}
                }
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
            tool_response: None,
        }
    }

    #[test]
    fn la_lista_de_taskcreate_y_taskupdate_se_reconstruye() {
        use serde_json::json;
        // La forma exacta de una sesión real de catatumbo (2026-09-30).
        let mut a = Agents::default();
        for (n, subject, active) in [
            ("1", "Scaffold catatumbo repo skeleton", "Scaffolding repo"),
            (
                "2",
                "Write Dart capture template",
                "Writing Dart capture template",
            ),
        ] {
            let mut h = with_input(
                "PostToolUse",
                "TaskCreate",
                json!({"subject": subject, "description": "…", "activeForm": active}),
            );
            h.tool_response = Some(json!({"task": {"id": n, "subject": subject}}));
            a.apply(h, 0);
        }
        a.apply(
            with_input(
                "PostToolUse",
                "TaskUpdate",
                json!({"taskId": "1", "status": "in_progress"}),
            ),
            1,
        );
        let s = &a.sessions(1)[0];
        assert_eq!(s.todos.len(), 2);
        assert_eq!(
            s.progress,
            Some(AgentProgress {
                done: 0,
                total: 2,
                current: Some("Scaffolding repo".into())
            })
        );
        assert_eq!(s.doing.as_deref(), Some("Scaffolding repo"));

        a.apply(
            with_input(
                "PostToolUse",
                "TaskUpdate",
                json!({"taskId": "1", "status": "completed"}),
            ),
            2,
        );
        a.apply(
            with_input(
                "PostToolUse",
                "TaskUpdate",
                json!({"taskId": "2", "status": "in_progress"}),
            ),
            3,
        );
        let s = &a.sessions(3)[0];
        assert_eq!(s.progress.as_ref().map(|p| (p.done, p.total)), Some((1, 2)));
        assert_eq!(
            s.finished,
            vec![FinishedTodo {
                content: "Scaffold catatumbo repo skeleton".into(),
                at_ms: 2
            }]
        );
        // Otra vez "completed" no la apunta dos veces; "deleted" la quita.
        a.apply(
            with_input(
                "PostToolUse",
                "TaskUpdate",
                json!({"taskId": "1", "status": "completed"}),
            ),
            4,
        );
        a.apply(
            with_input(
                "PostToolUse",
                "TaskUpdate",
                json!({"taskId": "2", "status": "deleted"}),
            ),
            5,
        );
        let s = &a.sessions(5)[0];
        assert_eq!(s.finished.len(), 1);
        assert_eq!(s.todos.len(), 1);
        // El PreToolUse de TaskCreate no es "hacer": no cambia lo que hace.
        a.apply(
            with_input("PreToolUse", "TaskCreate", json!({"subject": "x"})),
            6,
        );
        assert_eq!(a.sessions(6)[0].todos.len(), 1);
    }

    #[test]
    fn lo_terminado_con_todowrite_queda_aunque_empiece_otra_lista() {
        use serde_json::json;
        let mut a = Agents::default();
        let list = |s1: &str, s2: &str| {
            json!({"todos": [
                {"content": "Medir", "status": s1, "activeForm": "Midiendo"},
                {"content": "Escribir", "status": s2, "activeForm": "Escribiendo"},
            ]})
        };
        a.apply(
            with_input("PreToolUse", "TodoWrite", list("in_progress", "pending")),
            0,
        );
        a.apply(
            with_input("PreToolUse", "TodoWrite", list("completed", "in_progress")),
            1,
        );
        a.apply(
            with_input("PreToolUse", "TodoWrite", list("completed", "completed")),
            2,
        );
        // Un mensaje nuevo: la lista acabada se va, lo hecho se queda.
        a.apply(hook("UserPromptSubmit", None, None), 3);
        let s = &a.sessions(3)[0];
        assert!(s.todos.is_empty());
        let done: Vec<_> = s
            .finished
            .iter()
            .map(|f| (f.content.as_str(), f.at_ms))
            .collect();
        assert_eq!(done, vec![("Escribir", 2), ("Medir", 1)]);
    }

    #[test]
    fn se_recuerdan_las_ultimas_treinta_terminadas() {
        use serde_json::json;
        let mut a = Agents::default();
        for k in 0..40u64 {
            let mut h = with_input(
                "PostToolUse",
                "TaskCreate",
                json!({"subject": format!("t{k}")}),
            );
            h.tool_response = Some(json!({"task": {"id": k + 1, "subject": "x"}}));
            a.apply(h, k);
            a.apply(
                with_input(
                    "PostToolUse",
                    "TaskUpdate",
                    json!({"taskId": (k + 1).to_string(), "status": "completed"}),
                ),
                k,
            );
        }
        let s = &a.sessions(40)[0];
        assert_eq!(s.finished.len(), 30);
        assert_eq!(s.finished[0].content, "t39");
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
        assert_eq!(project_of(r"F:\workspace\am\apps\web"), "am/web");
        assert_eq!(project_of("/home/mague/iunci/backend/"), "iunci/backend");
        assert_eq!(project_of(r"C:\src"), "src");
        assert_eq!(project_of(r"F:\"), "?");
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
