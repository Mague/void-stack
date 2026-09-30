# Claude Code en La Oficina

Cada sesión de Claude Code aparece en La Oficina de void-hq como un robot en
su escritorio: teclea mientras usa herramientas y **levanta la mano** cuando
espera que apruebes algo. Lo cuentan sus propios hooks, que reenvían tal cual
el JSON que Claude Code les da por stdin al broker de GPU de void-stack
(`POST http://127.0.0.1:7410/v1/agents/hook`). Toda la interpretación vive en
el broker (`crates/void-stack-gpu/src/agents.rs`), así que el hook es una
línea de curl.

## Instalar

Copia el bloque `hooks` de [`settings.snippet.json`](settings.snippet.json) a
tu `~/.claude/settings.json` (en Windows,
`%USERPROFILE%\.claude\settings.json`). Si ya tienes hooks para esos eventos,
añade la entrada a la lista del evento en vez de reemplazarla.

Qué se escucha y qué significa en La Oficina:

| Evento             | En la oficina                                          |
|--------------------|--------------------------------------------------------|
| `UserPromptSubmit` | se pone a trabajar                                     |
| `PreToolUse`       | teclea; la ficha dice qué herramienta                  |
| `PostToolUse`      | sigue trabajando                                       |
| `Notification`     | `permission_prompt` → levanta la mano; `idle_prompt` → espera tu mensaje |
| `Stop`             | ¡terminó! (confeti unos segundos)                      |
| `SubagentStop`     | igual que `Stop`                                       |
| `SessionEnd`       | deja el escritorio                                     |

## Por qué el comando es así

- **`"async": true`**: el hook corre en segundo plano y no añade latencia a
  Claude. Además su código de salida se ignora, así que **si el broker no
  está corriendo no pasa nada**: ni bloquea una herramienta ni ensucia la
  conversación.
- **`curl.exe` y no `curl`**: en Windows los hooks corren con Git Bash si
  está instalado, y si no con PowerShell. En PowerShell 5.1 `curl` es un alias
  de `Invoke-WebRequest`, que no entiende estos argumentos. `curl.exe` es el
  binario real en los dos casos (viene con Windows 10+ y con Git).
- **`"@-"` entre comillas**: en PowerShell la `@` suelta se lee como el
  operador de splatting. Entre comillas, curl recibe `@-` —"lee el cuerpo de
  stdin"— en los dos intérpretes.
- **`-m 3` y `"timeout": 5`**: el broker contesta en milisegundos; si tarda
  más, algo va mal y no merece la pena esperar.
- **`-o NUL`: no gasta tokens.** En algunos eventos (`UserPromptSubmit`) lo
  que un hook imprime entra en el contexto de la sesión. El broker ya contesta
  204 sin cuerpo, y además la salida de curl va a la nada: medido el
  2026-09-30, el hook imprime 0 bytes.

## Qué se ve en La Oficina

De cada sesión: el proyecto (el `cwd`), qué está haciendo (la descripción
del comando o el fichero que edita), qué le pediste (la primera línea de tu
último mensaje) y, **sólo si lleva una lista de tareas**, cuánto lleva: el
`TodoWrite` trae la lista entera en cada cambio, y de ahí sale el porcentaje.
Sin lista no hay porcentaje: no se inventa uno.

## Comprobar que funciona

Con el broker corriendo (`void-gpu`), abre una sesión de Claude
Code, pídele algo que use una herramienta, y mira el estado:

    curl.exe -s http://127.0.0.1:7410/v1/state

La sesión aparece en `agents`, con su `project` (el último trozo del `cwd`),
su `status` y la herramienta en curso.
