//! El icono de la bandeja: el broker de GPU de un vistazo.
//!
//! Un punto de color que dice si hay que mirar La Oficina:
//!
//! - **verde**: la GPU trabaja o descansa y nadie espera;
//! - **amarillo**: alguien hace cola, alguien está cediendo, o la cola está
//!   pausada / en modo entrenamiento (una decisión tuya que sigue puesta);
//! - **rojo**: hay memoria en uso que ningún lease explica (un intruso) o la
//!   tarjeta está llena;
//! - **gris**: el broker no contesta. No es un error —void-stack-mcp
//!   `--http` puede no estar corriendo—, pero tampoco es verde.
//!
//! El tooltip cuenta quién trabaja y quién espera; el menú del clic derecho
//! pausa la cola, pone el modo entrenamiento y abre La Oficina en void-hq.
//!
//! Lo que decide (color, texto, dibujo) es puro y se prueba aquí abajo; lo
//! que toca la bandeja y la red es un bucle fino alrededor.

use std::time::Duration;

use serde::Deserialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_opener::OpenerExt;

/// El mismo valor por defecto y la misma variable que el cliente Python.
pub const DEFAULT_BROKER: &str = "http://127.0.0.1:7410";
/// Dónde abrir La Oficina: void-hq en esta máquina.
pub const DEFAULT_OFFICE: &str = "http://localhost:3200/?view=office";
/// Cada cuánto se pregunta. El broker muestrea la GPU cada 2 s.
const POLL: Duration = Duration::from_secs(3);
/// Windows corta el tooltip de la bandeja en 127 caracteres.
const TOOLTIP_MAX: usize = 127;
const ICON_SIZE: u32 = 32;

