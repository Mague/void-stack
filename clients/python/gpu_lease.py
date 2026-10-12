"""Cliente del broker de GPU de void-stack. Un solo archivo, solo stdlib.

Copialo tal cual al proyecto que use la GPU. Sin dependencias a proposito:
meter `requests` en cuatro proyectos para hablar con un servidor local seria
cuatro entornos que romper.

    from gpu_lease import gpu_lease

    with gpu_lease("osac", task="render expedientes/02", vram_gb=9,
                   priority="normal", preemptible=True, backend="comfyui",
                   on_yield=client.free) as lease:
        for n, seg in enumerate(segs, 1):
            lease.yield_point()          # a veces tarda: esta cediendo la GPU
            lease.heartbeat(progress=n / len(segs), message=f"fragmento {n}/{len(segs)}")
            render(seg)

Y en codigo async (Humboldt):

    async with gpu_lease_async("humboldt", task="OCR", vram_gb=10):
        ...

── Que significa ceder ─────────────────────────────────────────────────────
`yield_point()` pregunta "¿sigo o cedo?". Si toca ceder: llama a `on_yield`
(para que quien retiene la VRAM la suelte: ComfyUI, Ollama, torch), SUELTA el
lease, vuelve a la fila y bloquea hasta que le vuelva a tocar. Para quien lo
llama es una linea que a veces tarda. Ponlo solo donde no tengas nada vivo en
la GPU: entre fragmentos, despues de un checkpoint.

── Si no puedes esperar ─────────────────────────────────────────────────────
`max_wait_s` pone tope a la fila. Pasado, se sale de ella, lo dice en el log
y sigue SIN lease (y sin candado: la GPU la tiene otro, legitimamente). Es
para lo que no puede quedarse parado, como una confirmacion de trading en
vivo: trabajar mas lento compartiendo memoria es mejor que no contestar.
`lease.gave_up` dice si paso.

── Si el broker no esta ────────────────────────────────────────────────────
Funciona igual, sin cola, y lo dice una vez en el log. Pero no a pelo: se
queda con un candado dentro del proceso, que es exactamente lo que el
`gpu_lock` de Humboldt hacia antes. Sin broker nunca es peor que hoy.

Variables de entorno:
    VOID_GPU_BROKER   URL del broker (por defecto http://127.0.0.1:7410),
                      u "off" para no intentarlo siquiera.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import threading
import time
import urllib.error
import urllib.request
from contextlib import asynccontextmanager, contextmanager
from typing import Callable, Iterator, Optional

log = logging.getLogger("gpu_lease")

DEFAULT_BROKER = "http://127.0.0.1:7410"
#: Cada cuanto late el lease. El broker da por muerto a quien calla 60 s.
HEARTBEAT_S = 10.0
#: Cada cuanto pregunta quien espera en la fila si ya le toca.
POLL_S = 1.0
#: Una llamada al broker nunca puede colgar el trabajo que protege.
TIMEOUT_S = 3.0

_local_lock = threading.Lock()
_warned_down = False


class GpuLeaseCancelled(RuntimeError):
    """Alguien cancelo este trabajo desde La Oficina."""


def broker_url() -> Optional[str]:
    url = os.environ.get("VOID_GPU_BROKER", DEFAULT_BROKER).strip()
    return None if url.lower() in ("", "off", "0", "false") else url.rstrip("/")


def clear_cuda_cache() -> None:
    """Lo mismo que el `clear_cuda_cache` de Humboldt: soltar la cache de torch."""
    try:
        import torch  # noqa: WPS433 - opcional a proposito

        if torch.cuda.is_available():
            torch.cuda.empty_cache()
    except Exception:  # sin torch, o sin GPU: nada que soltar
        pass


def _call(method: str, url: str, body: Optional[dict] = None) -> tuple[int, dict]:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        req.add_header("content-type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT_S) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw else {})
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw) if raw else {}
        except ValueError:
            return e.code, {}


class Lease:
    """Un permiso sobre la GPU. No se construye a mano: usa `gpu_lease`."""

    def __init__(
        self, owner, task, vram_gb, priority, preemptible, backend, on_yield, max_wait_s=None
    ):
        self.owner = owner
        self.task = task
        self.vram_gb = vram_gb
        self.priority = priority
        self.preemptible = preemptible
        self.backend = backend
        self.on_yield = on_yield
        self.lease_id: Optional[str] = None
        #: Sin broker: se trabaja con el candado local y nada mas.
        self.offline = False
        self.max_wait_s = max_wait_s
        #: Se canso de esperar (`max_wait_s`) y siguio sin lease.
        self.gave_up = False
        self._progress: Optional[float] = None
        self._message: Optional[str] = None
        self._cancelled = False
        self._stop = threading.Event()
        self._beat: Optional[threading.Thread] = None
        self._base = broker_url()

    # ── ciclo de vida ──────────────────────────────────────────────────────

    def _request(self) -> dict:
        return {
            "owner": self.owner,
            "task": self.task,
            "vram_gb": self.vram_gb,
            "priority": self.priority,
            "preemptible": self.preemptible,
            "pid": os.getpid(),
            "backend": self.backend,
        }

    def _acquire(self) -> None:
        """Pide la GPU y espera turno. Si no hay broker, se queda offline."""
        global _warned_down
        if self._base is None:
            self.offline = True
            return
        try:
            _, got = _call("POST", f"{self._base}/v1/leases", self._request())
        except (urllib.error.URLError, OSError) as e:
            if not _warned_down:
                log.warning("broker de GPU no disponible (%s): sigo sin cola", e)
                _warned_down = True
            self.offline = True
            return
        self.lease_id = got["lease_id"]
        last_pos = None
        started = time.monotonic()
        while got.get("state") != "granted":
            if got.get("position") != last_pos:
                last_pos = got.get("position")
                log.info("%s esperando GPU (%.1f GB), posicion %s", self.owner, self.vram_gb, last_pos)
            waited = time.monotonic() - started
            if self.max_wait_s is not None and waited >= self.max_wait_s:
                log.warning(
                    "%s lleva %.0f s esperando la GPU: sigo sin lease (max_wait_s)",
                    self.owner,
                    waited,
                )
                self._release()
                self.gave_up = True
                return
            pause = POLL_S
            if self.max_wait_s is not None:
                pause = max(0.05, min(POLL_S, self.max_wait_s - waited))
            time.sleep(pause)
            status, got = _call("GET", f"{self._base}/v1/leases/{self.lease_id}")
            if status == 404:
                # Caduco en la fila o el broker se reinicio: se vuelve a pedir.
                _, got = _call("POST", f"{self._base}/v1/leases", self._request())
                self.lease_id = got["lease_id"]
        log.info("GPU concedida a %s (%s)", self.owner, self.lease_id)

    def _release(self) -> None:
        if self.lease_id and self._base:
            try:
                _call("DELETE", f"{self._base}/v1/leases/{self.lease_id}")
            except (urllib.error.URLError, OSError):
                pass  # si el broker no esta, el lease caduca solo
        self.lease_id = None

    def _heartbeat_loop(self) -> None:
        while not self._stop.wait(HEARTBEAT_S):
            self._send_heartbeat()

    def _send_heartbeat(self) -> Optional[str]:
        if self.offline or not self.lease_id:
            return None
        try:
            status, got = _call(
                "POST",
                f"{self._base}/v1/leases/{self.lease_id}/heartbeat",
                {"progress": self._progress, "message": self._message},
            )
        except (urllib.error.URLError, OSError):
            return None
        if status == 404:
            # El broker se reinicio y nos olvido. Seguimos trabajando (la GPU ya
            # la tenemos) y nos volvemos a apuntar para que nos vea.
            try:
                _, got = _call("POST", f"{self._base}/v1/leases", self._request())
                self.lease_id = got.get("lease_id")
            except (urllib.error.URLError, OSError):
                pass
            return None
        directive = got.get("directive")
        if directive == "cancel":
            self._cancelled = True
        return directive

    def start(self) -> "Lease":
        self._acquire()
        if not self.offline and not self.gave_up:
            self._stop.clear()
            self._beat = threading.Thread(target=self._heartbeat_loop, daemon=True)
            self._beat.start()
        return self

    def stop(self) -> None:
        self._stop.set()
        self._release()
        clear_cuda_cache()

    # ── lo que llama el trabajo ─────────────────────────────────────────────

    def heartbeat(self, progress: Optional[float] = None, message: Optional[str] = None) -> None:
        """Cuenta como va. Se manda en el proximo latido, sin bloquear."""
        if progress is not None:
            self._progress = max(0.0, min(1.0, float(progress)))
        if message is not None:
            self._message = message
        if self._cancelled:
            raise GpuLeaseCancelled(f"{self.owner}: cancelado desde La Oficina")

    def yield_point(self) -> bool:
        """¿Sigo o cedo? Devuelve True si cedio (y ya volvio a entrar)."""
        if self._cancelled:
            raise GpuLeaseCancelled(f"{self.owner}: cancelado desde La Oficina")
        if self.offline or not self.lease_id:
            return False
        try:
            status, got = _call("POST", f"{self._base}/v1/leases/{self.lease_id}/yield")
        except (urllib.error.URLError, OSError):
            return False
        directive = got.get("directive") if status == 200 else None
        if directive == "cancel":
            self._cancelled = True
            raise GpuLeaseCancelled(f"{self.owner}: cancelado desde La Oficina")
        if directive != "yield":
            return False

        log.info("%s cede la GPU", self.owner)
        if self.on_yield is not None:
            self.on_yield()
        clear_cuda_cache()
        self._release()
        self._acquire()  # a la fila, hasta que vuelva a tocar
        return True


@contextmanager
def gpu_lease(
    owner: str,
    task: str = "",
    vram_gb: float = 0.0,
    priority: str = "normal",
    preemptible: bool = False,
    backend: Optional[str] = None,
    on_yield: Optional[Callable[[], None]] = None,
    max_wait_s: Optional[float] = None,
) -> Iterator[Lease]:
    """Pide la GPU al broker y la suelta al salir, pase lo que pase."""
    lease = Lease(owner, task, vram_gb, priority, preemptible, backend, on_yield, max_wait_s)
    lease.start()
    local = lease.offline
    if local:
        # Sin broker: el candado del proceso, como el gpu_lock de Humboldt.
        _local_lock.acquire()
    try:
        yield lease
    finally:
        if local:
            _local_lock.release()
        lease.stop()


@asynccontextmanager
async def gpu_lease_async(
    owner: str,
    task: str = "",
    vram_gb: float = 0.0,
    priority: str = "normal",
    preemptible: bool = False,
    backend: Optional[str] = None,
    on_yield: Optional[Callable[[], None]] = None,
    max_wait_s: Optional[float] = None,
):
    """Lo mismo, para codigo async. La espera en la fila no bloquea el bucle."""
    lease = Lease(owner, task, vram_gb, priority, preemptible, backend, on_yield, max_wait_s)
    await asyncio.to_thread(lease.start)
    local = lease.offline
    if local:
        await asyncio.to_thread(_local_lock.acquire)
    try:
        yield lease
    finally:
        if local:
            _local_lock.release()
        await asyncio.to_thread(lease.stop)
