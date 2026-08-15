# Sessions

Consola de escritorio para gestionar varias sesiones de agentes CLI —**Claude Code**,
**Codex**, **OpenCode**— en una sola ventana, con métricas de tokens en vivo
(tok/s, ventana de contexto, coste) y configuración por TOML en
`~/.sessions`.

Tauri 2 (Rust) + React. El binario ocupa unos pocos MB y la app arranca en
~30 MB de RAM, en lugar de los cientos que consume una alternativa basada en
Electron.

---

## Requisitos

| Herramienta | Para qué |
|---|---|
| Node 20+ y npm | interfaz (Vite + React) |
| Rust estable 1.77+ | backend Tauri |
| WebView2 (Windows) · WebKitGTK (Linux) | motor de la ventana |
| Los CLIs que quieras usar | `claude`, `codex`, `opencode`… deben estar en el PATH |

## Puesta en marcha

```bash
npm install
npm run app:dev        # desarrollo con recarga en caliente
npm run app:build      # instalador / binario de producción
```

Otros comandos útiles:

```bash
npm run build          # comprueba tipos y compila la interfaz
npm run rs:test        # pruebas del backend (96 en total)
npm run import:pi      # importa los proveedores de ~/.pi/agent
npm run icons          # regenera el juego de iconos
npm run dev:free       # libera el puerto 5273 si quedó un Vite anterior
```

---

## Estructura

El código (identificadores, comentarios y pruebas) está en inglés; los textos de
la interfaz, los mensajes de error y los comentarios de los `.toml` de
`~/.sessions` están en español, que es lo que se lee al usar la app.

```
sessions/
├─ src/                     interfaz (React + TypeScript)
│  ├─ term/pool.ts          piscina de terminales xterm (fuera de React)
│  ├─ state/store.ts        estado global (zustand)
│  ├─ components/           barra de título, lateral, métricas, diálogos
│  └─ lib/                  IPC, tipos y formateo
├─ src-tauri/
│  ├─ src/pty/              PTY, buffer circular y bomba de salida
│  ├─ src/metrics/          lectores de tokens por agente
│  ├─ src/config/           carga y validación de los TOML
│  ├─ src/launcher.rs       agente + proveedor → comando y entorno
│  ├─ src/store.rs          proyectos, sesiones y scrollback
│  ├─ src/commands.rs       API expuesta a la interfaz
│  └─ assets/*.default.toml plantillas que se copian a ~/.sessions
└─ scripts/                 iconos, importador de pi, sondas de interfaz y utilidades
```

---

## `~/.sessions`

Se crea en el primer arranque y **no se sobrescribe** después:

```
~/.sessions/
├─ config.toml           apariencia, terminal, rendimiento, reanudación, atajos
├─ providers.toml        proveedores, modelos, precios y mapeo por agente
├─ agents.toml           CLIs que la app puede lanzar
├─ state/projects.json   proyectos y sesiones registradas
├─ scrollback/           historial por sesión
└─ logs/
```

`SESSIONS_HOME=/otra/ruta` permite usar otra ubicación (útil para pruebas o para
un uso portable).

Tras editar cualquier `.toml`, aplica los cambios con **Ctrl+Shift+R** o el botón
*Recargar* de Ajustes. Un fichero con errores no impide arrancar: la app avisa del
problema y usa los valores de fábrica.

### Proveedores compatibles con distintos entornos

Se editan desde **Ajustes → Proveedores** (alta, edición, duplicado y borrado) o a
mano en el fichero. En el caso normal basta con esto:

```toml
[[provider]]
id = "mi-gateway"
kind = "openai-chat"          # decide qué CLI puede usarlo
base_url = "https://gateway.interno.local/v1"
api_key_env = "MI_API_KEY"
default_model = "modelo-a"

[[provider.model]]
id = "modelo-a"
context_window = 128000
max_output_tokens = 16384
```

El `kind` es lo que hace el trabajo: la app ya sabe qué variables y argumentos
espera cada CLI y los deriva sola.

