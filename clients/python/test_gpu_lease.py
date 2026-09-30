"""Pruebas del cliente contra el broker DE VERDAD.

No hay dobles: se arranca `void-gpu`, el broker de verdad, con un gpu.toml
temporal en un puerto libre y `measure = false`, para que las pruebas no
dependan de lo que tenga abierto la GPU de la maquina.

    python -m unittest clients/python/test_gpu_lease.py -v

Se saltan si no esta compilado el binario (`cargo build -p void-stack-gpu`).
"""

import asyncio
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import gpu_lease  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target" / "debug" / ("void-gpu.exe" if os.name == "nt" else "void-gpu")


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def admin(method, path, body=None):
    return gpu_lease._call(method, f"{BROKER}{path}", body)


BROKER = ""


@unittest.skipUnless(BINARY.exists(), f"falta {BINARY}")
class ContraElBroker(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        global BROKER
        cls.data = tempfile.mkdtemp(prefix="gpu-lease-test-")
        port = free_port()
        cfg_dir = Path(cls.data) / "void-stack"
        cfg_dir.mkdir(parents=True)
        # 16 GB y 1,5 de escritorio, como la maquina del spec.
        (cfg_dir / "gpu.toml").write_text(
            f'listen = "127.0.0.1:{port}"\n'
            "baseline_gb = 1.5\n"
            "measure = false\n"
            "sample_every_ms = 250\n"
            'ollama_url = "http://127.0.0.1:9"\n'
        )
        env = dict(os.environ, VOID_STACK_DATA_DIR=cls.data)
        cls.proc = subprocess.Popen(
            [str(BINARY)],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        BROKER = f"http://127.0.0.1:{port}"
        os.environ["VOID_GPU_BROKER"] = BROKER
        for _ in range(100):
            try:
                urllib.request.urlopen(f"{BROKER}/v1/health", timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        else:
            cls.proc.kill()
            raise RuntimeError("el broker no arranco")
        gpu_lease.POLL_S = 0.05

    @classmethod
    def tearDownClass(cls):
        cls.proc.kill()
        cls.proc.wait()

    def state(self):
        return admin("GET", "/v1/state")[1]

    def test_pedir_trabajar_y_soltar(self):
        with gpu_lease.gpu_lease("osac", task="render", vram_gb=2) as lease:
            self.assertFalse(lease.offline)
            owners = [l["owner"] for l in self.state()["leases"]]
            self.assertIn("osac", owners)
        self.assertEqual(self.state()["leases"], [])

    def test_quien_no_cabe_espera_su_turno(self):
        order = []
        entered = threading.Event()

        def big():
            with gpu_lease.gpu_lease("big", vram_gb=10, priority="normal"):
                entered.set()
                time.sleep(0.4)
                order.append("big-sale")

        t = threading.Thread(target=big)
        t.start()
        entered.wait(5)
        with gpu_lease.gpu_lease("blender", vram_gb=8, priority="low"):
            order.append("blender-entra")
        t.join()
        self.assertEqual(order, ["big-sale", "blender-entra"])

    def test_ceder_suelta_llama_a_on_yield_y_vuelve_a_entrar(self):
        # El escenario del spec: OSAC dentro, llega algo critico que no cabe,
        # OSAC cede en su yield_point, entra lo critico, y OSAC vuelve.
        freed = []
        critical_in = threading.Event()

        with gpu_lease.gpu_lease(
            "osac", vram_gb=9, preemptible=True, backend="comfyui",
            on_yield=lambda: freed.append("comfyui.free()"),
        ) as osac:

            def critical():
                with gpu_lease.gpu_lease("maguetrader", vram_gb=12, priority="critical"):
                    critical_in.set()
                    time.sleep(0.3)

            t = threading.Thread(target=critical)
            t.start()
            time.sleep(0.3)  # que lo critico llegue y pida sitio

            self.assertTrue(osac.yield_point())
            self.assertTrue(critical_in.is_set(), "lo critico entro mientras OSAC cedia")
            self.assertEqual(freed, ["comfyui.free()"])
            t.join()
            # Y OSAC sigue con su GPU: vuelve a estar concedido.
            self.assertFalse(osac.yield_point())

    def test_el_progreso_llega_a_la_oficina(self):
        gpu_lease.HEARTBEAT_S = 0.1
        try:
            with gpu_lease.gpu_lease("osac", vram_gb=1) as lease:
                lease.heartbeat(progress=0.4, message="fragmento 7/18")
                time.sleep(0.4)
                mine = [l for l in self.state()["leases"] if l["owner"] == "osac"][0]
                self.assertAlmostEqual(mine["progress"], 0.4, places=3)
                self.assertEqual(mine["message"], "fragmento 7/18")
        finally:
            gpu_lease.HEARTBEAT_S = 10.0

    def test_cancelar_desde_la_oficina_para_el_trabajo(self):
        with self.assertRaises(gpu_lease.GpuLeaseCancelled):
            with gpu_lease.gpu_lease("osac", vram_gb=1, preemptible=True) as lease:
                admin("POST", f"/v1/leases/{lease.lease_id}/cancel")
                lease.yield_point()
        self.assertEqual(self.state()["leases"], [])

    def test_async(self):
        async def run():
            async with gpu_lease.gpu_lease_async("humboldt", task="OCR", vram_gb=10) as lease:
                return lease.lease_id

        self.assertIsNotNone(asyncio.run(run()))


class SinBroker(unittest.TestCase):
    def setUp(self):
        self.saved = os.environ.get("VOID_GPU_BROKER")
        # Un puerto donde no escucha nadie.
        os.environ["VOID_GPU_BROKER"] = "http://127.0.0.1:9"

    def tearDown(self):
        if self.saved is None:
            os.environ.pop("VOID_GPU_BROKER", None)
        else:
            os.environ["VOID_GPU_BROKER"] = self.saved

    def test_sin_broker_funciona_igual_sin_cola(self):
        with gpu_lease.gpu_lease("osac", vram_gb=9) as lease:
            self.assertTrue(lease.offline)
            self.assertFalse(lease.yield_point())
            lease.heartbeat(progress=0.5)

    def test_sin_broker_se_queda_el_candado_del_proceso(self):
        # Lo que hacia el gpu_lock de Humboldt: dos a la vez NO, ni sin broker.
        inside = []

        def worker(name):
            with gpu_lease.gpu_lease(name, vram_gb=1):
                inside.append(name)
                self.assertEqual(len(inside), 1, "dos trabajos a la vez en la GPU")
                time.sleep(0.1)
                inside.remove(name)

        threads = [threading.Thread(target=worker, args=(n,)) for n in ("a", "b", "c")]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

    def test_off_ni_lo_intenta(self):
        os.environ["VOID_GPU_BROKER"] = "off"
        self.assertIsNone(gpu_lease.broker_url())


if __name__ == "__main__":
    unittest.main()