// ── Lo que se lee de /v1/state (sólo lo necesario) ─────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Snap {
    pub total_mb: u32,
    pub baseline_mb: u32,
    pub accounted_mb: u32,
    pub gpu: Option<Gpu>,
    pub leases: Vec<Lease>,
    pub intruders: Intruders,
    pub training: bool,
    pub queue_paused: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Gpu {
    pub total_mb: u32,
    pub used_mb: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Lease {
    pub owner: String,
    pub vram_mb: u32,
    pub state: String,
    pub directive: String,
    pub position: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Intruders {
    pub unexplained_mb: u32,
    pub alert: bool,
}

// ── El color y el texto ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Green,
    Yellow,
    Red,
    Off,
}

impl Level {
    fn rgb(self) -> [u8; 3] {
        match self {
            Level::Green => [0x43, 0xd1, 0x9e],
            Level::Yellow => [0xff, 0xc2, 0x5a],
            Level::Red => [0xff, 0x4d, 0x5a],
            Level::Off => [0x8a, 0x90, 0xa0],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayStatus {
    pub level: Level,
    pub tooltip: String,
    pub training: bool,
    pub queue_paused: bool,
}

fn gb(mb: u32) -> String {
    let g = f64::from(mb) / 1024.0;
    let s = format!("{g:.1}");
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// Qué enseña la bandeja para una foto del broker (`None`: no contesta).
pub fn status(snap: Option<&Snap>) -> TrayStatus {
    let Some(s) = snap else {
        return TrayStatus {
            level: Level::Off,
            tooltip: "void-stack · el broker de GPU no contesta".into(),
            training: false,
            queue_paused: false,
        };
    };
    let (total, used) = match &s.gpu {
        Some(g) if g.total_mb > 0 => (g.total_mb, g.used_mb),
        _ => (s.total_mb, s.baseline_mb + s.accounted_mb),
    };
    let granted: Vec<&Lease> = s.leases.iter().filter(|l| l.state == "granted").collect();
    let mut queued: Vec<&Lease> = s.leases.iter().filter(|l| l.state == "queued").collect();
    queued.sort_by_key(|l| l.position.unwrap_or(u32::MAX));
    let yielding = granted.iter().any(|l| l.directive == "yield");
    let full = total > 0 && f64::from(used) / f64::from(total) >= 0.95;

    let level = if s.intruders.alert || full {
        Level::Red
    } else if !queued.is_empty() || yielding || s.training || s.queue_paused {
        Level::Yellow
    } else {
        Level::Green
    };

    let mut lines = vec![format!("GPU {}/{} GB", gb(used), gb(total))];
    let working: Vec<String> = granted.iter().map(|l| l.owner.clone()).collect();
    lines.push(if working.is_empty() {
        "nadie trabajando".into()
    } else {
        format!("trabajan: {}", working.join(", "))
    });
    if !queued.is_empty() {
        let q: Vec<String> = queued
            .iter()
            .map(|l| format!("{} {} GB", l.owner, gb(l.vram_mb)))
            .collect();
        lines.push(format!("en la fila: {}", q.join(", ")));
    }
    if s.intruders.alert {
        lines.push(format!(
            "intruso: {} GB sin lease",
            gb(s.intruders.unexplained_mb)
        ));
    }
    if s.training {
        lines.push("modo entrenamiento".into());
    }
    if s.queue_paused {
        lines.push("cola pausada".into());
    }
    TrayStatus {
        level,
        tooltip: clip(&lines.join("\n"), TOOLTIP_MAX),
        training: s.training,
        queue_paused: s.queue_paused,
    }
}

/// Corta por caracteres, no por bytes: "cola" lleva tildes alrededor.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Un punto de color con borde oscuro, en RGBA, suavizado en el canto.
pub fn dot_rgba(level: Level, size: u32) -> Vec<u8> {
    let [r, g, b] = level.rgb();
    let c = (size as f32 - 1.0) / 2.0;
    let outer = size as f32 / 2.0 - 1.0;
    let ring = outer - 2.5;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let alpha = (outer + 0.5 - d).clamp(0.0, 1.0);
            let (pr, pg, pb) = if d <= ring {
                (r, g, b)
            } else {
                (0x1a, 0x14, 0x22)
            };
            px.extend_from_slice(&[pr, pg, pb, (alpha * 255.0).round() as u8]);
        }
    }
    px
}

// ── La bandeja de verdad ───────────────────────────────────────────────────

fn broker_url() -> Option<String> {
    let raw = std::env::var("VOID_GPU_BROKER").unwrap_or_default();
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("off") {
        return None;
    }
    Some(if raw.is_empty() {
        DEFAULT_BROKER.to_owned()
    } else {
        raw.trim_end_matches('/').to_owned()
    })
}

fn office_url() -> String {
    std::env::var("VOID_HQ_OFFICE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OFFICE.to_owned())
}

/// Monta el icono y lanza el sondeo. Con `VOID_GPU_BROKER=off` no hay icono.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let Some(broker) = broker_url() else {
        tracing::info!("[bandeja] VOID_GPU_BROKER=off: sin icono de GPU");
        return Ok(());
    };
    let queue = CheckMenuItem::with_id(app, "queue", "Pausar la cola", true, false, None::<&str>)?;
    let training = CheckMenuItem::with_id(
        app,
        "training",
        "Modo entrenamiento",
        true,
        false,
        None::<&str>,
    )?;
    let office = MenuItem::with_id(app, "office", "Abrir La Oficina", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Mostrar void-stack", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &queue,
            &training,
            &PredefinedMenuItem::separator(app)?,
            &office,
            &show,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let first = status(None);
    let icon_px = dot_rgba(first.level, ICON_SIZE);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;

    let menu_broker = broker.clone();
    let menu_client = client.clone();
    let (menu_queue, menu_training) = (queue.clone(), training.clone());
    let tray = TrayIconBuilder::with_id("gpu")
        .icon(Image::new(&icon_px, ICON_SIZE, ICON_SIZE).to_owned())
        .tooltip(&first.tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref().to_owned();
            match id.as_str() {
                "queue" | "training" => {
                    // muda ya cambió la marca al hacer clic: su estado nuevo es
                    // lo que se pide. El siguiente sondeo pone la verdad.
                    let item = if id == "queue" {
                        &menu_queue
                    } else {
                        &menu_training
                    };
                    let checked = item.is_checked().unwrap_or(false);
                    let (path, body) = if id == "queue" {
                        ("/v1/queue", serde_json::json!({ "paused": checked }))
                    } else {
                        ("/v1/training", serde_json::json!({ "on": checked }))
                    };
                    let url = format!("{menu_broker}{path}");
                    let client = menu_client.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = client.post(&url).json(&body).send().await {
                            tracing::warn!("[bandeja] {url}: {e}");
                        }
                    });
                }
                "office" => {
                    if let Err(e) = app.opener().open_url(office_url(), None::<&str>) {
                        tracing::warn!("[bandeja] no se pudo abrir La Oficina: {e}");
                    }
                }
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    tauri::async_runtime::spawn(poll(tray, queue, training, client, broker));
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.webview_windows().values().next() {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

async fn poll(
    tray: TrayIcon,
    queue: CheckMenuItem<Wry>,
    training: CheckMenuItem<Wry>,
    client: reqwest::Client,
    broker: String,
) {
    let url = format!("{broker}/v1/state");
    let mut last: Option<TrayStatus> = None;
    loop {
        let snap = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.json::<Snap>().await.ok(),
            _ => None,
        };
        let now = status(snap.as_ref());
        if last.as_ref() != Some(&now) {
            if last.as_ref().map(|l| l.level) != Some(now.level) {
                let px = dot_rgba(now.level, ICON_SIZE);
                let _ = tray.set_icon(Some(Image::new(&px, ICON_SIZE, ICON_SIZE).to_owned()));
            }
            let _ = tray.set_tooltip(Some(&now.tooltip));
            let _ = queue.set_checked(now.queue_paused);
            let _ = training.set_checked(now.training);
            last = Some(now);
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(owner: &str, mb: u32, state: &str) -> Lease {
        Lease {
            owner: owner.into(),
            vram_mb: mb,
            state: state.into(),
            directive: "continue".into(),
            position: None,
        }
    }

    /// La foto del spec: 16 GB, 1,9 de escritorio, el Toro y el Cachicamo
    /// dentro y el Tucán esperando sus 9 GB.
    fn spec() -> Snap {
        Snap {
            total_mb: 16376,
            baseline_mb: 1946,
            accounted_mb: 8704,
            leases: vec![
                lease("maguetrader", 6656, "granted"),
                lease("terrain", 2048, "granted"),
                Lease {
                    position: Some(1),
                    ..lease("osac", 9216, "queued")
                },
            ],
            ..Snap::default()
        }
    }

    #[test]
    fn sin_broker_es_gris_y_lo_dice() {
        let s = status(None);
        assert_eq!(s.level, Level::Off);
        assert!(s.tooltip.contains("no contesta"));
    }

    #[test]
    fn nadie_esperando_es_verde() {
        let snap = Snap {
            total_mb: 16376,
            baseline_mb: 1946,
            leases: vec![lease("maguetrader", 6656, "granted")],
            ..Snap::default()
        };
        let s = status(Some(&snap));
        assert_eq!(s.level, Level::Green);
        assert!(
            s.tooltip
                .starts_with("GPU 1.9/16 GB\ntrabajan: maguetrader")
        );
    }

    #[test]
    fn alguien_en_la_fila_es_amarillo_y_sale_en_el_tooltip() {
        let s = status(Some(&spec()));
        assert_eq!(s.level, Level::Yellow);
        assert_eq!(
            s.tooltip,
            "GPU 10.4/16 GB\ntrabajan: maguetrader, terrain\nen la fila: osac 9 GB"
        );
    }

    #[test]
    fn la_fila_sale_en_su_orden() {
        let mut snap = spec();
        snap.leases.push(Lease {
            position: Some(0),
            ..lease("blender", 3072, "queued")
        });
        assert!(
            status(Some(&snap))
                .tooltip
                .contains("en la fila: blender 3 GB, osac 9 GB")
        );
    }

    #[test]
    fn ceder_entrenar_o_pausar_tambien_es_amarillo() {
        let mut yielding = Snap {
            leases: vec![lease("osac", 9216, "granted")],
            total_mb: 16376,
            ..Snap::default()
        };
        yielding.leases[0].directive = "yield".into();
        assert_eq!(status(Some(&yielding)).level, Level::Yellow);

        let training = Snap {
            training: true,
            total_mb: 16376,
            ..Snap::default()
        };
        let s = status(Some(&training));
        assert_eq!((s.level, s.training), (Level::Yellow, true));
        assert!(s.tooltip.contains("modo entrenamiento"));

        let paused = Snap {
            queue_paused: true,
            total_mb: 16376,
            ..Snap::default()
        };
        let s = status(Some(&paused));
        assert!(s.queue_paused && s.tooltip.ends_with("cola pausada"));
        assert!(s.tooltip.contains("nadie trabajando"));
    }

    #[test]
    fn un_intruso_es_rojo_aunque_haya_fila() {
        let mut snap = spec();
        snap.intruders = Intruders {
            unexplained_mb: 3137,
            alert: true,
        };
        let s = status(Some(&snap));
        assert_eq!(s.level, Level::Red);
        assert!(s.tooltip.contains("intruso: 3.1 GB sin lease"));
    }

    #[test]
    fn la_tarjeta_llena_es_roja_y_manda_la_medicion() {
        let snap = Snap {
            total_mb: 16376,
            gpu: Some(Gpu {
                total_mb: 16376,
                used_mb: 15800,
            }),
            ..Snap::default()
        };
        let s = status(Some(&snap));
        assert_eq!(s.level, Level::Red);
        assert!(s.tooltip.starts_with("GPU 15.4/16 GB"));
    }

    #[test]
    fn el_tooltip_cabe_en_la_bandeja_de_windows() {
        let mut snap = spec();
        for i in 0..20 {
            snap.leases.push(lease(
                &format!("proyecto-con-nombre-largo-{i}"),
                1024,
                "queued",
            ));
        }
        let s = status(Some(&snap));
        assert_eq!(s.tooltip.chars().count(), TOOLTIP_MAX);
        assert!(s.tooltip.ends_with('…'));
    }

    #[test]
    fn lee_la_foto_real_del_broker_ignorando_lo_demas() {
        // Recortada de /v1/state medido el 2026-09-30.
        let raw = r#"{"now_ms":1790777862344,"gpu":{"name":"NVIDIA GeForce RTX 4070 Ti SUPER",
            "total_mb":16376,"used_mb":1976,"temperature_c":37,"utilization_pct":2,"processes":[]},
            "total_mb":16376,"baseline_mb":1946,"accounted_mb":8704,"free_mb":5726,
            "leases":[{"id":"f2ad8b21-2","owner":"osac","task":"render smoke","vram_mb":9216,
            "priority":"normal","preemptible":true,"pid":null,"backend":"comfyui","state":"queued",
            "directive":"continue","reason":null,"requested_at_ms":1,"granted_at_ms":null,
            "last_seen_ms":1,"progress":null,"message":null,"held":false,"position":1}],
            "ollama":{"loaded_mb":0,"models":[]},
            "intruders":{"unexplained_mb":0,"alert":false,"suspects":[]},
            "agents":[],"training":false,"queue_paused":false,"warnings":[]}"#;
        let snap: Snap = serde_json::from_str(raw).expect("la foto del broker");
        let s = status(Some(&snap));
        assert_eq!(s.level, Level::Yellow);
        assert!(s.tooltip.contains("en la fila: osac 9 GB"));
    }

    #[test]
    fn el_punto_es_redondo_con_su_color_y_esquinas_transparentes() {
        let px = dot_rgba(Level::Green, 32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "la esquina no se pinta");
        assert_eq!(
            at(16, 16),
            &[0x43, 0xd1, 0x9e, 255],
            "el centro es del color"
        );
        assert_eq!(
            &at(16, 1)[..3],
            &[0x1a, 0x14, 0x22],
            "el canto es el borde oscuro"
        );
        assert_ne!(dot_rgba(Level::Red, 32), px);
    }

    #[test]
    fn gb_sin_decimales_de_sobra() {
        assert_eq!(gb(16384), "16");
        assert_eq!(gb(6656), "6.5");
    }
}
