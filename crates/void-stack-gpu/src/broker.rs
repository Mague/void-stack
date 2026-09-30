//! El núcleo del broker: una GPU, muchos que la quieren.
//!
//! ── Puro, a propósito ───────────────────────────────────────────────────────
//! Aquí no hay red, ni reloj, ni NVML. El tiempo entra como argumento (`now`),
//! las mediciones entran por `set_measured`/`set_ollama`, y lo que haya que
//! HACER sale como `Effect`. La capa asíncrona (server.rs) mide, llama a
//! Ollama y escribe el historial. Así cada escenario del spec es una prueba
//! determinista de milisegundos.
//!
//! ── Cómo se decide si cabe ──────────────────────────────────────────────────
//! Medido el 2026-09-30 sobre esta máquina (Windows, driver 617.14, WDDM):
//! NVML da la VRAM TOTAL usada (1.821 MiB con sólo el escritorio abierto) pero
//! NO por proceso: `used_gpu_memory` sale `[N/A]` para los 24 procesos, y los
//! 24 son `C+G` —no distingue cómputo de gráficos—. Así que no se puede
//! atribuir memoria a quien la usa. Se lleva la cuenta por lo DECLARADO y se
//! contrasta con lo MEDIDO:
//!
//!   contado  = Σ leases concedidos (+ Ollama si nadie lo está usando)
//!   extra    = medido − base_del_escritorio − contado   (≥ 0)
//!   libre    = total − base − contado − extra
//!
//! `extra` es memoria que ningún lease explica: un intruso, o alguien usando
//! más de lo que pidió. En los dos casos, no está libre.

use std::collections::BTreeSet;

use crate::model::{
    Directive, Effect, HistoryEntry, Lease, LeaseRequest, LeaseState, Outcome, Priority,
};

