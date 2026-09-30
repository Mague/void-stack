//! Lo que la GPU dice de sí misma, y quién la está usando sin permiso.
//!
//! ── Qué da NVML en esta máquina, medido el 2026-09-30 ───────────────────────
//! RTX 4070 Ti SUPER, driver 617.14, Windows en modo WDDM:
//!
//!   memoria total / usada ........ sí (16.376 / 1.821 MiB)
//!   temperatura, utilización ..... sí
//!   PIDs con contexto en la GPU .. sí, 24
//!   memoria POR proceso .......... NO: `[N/A]` en los 24
//!   cómputo vs gráficos .......... NO: los 24 salen `C+G`, incluidos
//!                                  Explorer, Chrome, WhatsApp y Claude
//!
//! Así que "un proceso en la GPU sin lease" no es un intruso: es el escritorio.
//! Lo que sí delata a un intruso es la MEMORIA: VRAM en uso que ningún lease
//! explica (`Broker::unexplained_mb`). Eso se marca; y para decir quién puede
//! ser, se cruzan los procesos contra una lista de ejecutables pesados.

use std::collections::BTreeSet;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GpuProcess {
    pub pid: u32,
    /// Ruta o nombre del ejecutable, tal cual lo da NVML.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GpuReading {
    pub name: String,
    pub total_mb: u32,
    pub used_mb: u32,
    pub temperature_c: Option<u32>,
    pub utilization_pct: Option<u32>,
    pub processes: Vec<GpuProcess>,
}

pub trait Probe: Send {
    fn read(&mut self) -> Result<GpuReading, String>;
}

/// El nombre del ejecutable sin ruta y en minúsculas: `C:\…\python.exe` → `python.exe`.
pub fn exe_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

