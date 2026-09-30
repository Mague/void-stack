//! La capa asíncrona: HTTP, WebSocket y el muestreador.
//!
//! El núcleo (`broker.rs`) decide; aquí sólo se mide, se cumplen los efectos
//! que devuelve (descargar Ollama), se escribe el historial y se avisa a quien
//! mire. Cada mutación pasa por `settle`, que es el único sitio que drena.
//!
//! La API, en `127.0.0.1` y nada más (ver `config::DEFAULT_LISTEN`):
//!
//!   POST   /v1/leases                  pedir la GPU → {lease_id, state, position}
//!   GET    /v1/leases/{id}             ¿ya me toca? (cuenta como latido)
//!   POST   /v1/leases/{id}/heartbeat   {progress, message} → {directive}
//!   POST   /v1/leases/{id}/yield       ¿sigo o cedo? → {directive}
//!   DELETE /v1/leases/{id}             soltar
//!   POST   /v1/leases/{id}/hold|resume|cancel,  /priority {priority}
//!   POST   /v1/training {on}, /v1/queue {paused}, /v1/ollama/unload
//!   GET    /v1/state                   la foto entera
//!   GET    /v1/history?hours=24        quién ocupó la GPU
//!   POST   /v1/agents/hook             lo que manda un hook de Claude Code
//!   GET    /v1/events                  WebSocket: la foto, cada vez que cambia

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::agents::{AgentSession, Agents, HookPayload};
use crate::broker::{Acquired, Broker, BrokerError};
use crate::config::GpuConfig;
use crate::history::History;
use crate::model::{Directive, Effect, HistoryEntry, Lease, LeaseRequest, LeaseState, Priority};
use crate::ollama::{Ollama, summarize};
use crate::probe::{GpuReading, Intruders, Probe, interactive_apps, intruders};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Un lease como lo ve La Oficina: con su posición en la fila.
#[derive(Debug, Clone, Serialize)]
pub struct LeaseView {
    #[serde(flatten)]
    pub lease: Lease,
    pub position: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaView {
    pub loaded_mb: u32,
    pub models: Vec<String>,
}

/// La foto entera. Lo que manda el WebSocket y `GET /v1/state`.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub now_ms: u64,
    /// Nulo si NVML no está: el broker reparte por lo declarado.
    pub gpu: Option<GpuReading>,
    pub total_mb: u32,
    pub baseline_mb: u32,
    pub accounted_mb: u32,
    pub free_mb: u32,
    pub leases: Vec<LeaseView>,
    pub ollama: OllamaView,
    pub intruders: Intruders,
    pub agents: Vec<AgentSession>,
    pub training: bool,
    pub queue_paused: bool,
    /// Lo que estás usando tú (modo juego). Vacío si no hay nada.
    pub interactive: Vec<String>,
    pub warnings: Vec<String>,
}

struct Inner {
    cfg: GpuConfig,
    broker: Mutex<Broker>,
    agents: Mutex<Agents>,
    history: Mutex<Option<History>>,
    reading: Mutex<Option<GpuReading>>,
    warnings: Mutex<Vec<String>>,
    changed: broadcast::Sender<()>,
    ollama: Ollama,
}

#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

impl Service {
    pub fn new(cfg: GpuConfig, history: Option<History>, warnings: Vec<String>) -> Self {
        let (changed, _) = broadcast::channel(64);
        let boot = format!("{:x}", now_ms() % 0xffff_ffff);
        Self {
            inner: Arc::new(Inner {
                broker: Mutex::new(Broker::new(cfg.limits(), boot)),
                ollama: Ollama::new(cfg.ollama_url.clone()),
                cfg,
                agents: Mutex::new(Agents::default()),
                history: Mutex::new(history),
                reading: Mutex::new(None),
                warnings: Mutex::new(warnings),
                changed,
            }),
        }
    }

    /// Lo que el núcleo dejó pendiente: historial al disco, Ollama a descargar,
    /// y un aviso a quien mire. El ÚNICO sitio que drena el broker.
    fn settle(&self) {
        let (effects, history) = {
            let mut b = self.inner.broker.lock().unwrap();
            (b.drain_effects(), b.drain_history())
        };
        if !history.is_empty()
            && let Some(h) = self.inner.history.lock().unwrap().as_ref()
        {
            for entry in &history {
                // De mejor esfuerzo: un disco lleno no para al árbitro.
                if let Err(e) = h.record(entry) {
                    tracing::warn!("gpu: no pude guardar el historial: {e}");
                }
            }
        }
        for effect in effects {
            match effect {
                Effect::UnloadOllama { reason } => {
                    let ollama = Ollama::new(self.inner.cfg.ollama_url.clone());
                    tokio::spawn(async move {
                        let unloaded = ollama.unload_all().await;
                        tracing::info!("gpu: Ollama descargado ({reason}): {unloaded:?}");
                    });
                }
                Effect::Changed => {}
            }
        }
        let _ = self.inner.changed.send(());
    }