| `kind` | Agentes que lo pueden usar | Qué inyecta |
|---|---|---|
| `anthropic` | Claude Code, OpenCode | `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_MODEL`, `API_TIMEOUT_MS`… |
| `openai-chat` | Codex | `<ID>_API_KEY` y los overrides `-c model_providers.<id>.… wire_api=chat` |
| `openai-responses` | Codex, OpenCode | igual, con `wire_api=responses` |
| `google` | Gemini CLI, OpenCode | `GEMINI_API_KEY`, `GOOGLE_GENERATIVE_AI_API_KEY` |
| `ollama` | Codex | endpoint local compatible con OpenAI |

La clave se escribe **directamente en el editor, debajo de la URL base** (campo
enmascarado con botón para verla). El desplegable a su derecha permite cambiar el
origen sin mover el campo de sitio: variable del entorno, **fichero** (texto o JSON
con `api_key_json_path`, p. ej. `gorouter.key`), **comando** (`op`, `pass`,
`gopass`…) o sin clave. Solo se guarda uno de los cuatro: al cambiar de origen se
limpian los demás campos.

El botón **Comprobar** del editor resuelve la clave de verdad y enseña, agente por
agente, las variables y argumentos exactos que recibirá el proceso, con el secreto
enmascarado. Es la forma rápida de ver por qué un proveedor no aparece para un CLI.

Al guardar desde la app se reescribe únicamente el bloque `[[provider]]` afectado:
los comentarios y el resto del fichero se conservan (`toml_edit`), incluso al
borrar un proveedor. Antes de escribir se valida el identificador, que no haya
modelos repetidos y que los modelos por defecto existan.

#### Cuando hace falta bajar al detalle

`[provider.env.<agente>]` y `[provider.args]` siguen ahí para añadir o corregir
cosas puntuales. Lo que escribas gana sobre la plantilla, y una variable con valor
vacío (`""`) anula la heredada. Ejemplo real: DeepSeek expone además un endpoint
compatible con Anthropic, así que también vale para Claude Code:

```toml
[provider.env.claude]
ANTHROPIC_BASE_URL = "{base_url}/anthropic"
ANTHROPIC_AUTH_TOKEN = "{api_key}"
ANTHROPIC_MODEL = "{model}"
```

```toml
[[provider]]
id = "openrouter"
name = "OpenRouter"
kind = "openai-chat"                   # → Codex, con los overrides -c automáticos
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"     # o api_key / api_key_file / api_key_command
default_model = "anthropic/claude-sonnet-4.5"

[[provider.model]]
id = "anthropic/claude-sonnet-4.5"
context_window = 200000
max_output_tokens = 64000
reasoning = true
[provider.model.pricing]               # USD por millón de tokens
input = 3.0
output = 15.0

[provider.env.opencode]                # OpenCode trae OpenRouter integrado
OPENROUTER_API_KEY = "{api_key}"

[provider.args]
opencode = ["--model", "openrouter/{model}"]
```

Plantillas disponibles: `{base_url}` `{api_key}` `{model}` `{model_id}`
`{context_window}` `{max_output_tokens}` `{small_model}` `{organization}`
`{project}` `{region}` `{api_version}` `{timeout_ms}` `{max_retries}`
`{provider_id}` `{provider_name}` `{env_var}` `{home}`. `{{` y `}}` escapan llaves.
Si una plantilla no se puede resolver (por ejemplo falta la clave), esa variable
**no se pasa** al proceso, en lugar de propagar un literal roto.

La clave se resuelve en este orden: `api_key` literal → `api_key_file`
(+ `api_key_json_path`) → `api_key_command` → `api_key_env`. Lo recomendable es no
escribir claves en el fichero.

De fábrica vienen Anthropic, OpenAI, Google, OpenRouter, DeepSeek, Ollama y una
plantilla de *gateway* propio.

### Importar los proveedores de pi