fn watched(name: &str, watch: &[String]) -> bool {
    let exe = exe_name(name);
    watch.iter().any(|w| {
        let w = w.to_ascii_lowercase();
        exe == w || (!w.contains('.') && exe.contains(&w))
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Intruders {
    /// VRAM en uso que ningún lease explica.
    pub unexplained_mb: u32,
    /// Hay memoria sin explicar por encima del umbral: eso es la alarma.
    pub alert: bool,
    /// Procesos vigilados en la GPU que no tienen lease: los sospechosos.
    pub suspects: Vec<GpuProcess>,
}

/// Quién está usando la GPU sin lease.
///
/// La alarma sale de la memoria, no de la lista de procesos (ver cabecera).
/// Los sospechosos se nombran para que la alarma diga algo útil, pero un
/// `python.exe` en la GPU sin memoria que lo delate no hace saltar nada: en
/// Windows medio escritorio tiene un contexto abierto.
pub fn intruders(
    processes: &[GpuProcess],
    leased_pids: &BTreeSet<u32>,
    watch: &[String],
    unexplained_mb: u32,
    min_mb: u32,
) -> Intruders {
    let suspects = processes
        .iter()
        .filter(|p| !leased_pids.contains(&p.pid) && watched(&p.name, watch))
        .cloned()
        .collect();
    Intruders {
        unexplained_mb,
        alert: min_mb > 0 && unexplained_mb >= min_mb,
        suspects,
    }
}

/// La GPU de verdad, vía `nvml.dll`.
pub struct NvmlProbe {
    nvml: nvml_wrapper::Nvml,
}

impl NvmlProbe {
    /// Falla si no hay driver NVIDIA. El broker sigue sin mediciones: repartir
    /// por lo declarado es peor que medir, pero mejor que no repartir.
    pub fn init() -> Result<Self, String> {
        nvml_wrapper::Nvml::init()
            .map(|nvml| Self { nvml })
            .map_err(|e| format!("NVML no disponible: {e}"))
    }
}

impl Probe for NvmlProbe {
    fn read(&mut self) -> Result<GpuReading, String> {
        use nvml_wrapper::enum_wrappers::device::TemperatureSensor;

        let device = self.nvml.device_by_index(0).map_err(|e| e.to_string())?;
        let memory = device.memory_info().map_err(|e| e.to_string())?;

        // Cómputo y gráficos juntos: en WDDM la distinción no existe (todo es
        // `C+G`) y en Linux conviene no perderse a ninguno.
        let mut pids = BTreeSet::new();
        for list in [
            device.running_compute_processes(),
            device.running_graphics_processes(),
        ]
        .into_iter()
        .flatten()
        {
            pids.extend(list.into_iter().map(|p| p.pid));
        }
        let processes = pids
            .into_iter()
            .map(|pid| GpuProcess {
                pid,
                name: self.nvml.sys_process_name(pid, 256).unwrap_or_default(),
            })
            .collect();

        Ok(GpuReading {
            name: device.name().unwrap_or_default(),
            total_mb: (memory.total / (1024 * 1024)) as u32,
            used_mb: (memory.used / (1024 * 1024)) as u32,
            temperature_c: device.temperature(TemperatureSensor::Gpu).ok(),
            utilization_pct: device.utilization_rates().ok().map(|u| u.gpu),
            processes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str) -> GpuProcess {
        GpuProcess {
            pid,
            name: name.into(),
        }
    }

    fn watch() -> Vec<String> {
        crate::config::GpuConfig::default().intruder_watch
    }

    /// Los procesos que había de verdad en la GPU el 2026-09-30, con sólo el
    /// escritorio abierto. Ninguno es un intruso.
    fn escritorio() -> Vec<GpuProcess> {
        vec![
            p(10468, r"C:\Windows\explorer.exe"),
            p(
                37124,
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            ),
            p(25332, r"C:\Program Files\WindowsApps\WhatsApp.Root.exe"),
            p(3324, r"C:\Users\x\AppData\Local\Programs\Warp\warp.exe"),
            p(43940, r"C:\Program Files\WindowsApps\Claude\app\claude.exe"),
        ]
    }

    #[test]
    fn el_escritorio_en_la_gpu_no_es_un_intruso() {
        let r = intruders(&escritorio(), &BTreeSet::new(), &watch(), 0, 1024);
        assert!(!r.alert);
        assert!(r.suspects.is_empty());
    }

    #[test]
    fn memoria_que_nadie_explica_hace_saltar_la_alarma() {
        let r = intruders(&escritorio(), &BTreeSet::new(), &watch(), 3_000, 1024);
        assert!(r.alert);
    }

    #[test]
    fn por_debajo_del_umbral_es_ruido() {
        let r = intruders(&escritorio(), &BTreeSet::new(), &watch(), 500, 1024);
        assert!(!r.alert);
    }

    #[test]
    fn nombra_al_sospechoso_por_su_ejecutable() {
        let mut procs = escritorio();
        procs.push(p(999, r"C:\Python312\python.exe"));
        let r = intruders(&procs, &BTreeSet::new(), &watch(), 3_000, 1024);
        assert_eq!(r.suspects, vec![p(999, r"C:\Python312\python.exe")]);
    }

    #[test]
    fn quien_tiene_lease_no_es_sospechoso() {
        let procs = vec![p(999, r"C:\Python312\python.exe")];
        let leased: BTreeSet<u32> = [999].into();
        assert!(
            intruders(&procs, &leased, &watch(), 0, 1024)
                .suspects
                .is_empty()
        );
    }

    #[test]
    fn un_patron_sin_extension_casa_por_trozo() {
        // `comfyui` en la lista casa con cualquier ejecutable que lo contenga.
        let procs = vec![p(7, r"D:\ComfyUI\ComfyUI_windows_portable.exe")];
        assert_eq!(
            intruders(&procs, &BTreeSet::new(), &watch(), 0, 1024)
                .suspects
                .len(),
            1
        );
    }

    #[test]
    fn el_nombre_del_ejecutable_sale_de_rutas_de_los_dos_sistemas() {
        assert_eq!(exe_name(r"C:\Windows\explorer.exe"), "explorer.exe");
        assert_eq!(exe_name("/usr/bin/python3"), "python3");
        assert_eq!(exe_name("ollama.exe"), "ollama.exe");
    }

    /// Lee la GPU de verdad. `cargo test -p void-stack-gpu -- --ignored`
    #[test]
    #[ignore = "necesita una GPU NVIDIA"]
    fn lee_la_gpu_de_verdad() {
        let reading = NvmlProbe::init().unwrap().read().unwrap();
        println!("{reading:#?}");
        assert!(reading.total_mb > 0);
    }
}