/// Los números que gobiernan el reparto. Salen de `gpu.toml`.
#[derive(Debug, Clone)]
pub struct Limits {
    pub total_mb: u32,
    /// Lo que usa el escritorio sin nada pesado encima. Medido: 1.821 MiB.
    pub baseline_mb: u32,
    /// Sin latido en este tiempo, un lease concedido se da por muerto.
    pub lease_timeout_ms: u64,
    /// Sin preguntar en este tiempo, uno en la fila se da por abandonado.
    pub queue_timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            total_mb: 16_376,
            baseline_mb: 1_900,
            lease_timeout_ms: 60_000,
            queue_timeout_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Acquired {
    pub lease_id: String,
    pub state: LeaseState,
    /// Posición en la fila, empezando en 1. Nula si ya entró.
    pub position: Option<usize>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BrokerError {
    #[error("no conozco el lease {0}: caducó o el broker se reinició")]
    UnknownLease(String),
    #[error("{0} no se puede pausar: no declaró que pudiera ceder (preemptible)")]
    NotPreemptible(String),
}

#[derive(Debug, Clone, Default)]
struct OllamaState {
    loaded_mb: u32,
    models: Vec<String>,
    /// Ya se pidió descargarlo y todavía no llegó la medición que lo confirme.
    /// Sin esto se pediría en cada vuelta del planificador.
    unloading: bool,
}

pub struct Broker {
    limits: Limits,
    leases: Vec<Lease>,
    boot: String,
    next: u64,
    queue_paused: bool,
    training: bool,
    /// Lo que estás usando tú ahora mismo (un juego, un directo): el modo
    /// juego. Mientras haya algo, la GPU es para ti y para lo crítico.
    interactive: Vec<String>,
    /// Dueños/tareas que el usuario pausó. Sobrevive a que el cliente suelte y
    /// vuelva a pedir: pausar es retener a ESE trabajo, no a un id.
    held: BTreeSet<String>,
    measured_used_mb: Option<u32>,
    ollama: OllamaState,
    /// Espacio que se está liberando para alguien en concreto. Ver `schedule`.
    reserved_for: Option<String>,
    effects: Vec<Effect>,
    history: Vec<HistoryEntry>,
}

fn key(owner: &str, task: &str) -> String {
    format!("{owner}\u{1f}{task}")
}

impl Broker {
    pub fn new(limits: Limits, boot: impl Into<String>) -> Self {
        Self {
            limits,
            leases: Vec::new(),
            boot: boot.into(),
            next: 0,
            queue_paused: false,
            training: false,
            interactive: Vec::new(),
            held: BTreeSet::new(),
            measured_used_mb: None,
            ollama: OllamaState::default(),
            reserved_for: None,
            effects: Vec::new(),
            history: Vec::new(),
        }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    // ── Mediciones ─────────────────────────────────────────────────────────

    pub fn set_measured(&mut self, used_mb: Option<u32>, total_mb: Option<u32>, now: u64) {
        self.measured_used_mb = used_mb;
        if let Some(total) = total_mb.filter(|t| *t > 0) {
            self.limits.total_mb = total;
        }
        self.schedule(now);
    }

    pub fn set_ollama(&mut self, loaded_mb: u32, models: Vec<String>, now: u64) {
        if loaded_mb == 0 {
            self.ollama.unloading = false;
        }
        self.ollama.loaded_mb = loaded_mb;
        self.ollama.models = models;
        // Ollama carga modelos a demanda: si alguien le habla sin lease en
        // pleno modo juego, vuelve a ocupar memoria. Se descarga otra vez; con
        // lease (`backend = "ollama"`) es de ese cliente y se respeta.
        if self.reserved_for_critical()
            && self.ollama.loaded_mb > 0
            && !self.ollama_in_use()
            && !self.ollama.unloading
        {
            let reason = self.reservation_reason();
            self.unload_ollama(&reason);
        }
        self.schedule(now);
    }

    // ── Lo que llama un cliente ────────────────────────────────────────────

    pub fn acquire(&mut self, req: LeaseRequest, now: u64) -> Acquired {
        self.next += 1;
        let id = format!("{}-{}", self.boot, self.next);
        let held = self.held.contains(&key(&req.owner, &req.task));
        self.leases.push(Lease {
            id: id.clone(),
            vram_mb: req.vram_mb(),
            owner: req.owner,
            task: req.task,
            priority: req.priority,
            preemptible: req.preemptible,
            pid: req.pid,
            backend: req.backend,
            state: LeaseState::Queued,
            directive: Directive::Continue,
            reason: None,
            requested_at_ms: now,
            granted_at_ms: None,
            last_seen_ms: now,
            progress: None,
            message: None,
            held,
        });
        self.schedule(now);
        self.effects.push(Effect::Changed);
        self.acquired(&id).expect("recién creado")
    }

    /// Un cliente en la fila pregunta si ya le toca. Cuenta como latido.
    pub fn poll(&mut self, id: &str, now: u64) -> Result<Acquired, BrokerError> {
        self.touch(id, now)?;
        self.acquired(id)
            .ok_or_else(|| BrokerError::UnknownLease(id.to_owned()))
    }

    pub fn heartbeat(
        &mut self,
        id: &str,
        progress: Option<f32>,
        message: Option<String>,
        now: u64,
    ) -> Result<Directive, BrokerError> {
        let lease = self.find_mut(id)?;
        lease.last_seen_ms = now;
        if let Some(p) = progress {
            lease.progress = Some(p.clamp(0.0, 1.0));
        }
        if message.is_some() {
            lease.message = message;
        }
        let directive = lease.directive;
        self.effects.push(Effect::Changed);
        Ok(directive)
    }

    /// "¿Sigo o cedo?". Lo pregunta quien puede soltar entre fragmentos.
    pub fn yield_point(&mut self, id: &str, now: u64) -> Result<Directive, BrokerError> {
        self.touch(id, now)?;
        Ok(self.find(id)?.directive)
    }

    pub fn release(&mut self, id: &str, now: u64) -> Result<(), BrokerError> {
        let lease = self.find(id)?;
        let outcome = match lease.directive {
            Directive::Yield => Outcome::Yielded,
            Directive::Cancel => Outcome::Cancelled,
            Directive::Continue => Outcome::Released,
        };
        self.end(id, outcome, now);
        self.schedule(now);
        Ok(())
    }

    /// Caduca a quien dejó de dar señales y vuelve a repartir.
    pub fn tick(&mut self, now: u64) {
        let dead: Vec<(String, Outcome)> = self
            .leases
            .iter()
            .filter(|l| {
                let limit = match l.state {
                    LeaseState::Granted => self.limits.lease_timeout_ms,
                    LeaseState::Queued => self.limits.queue_timeout_ms,
                };
                now.saturating_sub(l.last_seen_ms) > limit
            })
            .map(|l| (l.id.clone(), Outcome::Expired))
            .collect();
        for (id, outcome) in dead {
            self.end(&id, outcome, now);
        }
        self.schedule(now);
    }

    // ── Lo que hace el usuario desde La Oficina ─────────────────────────────

    /// Pausa: le pide ceder y lo retiene en la fila hasta que se reanude.
    pub fn hold(&mut self, id: &str, now: u64) -> Result<(), BrokerError> {
        let lease = self.find(id)?;
        if !lease.preemptible && lease.state == LeaseState::Granted {
            return Err(BrokerError::NotPreemptible(lease.owner.clone()));
        }
        self.held.insert(key(&lease.owner, &lease.task));
        let lease = self.find_mut(id)?;
        lease.held = true;
        if lease.state == LeaseState::Granted {
            lease.directive = Directive::Yield;
            lease.reason = Some("pausado desde La Oficina".into());
        }
        self.schedule(now);
        Ok(())
    }

    pub fn resume(&mut self, id: &str, now: u64) -> Result<(), BrokerError> {
        let lease = self.find(id)?;
        self.held.remove(&key(&lease.owner, &lease.task));
        let lease = self.find_mut(id)?;
        lease.held = false;
        self.schedule(now);
        Ok(())
    }

    pub fn set_priority(
        &mut self,
        id: &str,
        priority: Priority,
        now: u64,
    ) -> Result<(), BrokerError> {
        self.find_mut(id)?.priority = priority;
        self.schedule(now);
        Ok(())
    }

    /// Cancelar un lease en la fila lo quita; uno concedido recibe `Cancel` y
    /// es el cliente quien para — el broker no mata procesos.
    pub fn cancel(&mut self, id: &str, now: u64) -> Result<(), BrokerError> {
        let lease = self.find_mut(id)?;
        if lease.state == LeaseState::Queued {
            self.end(id, Outcome::Cancelled, now);
        } else {
            lease.directive = Directive::Cancel;
            lease.reason = Some("cancelado desde La Oficina".into());
        }
        self.schedule(now);
        Ok(())
    }

    /// Modo entrenamiento: la GPU para lo crítico. Lo demás cede y espera.
    pub fn set_training(&mut self, on: bool, now: u64) {
        let was = self.reserved_for_critical();
        self.training = on;
        if on && !was {
            self.clear_for_critical("modo entrenamiento");
        }
        self.schedule(now);
    }

    /// Modo juego: lo que estás usando tú (un juego, TikTok LIVE Studio, OBS).
    ///
    /// Lo decide el muestreador con las reglas `interactive` de gpu.toml sobre
    /// los procesos que hay en la GPU. Mientras dure, igual que el modo
    /// entrenamiento: nada nuevo entra salvo lo crítico, lo interrumpible cede
    /// en su siguiente punto seguro y Ollama se descarga. Al cerrar el juego
    /// todo vuelve solo, sin que nadie pulse nada.
    ///
    /// Sólo reparte memoria y turnos: no puede frenar a quien ya está dentro y
    /// no es interrumpible (el erosionador del terreno, por ejemplo).
    pub fn set_interactive(&mut self, apps: Vec<String>, now: u64) {
        if apps == self.interactive {
            return;
        }
        let was = self.reserved_for_critical();
        self.interactive = apps;
        if !was && self.reserved_for_critical() {
            let reason = self.reservation_reason();
            self.clear_for_critical(&reason);
        }
        self.schedule(now);
    }

    pub fn interactive(&self) -> &[String] {
        &self.interactive
    }

    /// Entrenamiento o juego: la GPU queda para lo crítico.
    fn reserved_for_critical(&self) -> bool {
        self.training || !self.interactive.is_empty()
    }

    fn reservation_reason(&self) -> String {
        match self.interactive.first() {
            Some(app) => format!("modo juego: {app}"),
            None => "modo entrenamiento".into(),
        }
    }

    /// Pide ceder a lo no crítico interrumpible y descarga a Ollama si nadie
    /// la está usando por su lease.
    fn clear_for_critical(&mut self, reason: &str) {
        for lease in self.leases.iter_mut().filter(|l| {
            l.state == LeaseState::Granted
                && l.priority != Priority::Critical
                && l.preemptible
                && l.directive == Directive::Continue
        }) {
            lease.directive = Directive::Yield;
            lease.reason = Some(reason.to_owned());
        }
        if self.ollama.loaded_mb > 0 && !self.ollama_in_use() {
            self.unload_ollama(reason);
        }
    }

    pub fn set_queue_paused(&mut self, paused: bool, now: u64) {
        self.queue_paused = paused;
        self.schedule(now);
    }

    /// El botón "descargar Ollama" de la ficha.
    pub fn request_ollama_unload(&mut self, now: u64) {
        if self.ollama_in_use() {
            return;
        }
        self.unload_ollama("pedido desde La Oficina");
        self.schedule(now);
    }

    // ── Lo que sale ─────────────────────────────────────────────────────────

    pub fn drain_effects(&mut self) -> Vec<Effect> {
        let mut out = std::mem::take(&mut self.effects);
        // "Changed" repetido no dice más que uno.
        let changed = out.contains(&Effect::Changed);
        out.retain(|e| *e != Effect::Changed);
        if changed {
            out.push(Effect::Changed);
        }
        out
    }

    pub fn drain_history(&mut self) -> Vec<HistoryEntry> {
        std::mem::take(&mut self.history)
    }

    pub fn leases(&self) -> &[Lease] {
        &self.leases
    }

    pub fn training(&self) -> bool {
        self.training
    }

    pub fn queue_paused(&self) -> bool {
        self.queue_paused
    }

    pub fn ollama_loaded_mb(&self) -> u32 {
        self.ollama.loaded_mb
    }

    pub fn ollama_models(&self) -> &[String] {
        &self.ollama.models
    }

    pub fn measured_used_mb(&self) -> Option<u32> {
        self.measured_used_mb
    }

    /// Lo que se cuenta como ocupado por leases (y Ollama, si nadie lo usa).
    pub fn accounted_mb(&self) -> u32 {
        let leased: u32 = self
            .leases
            .iter()
            .filter(|l| l.state == LeaseState::Granted)
            .map(|l| l.vram_mb)
            .sum();
        let ollama = if self.ollama_in_use() {
            0
        } else {
            self.ollama.loaded_mb
        };
        leased + ollama
    }

    /// Memoria en uso que ningún lease explica. Ver la cabecera.
    pub fn unexplained_mb(&self) -> u32 {
        self.measured_used_mb
            .map(|used| used.saturating_sub(self.limits.baseline_mb + self.accounted_mb()))
            .unwrap_or(0)
    }

    pub fn free_mb(&self) -> u32 {
        self.limits
            .total_mb
            .saturating_sub(self.limits.baseline_mb + self.accounted_mb() + self.unexplained_mb())
    }

    // ── El reparto ──────────────────────────────────────────────────────────

    /// Recorre la fila por prioridad y llegada, y concede lo que cabe.
    ///
    /// Dos reglas que no son obvias y que salen del spec:
    ///
    /// 1. **Se puede adelantar** si cabe. MagueTrader con 6,5 GB y OSAC
    ///    pidiendo 9: OSAC espera; el terreno pide 2 y entra aunque OSAC
    ///    llegara antes. Esperar a que se vacíe sin razón sería tirar GPU.
    ///
    /// 2. **Salvo si se está haciendo sitio para alguien.** Cuando llega algo
    ///    que obliga a otros a ceder, ese hueco queda RESERVADO para él: si
    ///    no, lo que suelta OSAC al ceder se lo llevaría el primero de menor
    ///    prioridad que pasara por delante — una inversión de prioridad.
    fn schedule(&mut self, now: u64) {
        if self.queue_paused {
            return;
        }

        // Si ya se fue quien reservaba, la reserva no protege a nadie.
        if let Some(id) = &self.reserved_for {
            let still = self
                .leases
                .iter()
                .any(|l| &l.id == id && l.state == LeaseState::Queued);
            if !still {
                self.reserved_for = None;
            }
        }

        let mut queue: Vec<(Priority, u64, String)> = self
            .leases
            .iter()
            .filter(|l| l.state == LeaseState::Queued)
            .map(|l| (l.priority, l.requested_at_ms, l.id.clone()))
            .collect();
        queue.sort();

        for (priority, _, id) in queue {
            let Some(lease) = self.leases.iter().find(|l| l.id == id) else {
                continue;
            };
            if lease.held {
                continue;
            }
            if self.reserved_for_critical() && priority != Priority::Critical {
                continue;
            }
            let need = lease.vram_mb;
            let reserved = match &self.reserved_for {
                Some(r) if r != &id => self
                    .leases
                    .iter()
                    .find(|l| &l.id == r)
                    .map(|l| l.vram_mb)
                    .unwrap_or(0),
                _ => 0,
            };

            if need + reserved <= self.free_mb() {
                self.grant(&id, now);
                continue;
            }

            // No cabe. ¿Puede hacerse sitio? Sólo para alguien que esté por
            // encima de a quien se le pide ceder.
            if self.reserved_for.is_none() && self.make_room(&id, priority, need) {
                self.reserved_for = Some(id.clone());
            }
        }
    }

    /// Pide ceder a quien esté por debajo, empezando por lo más barato.
    /// Devuelve true si inició algo que liberará memoria.
    fn make_room(&mut self, for_id: &str, priority: Priority, need: u32) -> bool {
        let missing = need.saturating_sub(self.free_mb());
        let mut freeing = 0u32;
        let mut started = false;
        let owner = self
            .leases
            .iter()
            .find(|l| l.id == for_id)
            .map(|l| l.owner.clone())
            .unwrap_or_default();

        // 1. Ollama ocioso: lo más barato de devolver.
        if self.ollama.loaded_mb > 0 && !self.ollama_in_use() && priority < Priority::Opportunistic
        {
            if !self.ollama.unloading {
                self.unload_ollama(&format!("{owner} necesita la memoria"));
            }
            freeing += self.ollama.loaded_mb;
            started = true;
        }

        // 2. Los que pueden ceder, del de menos prioridad al de más.
        let mut victims: Vec<(Priority, u64, String)> = self
            .leases
            .iter()
            .filter(|l| {
                l.state == LeaseState::Granted
                    && l.preemptible
                    && l.priority > priority
                    && l.directive == Directive::Continue
            })
            .map(|l| (l.priority, l.granted_at_ms.unwrap_or(0), l.id.clone()))
            .collect();
        // Menor prioridad primero; entre iguales, el que entró último.
        victims.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

        for (_, _, victim) in victims {
            if freeing >= missing {
                break;
            }
            if let Some(v) = self.leases.iter_mut().find(|l| l.id == victim) {
                v.directive = Directive::Yield;
                v.reason = Some(format!("cede a {owner}"));
                freeing += v.vram_mb;
                started = true;
                self.effects.push(Effect::Changed);
            }
        }
        started
    }

    fn grant(&mut self, id: &str, now: u64) {
        if let Some(lease) = self.leases.iter_mut().find(|l| l.id == id) {
            lease.state = LeaseState::Granted;
            lease.granted_at_ms = Some(now);
            lease.last_seen_ms = now;
            lease.directive = Directive::Continue;
            lease.reason = None;
        }
        if self.reserved_for.as_deref() == Some(id) {
            self.reserved_for = None;
        }
        self.effects.push(Effect::Changed);
    }

    fn end(&mut self, id: &str, outcome: Outcome, now: u64) {
        let Some(pos) = self.leases.iter().position(|l| l.id == id) else {
            return;
        };
        let lease = self.leases.remove(pos);
        self.history.push(HistoryEntry {
            lease_id: lease.id,
            owner: lease.owner,
            task: lease.task,
            vram_mb: lease.vram_mb,
            priority: lease.priority,
            requested_at_ms: lease.requested_at_ms,
            granted_at_ms: lease.granted_at_ms,
            ended_at_ms: now,
            outcome,
            turns: 1,
        });
        self.effects.push(Effect::Changed);
    }

    fn unload_ollama(&mut self, reason: &str) {
        self.ollama.unloading = true;
        self.effects.push(Effect::UnloadOllama {
            reason: reason.to_owned(),
        });
    }

    /// Hay un lease concedido que usa Ollama como motor: no se le toca.
    fn ollama_in_use(&self) -> bool {
        self.leases.iter().any(|l| {
            l.state == LeaseState::Granted
                && l.backend
                    .as_deref()
                    .is_some_and(|b| b.eq_ignore_ascii_case("ollama"))
        })
    }

    fn touch(&mut self, id: &str, now: u64) -> Result<(), BrokerError> {
        self.find_mut(id)?.last_seen_ms = now;
        Ok(())
    }

    fn find(&self, id: &str) -> Result<&Lease, BrokerError> {
        self.leases
            .iter()
            .find(|l| l.id == id)
            .ok_or_else(|| BrokerError::UnknownLease(id.to_owned()))
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Lease, BrokerError> {
        self.leases
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| BrokerError::UnknownLease(id.to_owned()))
    }

    fn acquired(&self, id: &str) -> Option<Acquired> {
        let lease = self.leases.iter().find(|l| l.id == id)?;
        let position = match lease.state {
            LeaseState::Granted => None,
            LeaseState::Queued => {
                let mut queue: Vec<(Priority, u64, &str)> = self
                    .leases
                    .iter()
                    .filter(|l| l.state == LeaseState::Queued)
                    .map(|l| (l.priority, l.requested_at_ms, l.id.as_str()))
                    .collect();
                queue.sort();
                queue.iter().position(|(_, _, q)| *q == id).map(|p| p + 1)
            }
        };
        Some(Acquired {
            lease_id: id.to_owned(),
            state: lease.state,
            position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::gb_to_mb;

    /// La máquina del spec: 16 GB y un escritorio que come 1,5.
    fn office() -> Broker {
        Broker::new(
            Limits {
                total_mb: gb_to_mb(16.0),
                baseline_mb: gb_to_mb(1.5),
                lease_timeout_ms: 60_000,
                queue_timeout_ms: 60_000,
            },
            "t",
        )
    }

    fn req(owner: &str, gb: f64, priority: Priority, preemptible: bool) -> LeaseRequest {
        LeaseRequest {
            owner: owner.into(),
            task: format!("{owner}-task"),
            vram_gb: gb,
            priority,
            preemptible,
            pid: None,
            backend: None,
        }
    }

    fn state(b: &Broker, id: &str) -> LeaseState {
        b.leases().iter().find(|l| l.id == id).unwrap().state
    }

    // ── Los escenarios del spec, uno por uno ────────────────────────────────

    #[test]
    fn osac_espera_detras_de_maguetrader() {
        let mut b = office();
        let mt = b.acquire(req("maguetrader", 6.5, Priority::Critical, false), 0);
        assert_eq!(mt.state, LeaseState::Granted);

        // 1,5 + 6,5 + 9 = 17 > 16: no cabe.
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 10);
        assert_eq!(osac.state, LeaseState::Queued);
        assert_eq!(osac.position, Some(1));
    }

    #[test]
    fn el_terreno_entra_aunque_osac_llegara_antes() {
        // Adelantar si cabe: dejar la GPU vacía esperando a OSAC sería
        // tirarla. Es exactamente lo que pide el spec.
        let mut b = office();
        b.acquire(req("maguetrader", 6.5, Priority::Critical, false), 0);
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 10);
        let terrain = b.acquire(req("terrain", 2.0, Priority::Low, false), 20);

        assert_eq!(terrain.state, LeaseState::Granted);
        assert_eq!(state(&b, &osac.lease_id), LeaseState::Queued);
    }

    #[test]
    fn llega_algo_critico_ollama_se_descarga_y_osac_cede() {
        let mut b = office();
        b.set_ollama(gb_to_mb(4.0), vec!["qwen2.5vl:7b".into()], 0);
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        assert_eq!(osac.state, LeaseState::Granted);
        b.drain_effects();

        // Libre: 16 − 1,5 − 4 (Ollama) − 9 = 1,5. Llega algo crítico de 6.
        let crit = b.acquire(req("maguetrader", 6.0, Priority::Critical, false), 10);
        assert_eq!(crit.state, LeaseState::Queued);

        // Ollama se descarga…
        let effects = b.drain_effects();
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::UnloadOllama { .. }))
        );
        // …y OSAC recibe "cede" en su próximo yield_point.
        assert_eq!(b.yield_point(&osac.lease_id, 11), Ok(Directive::Yield));

        // Con Ollama fuera (4 GB) aún no caben 6 con OSAC dentro: sigue en fila.
        b.set_ollama(0, vec![], 12);
        assert_eq!(state(&b, &crit.lease_id), LeaseState::Queued);

        // OSAC suelta en su punto seguro: entra lo crítico.
        b.release(&osac.lease_id, 13).unwrap();
        assert_eq!(state(&b, &crit.lease_id), LeaseState::Granted);

        // Y en el historial queda que OSAC cedió, no que terminó.
        let h = b.drain_history();
        assert_eq!(h[0].outcome, Outcome::Yielded);
    }

    #[test]
    fn lo_que_suelta_osac_no_se_lo_lleva_otro_por_delante() {
        // La reserva. Números elegidos para que colarse SÍ haga daño: la
        // primera versión de esta prueba pasaba igual sin reserva, porque al
        // final a todos les cabía.
        //
        //   OSAC 9 dentro → libre 5,5. Llega lo crítico pidiendo 12: OSAC cede.
        //   Llega el terreno pidiendo 4: cabe en los 5,5 AHORA MISMO.
        //   Sin reserva entra, y cuando OSAC suelta quedan 10,5 < 12:
        //   lo crítico se queda fuera por culpa de alguien de prioridad baja.
        let mut b = office();
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        let crit = b.acquire(req("maguetrader", 12.0, Priority::Critical, false), 10);
        let terrain = b.acquire(req("terrain", 4.0, Priority::Low, false), 20);
        assert_eq!(
            state(&b, &terrain.lease_id),
            LeaseState::Queued,
            "no se cuela"
        );

        b.release(&osac.lease_id, 30).unwrap();

        assert_eq!(state(&b, &crit.lease_id), LeaseState::Granted);
        // Tras lo crítico quedan 2,5: el terreno espera su turno.
        assert_eq!(state(&b, &terrain.lease_id), LeaseState::Queued);
    }

    #[test]
    fn lo_critico_nunca_recibe_cede() {
        let mut b = office();
        let mt = b.acquire(req("maguetrader", 6.5, Priority::Critical, false), 0);
        b.acquire(req("otro", 12.0, Priority::Critical, false), 10);
        assert_eq!(b.yield_point(&mt.lease_id, 11), Ok(Directive::Continue));
    }

    #[test]
    fn a_quien_no_puede_ceder_no_se_le_pide() {
        let mut b = office();
        let terrain = b.acquire(req("terrain", 10.0, Priority::Low, false), 0);
        b.acquire(req("maguetrader", 8.0, Priority::Critical, false), 10);
        assert_eq!(
            b.yield_point(&terrain.lease_id, 11),
            Ok(Directive::Continue)
        );
    }

    // ── Ollama como motor de otro ───────────────────────────────────────────

    #[test]
    fn no_se_descarga_ollama_debajo_de_quien_lo_esta_usando() {
        // Humboldt corre su LLM dentro de Ollama. Descargarlo "por
        // oportunista" le rompería el trabajo en marcha.
        let mut b = office();
        b.set_ollama(gb_to_mb(6.0), vec!["llava:7b".into()], 0);
        let mut humboldt = req("humboldt", 8.0, Priority::Low, false);
        humboldt.backend = Some("ollama".into());
        b.acquire(humboldt, 0);
        b.drain_effects();

        b.acquire(req("maguetrader", 8.0, Priority::Critical, false), 10);
        assert!(
            !b.drain_effects()
                .iter()
                .any(|e| matches!(e, Effect::UnloadOllama { .. }))
        );
    }

    #[test]
    fn la_memoria_de_ollama_no_se_cuenta_dos_veces() {
        let mut b = office();
        b.set_ollama(gb_to_mb(6.0), vec![], 0);
        let mut humboldt = req("humboldt", 8.0, Priority::Low, false);
        humboldt.backend = Some("ollama".into());
        b.acquire(humboldt, 0);
        // Sólo los 8 del lease, no 8 + 6.
        assert_eq!(b.accounted_mb(), gb_to_mb(8.0));
    }

    #[test]
    fn ollama_se_pide_descargar_una_vez_no_en_cada_vuelta() {
        let mut b = office();
        b.set_ollama(gb_to_mb(4.0), vec![], 0);
        b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        b.drain_effects();
        b.acquire(req("maguetrader", 6.0, Priority::Critical, false), 10);
        b.tick(20);
        b.tick(30);
        let unloads = b
            .drain_effects()
            .into_iter()
            .filter(|e| matches!(e, Effect::UnloadOllama { .. }))
            .count();
        assert_eq!(unloads, 1);
    }

    // ── Lo medido manda sobre lo declarado ─────────────────────────────────

    #[test]
    fn memoria_que_nadie_explica_no_esta_libre() {
        // Un intruso con 5 GB sin lease: no se le puede contar como libre.
        let mut b = office();
        b.set_measured(Some(gb_to_mb(1.5 + 5.0)), None, 0);
        assert_eq!(b.unexplained_mb(), gb_to_mb(5.0));
        let osac = b.acquire(req("osac", 12.0, Priority::Normal, true), 0);
        assert_eq!(osac.state, LeaseState::Queued);
    }

    #[test]
    fn quien_usa_menos_de_lo_que_pidio_no_regala_memoria() {
        // Declaró 9 y todavía no cargó nada: lo contado manda.
        let mut b = office();
        b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        b.set_measured(Some(gb_to_mb(1.5)), None, 0);
        assert_eq!(b.free_mb(), gb_to_mb(16.0 - 1.5 - 9.0));
    }

    #[test]
    fn el_total_medido_reemplaza_al_configurado() {
        let mut b = office();
        b.set_measured(Some(1_821), Some(16_376), 0);
        assert_eq!(b.limits().total_mb, 16_376);
    }

    // ── Los que se mueren ───────────────────────────────────────────────────

    #[test]
    fn sin_latido_se_da_por_muerto_y_entra_el_siguiente() {
        let mut b = office();
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        let blender = b.acquire(req("blender", 8.0, Priority::Low, false), 0);
        assert_eq!(state(&b, &blender.lease_id), LeaseState::Queued);

        // Blender sigue preguntando; OSAC calla.
        b.poll(&blender.lease_id, 50_000).unwrap();
        b.tick(61_000);

        assert!(b.leases().iter().all(|l| l.id != osac.lease_id));
        assert_eq!(state(&b, &blender.lease_id), LeaseState::Granted);
        assert_eq!(b.drain_history()[0].outcome, Outcome::Expired);
    }

    #[test]
    fn el_latido_mantiene_vivo_y_trae_el_progreso() {
        let mut b = office();
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        b.heartbeat(
            &osac.lease_id,
            Some(0.4),
            Some("fragmento 7/18".into()),
            50_000,
        )
        .unwrap();
        b.tick(100_000);
        let lease = &b.leases()[0];
        assert_eq!(lease.progress, Some(0.4));
        assert_eq!(lease.message.as_deref(), Some("fragmento 7/18"));
    }

    #[test]
    fn un_lease_que_no_existe_lo_dice() {
        let mut b = office();
        assert_eq!(
            b.yield_point("t-99", 0),
            Err(BrokerError::UnknownLease("t-99".into()))
        );
    }

    // ── Lo que hace el usuario ──────────────────────────────────────────────

    #[test]
    fn pausar_cede_y_retiene_aunque_vuelva_a_pedir() {
        let mut b = office();
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        b.hold(&osac.lease_id, 1).unwrap();
        assert_eq!(b.yield_point(&osac.lease_id, 2), Ok(Directive::Yield));

        // El cliente suelta y vuelve a pedir, como hace al ceder:
        b.release(&osac.lease_id, 3).unwrap();
        let again = b.acquire(req("osac", 9.0, Priority::Normal, true), 4);
        assert_eq!(again.state, LeaseState::Queued, "sigue pausado");

        b.resume(&again.lease_id, 5).unwrap();
        assert_eq!(state(&b, &again.lease_id), LeaseState::Granted);
    }

    #[test]
    fn no_se_pausa_a_quien_no_puede_ceder() {
        let mut b = office();
        let terrain = b.acquire(req("terrain", 2.0, Priority::Low, false), 0);
        assert_eq!(
            b.hold(&terrain.lease_id, 1),
            Err(BrokerError::NotPreemptible("terrain".into()))
        );
    }

    #[test]
    fn cancelar_en_la_fila_lo_quita_y_concedido_le_dice_que_pare() {
        let mut b = office();
        let osac = b.acquire(req("osac", 9.0, Priority::Normal, true), 0);
        let blender = b.acquire(req("blender", 8.0, Priority::Low, false), 0);

        b.cancel(&blender.lease_id, 1).unwrap();
        assert!(b.leases().iter().all(|l| l.id != blender.lease_id));

        b.cancel(&osac.lease_id, 2).unwrap();
        assert_eq!(
            b.heartbeat(&osac.lease_id, None, None, 3),
            Ok(Directive::Cancel)
        );
        b.release(&osac.lease_id, 4).unwrap();
        let h = b.drain_history();
        assert!(h.iter().all(|e| e.outcome == Outcome::Cancelled));
    }

    #[test]
    fn subir_la_prioridad_cambia_el_orden_de_la_fila() {
        let mut b = office();
        b.acquire(req("big", 14.0, Priority::Normal, false), 0);
        let blender = b.acquire(req("blender", 3.0, Priority::Low, false), 10);
        let osac = b.acquire(req("osac", 3.0, Priority::Low, false), 20);
        assert_eq!(b.poll(&blender.lease_id, 21).unwrap().position, Some(1));
        b.set_priority(&osac.lease_id, Priority::Normal, 22)
            .unwrap();
        assert_eq!(b.poll(&osac.lease_id, 23).unwrap().position, Some(1));
    }

    #[test]
    fn modo_entrenamiento_reserva_la_gpu_para_lo_critico() {
        let mut b = office();
        let osac = b.acquire(req("osac", 4.0, Priority::Normal, true), 0);
        b.set_training(true, 1);
        assert_eq!(b.yield_point(&osac.lease_id, 2), Ok(Directive::Yield));

        // Mientras dure, lo no crítico espera aunque quepa.
        let terrain = b.acquire(req("terrain", 1.0, Priority::Low, false), 3);
        assert_eq!(terrain.state, LeaseState::Queued);
        let mt = b.acquire(req("maguetrader", 6.5, Priority::Critical, false), 4);
        assert_eq!(mt.state, LeaseState::Granted);

        b.set_training(false, 5);
        assert_eq!(state(&b, &terrain.lease_id), LeaseState::Granted);
    }

    #[test]
    fn modo_juego_reserva_la_gpu_y_vuelve_solo_al_cerrar() {
        let mut b = office();
        b.set_ollama(gb_to_mb(5.6), vec!["qwen3:8b".into()], 0);
        let osac = b.acquire(req("osac", 4.0, Priority::Normal, true), 0);

        b.set_interactive(vec!["TikTok LIVE Studio.exe".into()], 1);
        assert_eq!(b.yield_point(&osac.lease_id, 2), Ok(Directive::Yield));
        let reason = b
            .leases()
            .iter()
            .find(|l| l.id == osac.lease_id)
            .unwrap()
            .reason
            .clone();
        assert_eq!(
            reason.as_deref(),
            Some("modo juego: TikTok LIVE Studio.exe")
        );
        assert!(b.drain_effects().iter().any(
            |e| matches!(e, Effect::UnloadOllama { reason } if reason.contains("modo juego"))
        ));

        // Lo no crítico espera aunque quepa; lo crítico entra.
        let terrain = b.acquire(req("terrain", 1.0, Priority::Low, false), 3);
        assert_eq!(terrain.state, LeaseState::Queued);
        let mt = b.acquire(req("maguetrader", 1.0, Priority::Critical, false), 4);
        assert_eq!(mt.state, LeaseState::Granted);

        // Cerraste el directo: todo vuelve sin pulsar nada.
        b.set_interactive(Vec::new(), 5);
        assert_eq!(state(&b, &terrain.lease_id), LeaseState::Granted);
    }

    #[test]
    fn en_modo_juego_ollama_sin_lease_se_vuelve_a_descargar() {
        let mut b = office();
        b.set_interactive(vec!["obs64.exe".into()], 0);
        b.drain_effects();
        // Alguien le habló a Ollama sin pedir turno y cargó un modelo.
        b.set_ollama(gb_to_mb(5.6), vec!["qwen3:8b".into()], 1);
        assert!(
            b.drain_effects()
                .iter()
                .any(|e| matches!(e, Effect::UnloadOllama { .. }))
        );
    }

    #[test]
    fn en_modo_juego_ollama_con_lease_critico_se_respeta() {
        let mut b = office();
        b.set_interactive(vec!["obs64.exe".into()], 0);
        let mut r = req("maguetrader", 5.6, Priority::Critical, false);
        r.backend = Some("ollama".into());
        b.acquire(r, 1);
        b.drain_effects();
        b.set_ollama(gb_to_mb(5.6), vec!["qwen3:8b".into()], 2);
        assert!(
            !b.drain_effects()
                .iter()
                .any(|e| matches!(e, Effect::UnloadOllama { .. }))
        );
    }

    #[test]
    fn el_mismo_juego_otra_vez_no_repite_nada() {
        let mut b = office();
        let osac = b.acquire(req("osac", 4.0, Priority::Normal, true), 0);
        b.set_interactive(vec!["obs64.exe".into()], 1);
        b.resume(&osac.lease_id, 2).unwrap();
        b.drain_effects();
        b.set_interactive(vec!["obs64.exe".into()], 3);
        assert!(b.drain_effects().is_empty());
        assert_eq!(b.interactive(), ["obs64.exe".to_string()]);
    }

    #[test]
    fn entrenamiento_y_juego_a_la_vez_no_piden_ceder_dos_veces() {
        let mut b = office();
        let osac = b.acquire(req("osac", 4.0, Priority::Normal, true), 0);
        b.set_training(true, 1);
        b.set_interactive(vec!["obs64.exe".into()], 2);
        let reason = b
            .leases()
            .iter()
            .find(|l| l.id == osac.lease_id)
            .unwrap()
            .reason
            .clone();
        assert_eq!(reason.as_deref(), Some("modo entrenamiento"));
        // Apagar el entrenamiento con el juego abierto no suelta nada.
        b.set_training(false, 3);
        let terrain = b.acquire(req("terrain", 1.0, Priority::Low, false), 4);
        assert_eq!(terrain.state, LeaseState::Queued);
    }

    #[test]
    fn pausar_la_cola_congela_las_entradas() {
        let mut b = office();
        b.set_queue_paused(true, 0);
        let osac = b.acquire(req("osac", 1.0, Priority::Normal, false), 1);
        assert_eq!(osac.state, LeaseState::Queued);
        b.set_queue_paused(false, 2);
        assert_eq!(state(&b, &osac.lease_id), LeaseState::Granted);
    }

    #[test]
    fn el_historial_cuenta_cuanto_se_espero() {
        let mut b = office();
        let big = b.acquire(req("big", 14.0, Priority::Normal, false), 0);
        let small = b.acquire(req("small", 3.0, Priority::Low, false), 1_000);
        b.release(&big.lease_id, 5_000).unwrap();
        b.release(&small.lease_id, 9_000).unwrap();
        let h = b.drain_history();
        let small_entry = h.iter().find(|e| e.owner == "small").unwrap();
        assert_eq!(small_entry.waited_ms(), 4_000);
    }

    #[test]
    fn los_gb_no_se_pierden_en_flotantes() {
        // 16.0 GB tiene que caber en 16.0 GB.
        let mut b = Broker::new(
            Limits {
                total_mb: gb_to_mb(16.0),
                baseline_mb: 0,
                ..Limits::default()
            },
            "t",
        );
        let all = b.acquire(req("all", 16.0, Priority::Normal, false), 0);
        assert_eq!(all.state, LeaseState::Granted);
    }
}