Si ya usas [pi](https://github.com/earendil-works) hay un importador que lee
`~/.pi/agent/models.json`, `models-store.json` y `auth.json`:

```bash
npm run import:pi -- --dry     # enseña lo que haría
npm run import:pi              # escribe en ~/.sessions/providers.toml
npm run import:pi -- --incluir-openai
```

Qué hace:

* Traduce `anthropic-messages` → `kind = "anthropic"` (Claude Code) y
  `openai-completions` → `kind = "openai-chat"` (Codex). El mapeo de variables no
  se escribe: lo deduce la app del `kind`, así que los bloques importados son
  cortos y legibles.
* Copia modelos con su ventana de contexto, límite de salida y precios.
* **No copia las claves**: apunta `api_key_file` a `~/.pi/agent/auth.json` con
  `api_key_json_path = "<proveedor>.key"`, y se leen al lanzar la sesión.
* Es idempotente: reemplaza por `id` los bloques de una importación anterior y no
  toca el resto del fichero. Si editas un proveedor importado desde la app, una
  reimportación lo sobrescribe.

### Agentes

```toml
[[agent]]
id = "claude"                       # este id es la clave en [provider.env.<id>]
name = "Claude Code"
command = "claude"
command_windows = "claude.cmd"      # los shims de npm en Windows son .cmd
resume_args = ["--resume", "{session_id}"]
continue_args = ["--continue"]
model_args = ["--model", "{model}"]
metrics = "claude-jsonl"            # de dónde salen los tokens
```

`metrics` admite `claude-jsonl`, `codex-rollout`, `opencode-sqlite` o `none`.
Añadir un CLI nuevo es solo otro bloque `[[agent]]`; si no publica telemetría,
la app sigue mostrando su actividad y el rendimiento de salida.

---

## Métricas: de dónde salen

No se estiman: se leen del propio registro de cada agente.

| Agente | Origen | Datos |
|---|---|---|
| Claude Code | `~/.claude/projects/<cwd>/<id>.jsonl` | `message.usage` por turno (entrada, salida, caché, *thinking*), modelo |
| Codex | `~/.codex/sessions/AAAA/MM/DD/rollout-*.jsonl` | eventos `token_count`, `model_context_window`, modelo del turno |
| OpenCode | `~/.local/share/opencode/opencode.db` (solo lectura) | acumulados de la tabla `session`, coste |

Los ficheros se leen de forma incremental por desplazamiento, nunca completos, y
el sondeo se adapta: rápido mientras la sesión produce salida, lento en reposo.

* **tok/s** — tokens de salida del último turno entre su duración, suavizado con
  media móvil. Se muestra también el pico.
* **Ventana de contexto** — la que reporte el agente; si no la reporta, la
  declarada en `providers.toml` para ese modelo.
* **Coste** — el que informe el agente o el calculado con `[provider.model.pricing]`.
* **Bytes/s de salida del PTY** — señal de actividad válida incluso con agentes sin
  telemetría propia.

El backend también puede medir CPU y RAM del árbol de procesos de cada sesión,
pero la barra ya no los muestra y el muestreo viene desactivado
(`process_sample_ms = 0`): activarlo obliga a enumerar todos los procesos de la
máquina en cada ciclo.

### Al reabrir la app: reanudación automática

Los procesos no sobreviven al cierre, así que al arrancar la app vuelve a lanzar
las sesiones guardadas por su cuenta: con los agentes que lo admiten reanuda la
conversación (`claude --resume <id>`, `codex resume <id>`) y el resto arrancan de
cero en el mismo directorio, con su mismo proveedor y modelo. No hay que pulsar
nada.

```toml
[app]
restore_sessions = true     # interruptor general
auto_resume = "active"      # active (la última usada) | all (todas) | none
```

`active` es el valor por defecto a propósito: cada agente es un proceso Node que
puede rondar los 700 MB, así que `all` conviene solo si trabajas con pocas
sesiones. Con `none` (o `restore_sessions = false`) las sesiones aparecen como
*Terminada* y se reabren a mano.

La sesión reanudada recibe un id nuevo y sustituye al registro anterior. Si el
lanzamiento falla —el CLI ya no está en el PATH, el directorio se movió— el
registro **no** se pierde: la app avisa del motivo y la sesión se queda como
terminada, lista para reintentarlo.

### Sesiones que no se reanudan

Con `auto_resume = "none"`, o si la reanudación falla, la sesión aparece como
*Terminada* y se muestra su última pantalla. Esa salida no se reproduce tal cual:
los agentes dibujan con posicionamiento absoluto y secuencias como `CSI ?25l`
(ocultar cursor), de modo que escribir esos bytes en un terminal nuevo de otro
tamaño se vería roto y sin cursor.

En su lugar se reconstruye la pantalla: los bytes se reproducen en un emulador
fuera de pantalla del tamaño original y se serializa su búfer, así que se ve la
última pantalla real con sus colores. Un aviso indica que ese registro no acepta
escritura y ofrece **Relanzar** (proceso nuevo) o **Reanudar** (`--resume` con el
id de sesión del CLI). Si escribes de todas formas, la app lo dice en lugar de
tragarse la pulsación.

---

## Rendimiento

El pipeline de salida está pensado para agentes que escriben mucho y muy a menudo:

```
[hilo lector/sesión] read(64K) ─► buffer circular (rehidratación)
        └─ canal acotado ─► [1 hilo bomba] agrupa por sesión y envía cada
                             flush_interval_ms o al llegar a max_chunk_bytes
                             ─► canal binario ─► xterm.write()
[1 hilo supervisor] try_wait() cada 300 ms ─► evento de fin + cierre de handles
```

* **Coalescencia**: miles de escrituras pequeñas se convierten en ~80 mensajes
  por segundo y sesión. Verificado en pruebas (300 líneas → menos de 100 mensajes).
* **Transporte binario**: la salida viaja como bytes por un `Channel` de Tauri, sin
  serializar a JSON ni volver a decodificar.
* **Un solo hilo de salida y uno de supervisión** para todas las sesiones, más un
  lector por PTY; el número de hilos no crece con la carga.
* **Cola acotada**: si la interfaz se atasca, los lectores se frenan en lugar de
  consumir memoria sin límite.
* **React fuera del camino caliente**: la salida va directa a xterm; las tarjetas
  de la barra lateral se suscriben solo a sus propias métricas.
* **Terminales que no se re-montan**: cada sesión conserva su instancia de xterm
  y se oculta con CSS; las de sesiones terminadas se liberan por LRU
  (`max_live_terminals`) y se rehidratan desde el buffer circular.
* **Renderizador WebGL** con `addon-webgl`, con caída a DOM si no hay contexto.
* **Tipografías empaquetadas** (Inter variable e JetBrains Mono, ~150 KB de woff2
  en total): no dependen de lo instalado en el sistema y las métricas del terminal
  son idénticas en cualquier equipo. Al terminar de cargar se rehacen las medidas
  de xterm, para que las columnas no se calculen con la fuente de reserva.

Todo esto se ajusta en `[performance]` y `[terminal]` de `config.toml`; los
valores se acotan al cargarlos para que una edición desafortunada no degrade la app.

### Nota sobre ConPTY (Windows)

Dos comportamientos condicionan el diseño y están cubiertos por pruebas:

1. ConPTY emite `ESC[6n` al arrancar y **no produce salida hasta recibir
   respuesta**. La contesta el propio emulador: por eso el terminal de una sesión
   viva nunca se libera, y al rehidratar el historial la respuesta se regenera.
2. El lector del PTY **no recibe EOF** cuando el proceso hijo termina. El fin se
   detecta con `try_wait` desde el supervisor, que además cierra los handles y así
   desbloquea el hilo lector.

De ahí sale una regla que no es evidente: **el historial y el flujo en vivo no
pueden solaparse**. Enganchar un terminal es por eso una operación del hilo de
salida, que toma la instantánea y la marca de bytes juntas y descarta lo que la
instantánea ya cubría. Si un fragmento se entregase dos veces, el `ESC[6n`
duplicado haría que el emulador respondiera dos veces, y esa segunda respuesta
aparecería como un carácter suelto en el prompt del agente.

---

## Atajos

| Atajo | Acción |
|---|---|
| `Ctrl+Shift+T` | nueva sesión |
| `Ctrl+Shift+W` | cerrar la sesión activa |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | sesión siguiente / anterior |
| `Ctrl+Shift+B` | barra lateral |
| `Ctrl+Shift+M` | barra de métricas |
| `Ctrl+Shift+F` | buscar en el terminal |
| `Ctrl+Shift+K` | limpiar el terminal |
| `Ctrl+Shift+R` | recargar la configuración |
| `Ctrl+,` | ajustes |

Se usan combinaciones con Shift a propósito: `Ctrl+C`, `Ctrl+R`, `Ctrl+W` o
`Ctrl+F` pertenecen al agente y llegan intactas al PTY.

---

## Pruebas

```bash
npm run rs:test     # 92 unitarias + 4 de extremo a extremo
npm run build       # tipos + interfaz
```

Cubren el parseo de los TOML de fábrica, la resolución de proveedores por agente,
la edición de `providers.toml` conservando comentarios, el buffer circular, el
ciclo de vida del PTY (incluidos el diálogo DSR de ConPTY y la detección de fin sin
EOF), la coalescencia de salida, los tres lectores de métricas, la persistencia y
la recarga en caliente.

Para inspeccionar o automatizar la interfaz real dentro de WebView2:

```bash
npm run app:dev     # en otra terminal
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
  ./src-tauri/target/debug/sessions.exe
node scripts/ui-probe.mjs "document.body.innerText"
node scripts/ui-probe.mjs --type $'echo hola\r'
node scripts/ui-shot.mjs captura.png
node scripts/ui-test-provider.mjs gorouter 300000   # edita y guarda un proveedor
```

---

## Problemas frecuentes

**`Error: Port 5273 is already in use`** al lanzar `npm run app:dev`.
Ha quedado un servidor de desarrollo anterior vivo. El puerto es fijo a
propósito (`strictPort`): la ventana apunta a esa URL exacta, y si Vite se
moviera a otro puerto la app cargaría en blanco. Para liberarlo:

```bash
npm run dev:free      # mata el proceso node que tenga el 5273, y solo ese
```

El script comprueba el nombre de imagen antes de matar nada: si el puerto lo
ocupa otro programa, lo dice y no lo toca. A propósito no está enganchado como
`predev`: matar procesos desde dentro del propio `npm run dev` puede tumbar la
cadena que acaba de arrancar.

Si prefieres otro puerto, cámbialo en los dos sitios que deben coincidir:
`server.port` de `vite.config.ts` y `build.devUrl` de `src-tauri/tauri.conf.json`.

**`error LNK2001: símbolo externo anon.… sin resolver`** al compilar.
Artefactos de compilación incremental corruptos, normalmente por haber
interrumpido un enlazado a medias. Se arregla borrando solo eso:

```bash
rm -rf src-tauri/target/debug/incremental    # o cargo clean -p sessions
```

**El agente aparece como «no instalado»** en el diálogo de nueva sesión. La app
busca el ejecutable en el `PATH` respetando `PATHEXT` en Windows. Ajusta
`command` o `command_windows` en `agents.toml` (los CLIs instalados con npm son
`.cmd`) o indica la ruta absoluta.

**Una sesión se queda en blanco al arrancar.** Es señal de que nadie ha
contestado a la consulta `ESC[6n` de ConPTY. Ocurre si se libera el terminal de
una sesión viva; la app no lo hace, pero si tocas `max_live_terminals` déjalo en
2 o más.

---

## Seguridad

* Las claves de API no se escriben en el disco por la app: se leen del entorno o
  de un gestor de secretos mediante `api_key_command`. Si las pones en
  `providers.toml`, ese fichero queda en tu carpeta de usuario.
* `providers.toml` y `agents.toml` **ejecutan procesos y definen su entorno**:
  trátalos como código y no cargues ficheros de terceros sin revisarlos.
* La base de datos de OpenCode se abre siempre en modo solo lectura.
* La ventana usa CSP restrictiva y solo se conceden los permisos de Tauri que la
  app necesita (diálogo de carpetas, abrir rutas y control de la ventana).
