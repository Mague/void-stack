//! `void-gpu`: el broker de GPU como programa propio.
//!
//! Lee `gpu.toml` de la carpeta de configuración de void-stack (la misma que
//! respeta `VOID_STACK_DATA_DIR`) y escucha en `127.0.0.1:7410` salvo que el
//! fichero diga otra cosa. Nunca fuera de loopback: da órdenes de ceder y de
//! descargar, y no tiene autenticación.
//!
//! Si no está corriendo no se rompe nada: el cliente Python cae a un candado
//! local, La Oficina de void-hq dice que está apagado y la bandeja de
//! void-stack-desktop no enseña icono.

const HELP: &str = "void-gpu: broker de una GPU compartida (leases, fila por prioridad, VRAM)

Uso: void-gpu

Configuración: gpu.toml en la carpeta de void-stack
  (VOID_STACK_DATA_DIR/void-stack/gpu.toml, o la global si no se define).
Escucha por defecto en 127.0.0.1:7410; sólo acepta direcciones de loopback.";

#[tokio::main]
async fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("void-gpu {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("void-gpu: argumento desconocido: {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }

    tracing_subscriber::fmt().with_ansi(false).init();
    tracing::info!("void-gpu {} arrancando", env!("CARGO_PKG_VERSION"));
    // Un fallo al arrancar (el puerto ya está cogido, un gpu.toml roto) sale
    // con código distinto de cero: la tarea programada lo deja en su log.
    if let Err(e) = void_stack_gpu::run().await {
        tracing::error!("void-gpu: el broker no arrancó: {e}");
        std::process::exit(1);
    }
}