    /// Una vuelta del muestreador: medir, preguntar a Ollama, caducar muertos.
    pub async fn sample(&self, probe: Option<Arc<Mutex<Box<dyn Probe>>>>) {
        let reading = match probe {
            Some(p) => tokio::task::spawn_blocking(move || p.lock().unwrap().read())
                .await
                .ok()
                .and_then(Result::ok),
            None => None,
        };
        let (ollama_mb, models) = summarize(&self.inner.ollama.loaded().await);
        let now = now_ms();
        {
            let mut b = self.inner.broker.lock().unwrap();
            b.set_ollama(ollama_mb, models, now);
            if let Some(r) = &reading {
                b.set_measured(Some(r.used_mb), Some(r.total_mb), now);
                let apps = interactive_apps(&r.processes, &self.inner.cfg.interactive);
                b.set_interactive(apps, now);
            }
            b.tick(now);
        }
        *self.inner.reading.lock().unwrap() = reading;
        self.settle();
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = now_ms();
        let reading = self.inner.reading.lock().unwrap().clone();
        let agents = self.inner.agents.lock().unwrap().sessions(now);
        let b = self.inner.broker.lock().unwrap();

        let mut queue: Vec<&Lease> = b
            .leases()
            .iter()
            .filter(|l| l.state == LeaseState::Queued)
            .collect();
        queue.sort_by_key(|l| (l.priority, l.requested_at_ms));
        let leases = b
            .leases()
            .iter()
            .map(|l| LeaseView {
                lease: l.clone(),
                position: queue.iter().position(|q| q.id == l.id).map(|p| p + 1),
            })
            .collect();

        let leased_pids: BTreeSet<u32> = b
            .leases()
            .iter()
            .filter(|l| l.state == LeaseState::Granted)
            .filter_map(|l| l.pid)
            .collect();
        let mut intruders = intruders(
            reading
                .as_ref()
                .map(|r| r.processes.as_slice())
                .unwrap_or(&[]),
            &leased_pids,
            &self.inner.cfg.intruder_watch,
            b.unexplained_mb(),
            self.inner.cfg.intruder_min_mb(),
        );
        // La memoria de tu juego no se atribuye a nadie (en WDDM no hay
        // memoria por proceso), pero es tuya: no es un intruso.
        if !b.interactive().is_empty() {
            intruders.alert = false;
        }

        Snapshot {
            now_ms: now,
            gpu: reading,
            total_mb: b.limits().total_mb,
            baseline_mb: b.limits().baseline_mb,
            accounted_mb: b.accounted_mb(),
            free_mb: b.free_mb(),
            leases,
            ollama: OllamaView {
                loaded_mb: b.ollama_loaded_mb(),
                models: b.ollama_models().to_vec(),
            },
            intruders,
            agents,
            training: b.training(),
            queue_paused: b.queue_paused(),
            interactive: b.interactive().to_vec(),
            warnings: self.inner.warnings.lock().unwrap().clone(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.inner.changed.subscribe()
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut Broker, u64) -> T) -> T {
        let out = {
            let mut b = self.inner.broker.lock().unwrap();
            f(&mut b, now_ms())
        };
        self.settle();
        out
    }
}

// ── HTTP ────────────────────────────────────────────────────────────────────

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<BrokerError> for ApiError {
    fn from(e: BrokerError) -> Self {
        let status = match e {
            BrokerError::UnknownLease(_) => StatusCode::NOT_FOUND,
            BrokerError::NotPreemptible(_) => StatusCode::CONFLICT,
        };
        ApiError(status, e.to_string())
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Serialize)]
struct DirectiveBody {
    directive: Directive,
}

#[derive(Deserialize, Default)]
struct HeartbeatBody {
    #[serde(default)]
    progress: Option<f32>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct PriorityBody {
    priority: Priority,
}

#[derive(Deserialize)]
struct OnBody {
    on: bool,
}

#[derive(Deserialize)]
struct PausedBody {
    paused: bool,
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default = "default_hours")]
    hours: u64,
}

fn default_hours() -> u64 {
    24
}

async fn acquire(State(s): State<Service>, Json(mut req): Json<LeaseRequest>) -> Json<Acquired> {
    // El fichero manda sobre lo que pide el cliente. Ver config.rs.
    req.priority = s.inner.cfg.priority_for(&req.owner, req.priority);
    Json(s.mutate(|b, now| b.acquire(req, now)))
}

async fn poll(State(s): State<Service>, Path(id): Path<String>) -> ApiResult<Acquired> {
    Ok(Json(s.mutate(|b, now| b.poll(&id, now))?))
}

async fn heartbeat(
    State(s): State<Service>,
    Path(id): Path<String>,
    body: Option<Json<HeartbeatBody>>,
) -> ApiResult<DirectiveBody> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let directive = s.mutate(|b, now| b.heartbeat(&id, body.progress, body.message, now))?;
    Ok(Json(DirectiveBody { directive }))
}

async fn yield_point(State(s): State<Service>, Path(id): Path<String>) -> ApiResult<DirectiveBody> {
    let directive = s.mutate(|b, now| b.yield_point(&id, now))?;
    Ok(Json(DirectiveBody { directive }))
}

async fn release(State(s): State<Service>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.mutate(|b, now| b.release(&id, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn hold(State(s): State<Service>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.mutate(|b, now| b.hold(&id, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume(State(s): State<Service>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.mutate(|b, now| b.resume(&id, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cancel(State(s): State<Service>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.mutate(|b, now| b.cancel(&id, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn priority(
    State(s): State<Service>,
    Path(id): Path<String>,
    Json(body): Json<PriorityBody>,
) -> Result<StatusCode, ApiError> {
    s.mutate(|b, now| b.set_priority(&id, body.priority, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn training(State(s): State<Service>, Json(body): Json<OnBody>) -> StatusCode {
    s.mutate(|b, now| b.set_training(body.on, now));
    StatusCode::NO_CONTENT
}

async fn queue(State(s): State<Service>, Json(body): Json<PausedBody>) -> StatusCode {
    s.mutate(|b, now| b.set_queue_paused(body.paused, now));
    StatusCode::NO_CONTENT
}

async fn unload_ollama(State(s): State<Service>) -> StatusCode {
    s.mutate(|b, now| b.request_ollama_unload(now));
    StatusCode::ACCEPTED
}

async fn state(State(s): State<Service>) -> Json<Snapshot> {
    Json(s.snapshot())
}

async fn history(
    State(s): State<Service>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Vec<HistoryEntry>> {
    let since = now_ms().saturating_sub(q.hours.min(24 * 30) * 3_600_000);
    let guard = s.inner.history.lock().unwrap();
    let Some(h) = guard.as_ref() else {
        return Ok(Json(Vec::new()));
    };
    h.since(since)
        .map(Json)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn agent_hook(State(s): State<Service>, Json(hook): Json<HookPayload>) -> StatusCode {
    s.inner.agents.lock().unwrap().apply(hook, now_ms());
    let _ = s.inner.changed.send(());
    StatusCode::NO_CONTENT
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "service": "void-stack-gpu" }))
}

async fn events(ws: WebSocketUpgrade, State(s): State<Service>) -> Response {
    ws.on_upgrade(move |socket| stream(socket, s))
}

/// Manda la foto al conectar y cada vez que algo cambia. Si el cliente se
/// queda atrás no se le mandan las fotos viejas una a una: sólo la última.
async fn stream(mut socket: WebSocket, s: Service) {
    let mut rx = s.subscribe();
    loop {
        let body = serde_json::to_string(&s.snapshot()).unwrap_or_default();
        if socket.send(Message::Text(body)).await.is_err() {
            return;
        }
        match rx.recv().await {
            Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

pub fn router(service: Service) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/state", get(state))
        .route("/v1/history", get(history))
        .route("/v1/events", get(events))
        .route("/v1/leases", post(acquire))
        .route("/v1/leases/:id", get(poll).delete(release))
        .route("/v1/leases/:id/heartbeat", post(heartbeat))
        .route("/v1/leases/:id/yield", post(yield_point))
        .route("/v1/leases/:id/hold", post(hold))
        .route("/v1/leases/:id/resume", post(resume))
        .route("/v1/leases/:id/cancel", post(cancel))
        .route("/v1/leases/:id/priority", post(priority))
        .route("/v1/training", post(training))
        .route("/v1/queue", post(queue))
        .route("/v1/ollama/unload", post(unload_ollama))
        .route("/v1/agents/hook", post(agent_hook))
        .with_state(service)
}

/// Arranca el broker: lee `gpu.toml`, abre el historial, enlaza en loopback y
/// muestrea la GPU para siempre. Nunca tumba a quien lo aloja: si algo falla
/// se registra y se sigue.
pub async fn run() -> anyhow::Result<()> {
    let path = crate::config::default_path();
    let (cfg, warning) = crate::config::load(path.as_deref());
    let mut warnings: Vec<String> = warning.into_iter().collect();

    let history = void_stack_core::global_config::global_config_dir()
        .ok()
        .and_then(|d| History::open(&d.join(crate::history::FILE_NAME)).ok());
    if history.is_none() {
        warnings.push("sin historial: no pude abrir gpu-history.db".into());
    }

    let probe: Option<Arc<Mutex<Box<dyn Probe>>>> = if !cfg.measure {
        None
    } else {
        match crate::probe::NvmlProbe::init() {
            Ok(p) => Some(Arc::new(Mutex::new(Box::new(p) as Box<dyn Probe>))),
            Err(e) => {
                warnings.push(format!("{e}; reparto por lo declarado, sin medir"));
                None
            }
        }
    };

    let listen: std::net::SocketAddr = cfg.listen.parse()?;
    if !listen.ip().is_loopback() {
        anyhow::bail!("el broker de GPU sólo escucha en loopback, no en {listen}");
    }
    let every = Duration::from_millis(cfg.sample_every_ms.max(250));
    let service = Service::new(cfg, history, warnings);

    let sampler = service.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        loop {
            tick.tick().await;
            sampler.sample(probe.clone()).await;
        }
    });

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("gpu: broker escuchando en http://{listen}");
    axum::serve(listener, router(service)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::GpuProcess;

    /// Una GPU de mentira con la memoria que se le diga.
    struct Fake(Arc<Mutex<u32>>, Vec<GpuProcess>);

    impl Probe for Fake {
        fn read(&mut self) -> Result<GpuReading, String> {
            Ok(GpuReading {
                name: "fake".into(),
                total_mb: 16_384,
                used_mb: *self.0.lock().unwrap(),
                temperature_c: Some(40),
                utilization_pct: Some(10),
                processes: self.1.clone(),
            })
        }
    }

    fn cfg() -> GpuConfig {
        GpuConfig {
            baseline_gb: 1.5,
            // Un Ollama que no existe: las pruebas no tocan el de verdad.
            ollama_url: "http://127.0.0.1:9".into(),
            ..GpuConfig::default()
        }
    }

    async fn serve(service: Service) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, router(service)).into_future());
        format!("http://{addr}")
    }

    fn body(owner: &str, gb: f64, priority: &str) -> serde_json::Value {
        serde_json::json!({ "owner": owner, "task": "t", "vram_gb": gb, "priority": priority, "preemptible": true })
    }

    #[tokio::test]
    async fn pedir_esperar_y_entrar_por_http() {
        let base = serve(Service::new(
            cfg(),
            Some(History::in_memory().unwrap()),
            vec![],
        ))
        .await;
        let http = reqwest::Client::new();

        let a: serde_json::Value = http
            .post(format!("{base}/v1/leases"))
            .json(&body("osac", 9.0, "normal"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(a["state"], "granted");

        let b: serde_json::Value = http
            .post(format!("{base}/v1/leases"))
            .json(&body("blender", 8.0, "low"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(b["state"], "queued");
        assert_eq!(b["position"], 1);

        let del = http
            .delete(format!(
                "{base}/v1/leases/{}",
                a["lease_id"].as_str().unwrap()
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(del.status(), 204);

        let polled: serde_json::Value = http
            .get(format!(
                "{base}/v1/leases/{}",
                b["lease_id"].as_str().unwrap()
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(polled["state"], "granted");

        let hist: Vec<serde_json::Value> = http
            .get(format!("{base}/v1/history"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0]["owner"], "osac");
    }

    #[tokio::test]
    async fn el_fichero_manda_sobre_la_prioridad_pedida() {
        let base = serve(Service::new(cfg(), None, vec![])).await;
        let http = reqwest::Client::new();
        http.post(format!("{base}/v1/leases"))
            .json(&body("osac", 1.0, "critical"))
            .send()
            .await
            .unwrap();
        let st: serde_json::Value = http
            .get(format!("{base}/v1/state"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(st["leases"][0]["priority"], "normal");
    }

    #[tokio::test]
    async fn un_lease_desconocido_es_404() {
        let base = serve(Service::new(cfg(), None, vec![])).await;
        let r = reqwest::Client::new()
            .post(format!("{base}/v1/leases/nope/yield"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
    }

    #[tokio::test]
    async fn un_intruso_aparece_marcado_con_su_sospechoso() {
        // El escenario del spec: algo usa la GPU sin lease. 1,5 de escritorio
        // más 3 GB que nadie ha pedido, y un python.exe sin lease.
        let used = Arc::new(Mutex::new(1_536 + 3_072));
        let probe: Arc<Mutex<Box<dyn Probe>>> = Arc::new(Mutex::new(Box::new(Fake(
            used.clone(),
            vec![
                GpuProcess {
                    pid: 10,
                    name: r"C:\Windows\explorer.exe".into(),
                },
                GpuProcess {
                    pid: 99,
                    name: r"C:\Python312\python.exe".into(),
                },
            ],
        ))));
        let service = Service::new(cfg(), None, vec![]);
        service.sample(Some(probe.clone())).await;

        let snap = service.snapshot();
        assert!(snap.intruders.alert);
        assert_eq!(snap.intruders.unexplained_mb, 3_072);
        assert_eq!(snap.intruders.suspects.len(), 1);
        assert_eq!(snap.intruders.suspects[0].pid, 99);

        // Cuando el intruso termina, la alarma se apaga sola.
        *used.lock().unwrap() = 1_536;
        service.sample(Some(probe)).await;
        assert!(!service.snapshot().intruders.alert);
    }

    #[tokio::test]
    async fn un_directo_abierto_es_modo_juego_y_no_un_intruso() {
        // TikTok LIVE Studio con 3 GB que ningun lease explica: son tuyos.
        let used = Arc::new(Mutex::new(1_536 + 3_072));
        let procs = Arc::new(Mutex::new(vec![
            GpuProcess {
                pid: 10,
                name: r"C:\Windows\explorer.exe".into(),
            },
            GpuProcess {
                pid: 77,
                name: r"C:\Program Files\TikTok LIVE Studio\0.63.0\TikTok LIVE Studio.exe".into(),
            },
        ]));
        let probe: Arc<Mutex<Box<dyn Probe>>> = Arc::new(Mutex::new(Box::new(Fake(
            used.clone(),
            procs.lock().unwrap().clone(),
        ))));
        let service = Service::new(cfg(), None, vec![]);
        service.sample(Some(probe)).await;

        let snap = service.snapshot();
        assert_eq!(snap.interactive, vec!["TikTok LIVE Studio.exe".to_string()]);
        assert!(!snap.intruders.alert, "tu directo no es un intruso");
        assert_eq!(
            snap.intruders.unexplained_mb, 3_072,
            "pero se sigue contando"
        );

        // Cerrado el directo, el modo juego se va solo.
        let closed: Arc<Mutex<Box<dyn Probe>>> = Arc::new(Mutex::new(Box::new(Fake(
            Arc::new(Mutex::new(1_536)),
            vec![procs.lock().unwrap()[0].clone()],
        ))));
        service.sample(Some(closed)).await;
        assert!(service.snapshot().interactive.is_empty());
    }

    #[tokio::test]
    async fn el_websocket_manda_la_foto_y_la_vuelve_a_mandar_al_cambiar() {
        use futures_util::StreamExt;
        let service = Service::new(cfg(), None, vec![]);
        let base = serve(service.clone()).await;
        let ws_url = base.replace("http://", "ws://") + "/v1/events";
        let (mut ws, _) = tokio_tungstenite::connect_async(ws_url).await.unwrap();

        let first = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let first: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(first["leases"].as_array().unwrap().len(), 0);

        reqwest::Client::new()
            .post(format!("{base}/v1/leases"))
            .json(&body("osac", 2.0, "normal"))
            .send()
            .await
            .unwrap();
        let second = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let second: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(second["leases"][0]["owner"], "osac");
    }

    #[tokio::test]
    async fn un_hook_de_claude_code_aparece_en_la_oficina() {
        let base = serve(Service::new(cfg(), None, vec![])).await;
        let http = reqwest::Client::new();
        let hook = serde_json::json!({
            "session_id": "abc", "cwd": r"F:\workspace\void-hq",
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash"
        });
        assert_eq!(
            http.post(format!("{base}/v1/agents/hook"))
                .json(&hook)
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
        let st: serde_json::Value = http
            .get(format!("{base}/v1/state"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(st["agents"][0]["status"], "waiting_approval");
        assert_eq!(st["agents"][0]["project"], "void-hq");
    }
}
