# dl-tui

### Dashboard no oficial de DockerLabs — TUI no oficial para DockerLabs

<p><a href="README.md">English</a> · <strong>Español</strong></p>

<p align="center">
  <img src="assets/dashboard-maquinas.png" alt="dl-tui — pestaña Máquinas con tema Nord" width="100%">
</p>

**dl-tui** es un dashboard interactivo para tu terminal de [DockerLabs](https://dockerlabs.es): explora el catálogo de máquinas, descárgalas, lee y envía writeups de la comunidad, valora máquinas, sigue tu progreso y descarga tus certificados — todo sin salir de la terminal.

Un comando, una pantalla: ejecutar `dl` abre el dashboard. Escrito en **Rust** puro (ratatui), distribuido como un binario estático. La interfaz está en **español**, fiel a la plataforma original, con tema **Nord**.

> **Inicio de sesión obligatorio**: el dashboard arranca con un popup de login dentro de la propia app (tu contraseña vive en la bóveda del sistema — nunca en texto plano), porque la mayoría de las funciones de la plataforma están ligadas a la cuenta: progreso, máquinas completadas, valoraciones, envíos de writeups y certificados.

---

## Capturas

| Máquinas | Progreso |
| :---: | :---: |
| ![Pestaña Máquinas](assets/dashboard-maquinas.png) | ![Pestaña Progreso](assets/dashboard-progreso.png) |

Dificultades con los colores oficiales del sitio, medidores de progreso en vivo, ids de certificado por máquina y una vista de writeups pendientes para que siempre sepas qué falta.

---

## Características

* **Un solo comando** — `dl` abre el dashboard: catálogo, descargas, writeups, valoraciones y tu progreso en una pantalla.
* **Máquinas** — el catálogo completo (nombre, dificultad con los colores oficiales, categoría, autor, fecha, descripción) con filtrado instantáneo con `/` y ordenación con `s` (orden del sitio → nombre → fecha → dificultad).
* **Descargas rápidas** — las máquinas se descargan directamente del almacenamiento público de DockerLabs: hasta **2 en paralelo** (las demás en cola), medidores en vivo en el overlay de Descargas, staging en `.part` para que los archivos parciales nunca parezcan completos, `c` cancela. Los archivos que ya existen se omiten automáticamente.
* **Autocompletado de rutas** — el destino de descarga soporta `Tab` al estilo zsh: expande `~`, lista los directorios y completa el prefijo común.
* **Writeups** — popup de writeups de la comunidad por máquina (`w`): artículos 📝 y vídeos 🎥, `Enter` los abre en el navegador y `u` envía el tuyo. Además, una pestaña **Writeups** con todos los que has publicado.
* **Valoraciones** — medias por máquina en 4 criterios (`v`), envío de tus propias puntuaciones (1–5) y vista de lo que ya valoraste.
* **Progreso** — medidores de finalización por dificultad, estadísticas (puntos, rankings), tu perfil (nombre del diploma, miembro desde, biografía), la lista de máquinas completadas con sus ids de certificado y una vista de writeups pendientes (`p`).
* **Certificados** — descarga el PDF del certificado emitido por el admin (`c`) o todos de golpe (`C`) en `certificados/`. Si aún no se ha emitido, el dashboard te dice exactamente qué paso falta.
* **Gestión de cuenta en la app** (`a`) — cambiar de cuenta o cerrar sesión; las descargas en curso nunca se ven afectadas.
* **Auth segura** — la contraseña vive en la bóveda del sistema (Secret Service en Linux, Credential Manager en Windows, Keychain en macOS) vía `keyring`. Solo el usuario y la última carpeta de descargas tocan `~/.dl-tui/config.json`.

---

## Requisitos previos

* **SO**: Linux (objetivo principal — **desarrollado y probado en Arch Linux**); se proporcionan binarios de release para macOS y Windows.
* Una cuenta en [DockerLabs](https://dockerlabs.es).
* Un proveedor de Secret Service en Linux (p. ej. `sudo pacman -S gnome-keyring`) para guardar credenciales.

---

## Instalación

### 1. Desde un binario de release (lo más fácil)

Descarga el archivo de tu plataforma desde la página de [Releases](https://github.com/setyanoegraha/dl-tui/releases):

| Plataforma | Archivo |
| :--- | :--- |
| Linux x86_64 | `dl-v0.1.2-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `dl-v0.1.2-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `dl-v0.1.2-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `dl-v0.1.2-x86_64-pc-windows-msvc.zip` |

```bash
tar xzf dl-v0.1.2-x86_64-unknown-linux-gnu.tar.gz
install -m 755 dl ~/.local/bin/dl
```

> **Arch Linux** — dl-tui se desarrolla y prueba en Arch. El binario de Linux se compila en Ubuntu, pero funciona en Arch tal cual; solo asegúrate de tener un proveedor de Secret Service:
>
> ```bash
> sudo pacman -S --needed gnome-keyring
> ```

### 2. Desde el código fuente

```bash
git clone https://github.com/setyanoegraha/dl-tui.git
cd dl-tui
cargo install --path .
```

### 3. Directamente desde git

```bash
cargo install --git https://github.com/setyanoegraha/dl-tui.git
```

> Requiere el toolchain de Rust (1.85+): https://rustup.rs

---

## Primer arranque

Nada que configurar a mano — solo ejecuta:

```bash
dl
```

En la primera ejecución (o cuando la contraseña guardada deje de funcionar) el dashboard abre un popup **Configurar DockerLabs**:

1. Escribe tu **usuario** de DockerLabs y pulsa `Tab` / `↓`.
2. Escribe tu **contraseña** (oculta como `•••`) y pulsa `Enter`.

Las credenciales se validan con un login real **antes** de guardar nada. Si el login falla, el popup se vuelve a abrir con tu usuario intacto; `Esc` sale de la app.

---

## Guía de uso

Tres pestañas manejadas por teclado — **Máquinas**, **Progreso** y **Writeups**:

| Teclas | Acción |
| :--- | :--- |
| `Tab` / `←` `→` | Cambiar de pestaña |
| `↑` `↓` / `j` `k` | Mover la selección |
| `g` / `Home` | Ir al inicio de la lista |
| `/` | Filtrar la lista actual (escribe para filtrar, `Enter` lo mantiene, `Esc` limpia y sale) |
| `s` | **Máquinas** — ciclo de orden: orden del sitio → nombre → fecha → dificultad |
| `d` | **Máquinas** — popup de descarga: elige la carpeta de destino (se recuerda entre sesiones, `Tab` autocompleta la ruta), progreso en vivo en el overlay de Descargas |
| `w` | **Máquinas** — popup de writeups de la comunidad: `j`/`k` selecciona, `Enter` abre el enlace, `u` envía el tuyo |
| `v` | **Máquinas** — popup de valoraciones medias; `Enter` abre el formulario para enviar tus 4 puntuaciones (1–5). Tras valorar, `Tu valoración: n/n/n/n` muestra tus notas |
| `m` | **Máquinas** — marcar/desmarcar la máquina como completada (la mecánica de "resuelta" de la plataforma) |
| `i` / `Enter` | **Máquinas** — popup de descripción con todos los detalles de la máquina |
| `c` | **Máquinas y Progreso** — descarga el PDF del certificado de la máquina seleccionada a `certificados/` y lo abre. Si aún no se ha emitido, el popup te dice el paso exacto que falta (publica el writeup `w` → marca como completada `m` → espera la validación del admin) |
| `C` | **Progreso** — descarga por lotes todos los certificados emitidos a `certificados/` |
| `p` | **Progreso** — muestra solo las máquinas completadas cuyo writeup sigue pendiente |
| `a` | Popup de cuenta: `Enter` cambiar de cuenta, `l` cerrar sesión |
| `o` | Alternar el overlay de Descargas (las descargas siguen aunque lo cierres) |
| `c` | En el overlay de Descargas — cancelar la descarga activa más reciente |
| `Enter` | **Máquinas** — popup de descripción · **Progreso** — abrir el writeup enlazado · **Writeups** — abrir tu writeup |
| `r` | Volver a obtener todos los datos |
| `q` / `Esc` / `Ctrl-C` | Salir (con descargas activas, el primer `q` las lista — pulsa otra vez para abortar) |

Las acciones muestran su veredicto en un popup de resultado que persiste hasta cerrarse (`✓ ENVIADO`, `✗ RECHAZADO`, ...) y disparan una actualización de datos al cerrarse si tu progreso cambió.

### Descargas

Las máquinas se descargan directamente del almacenamiento público de DockerLabs — sin MEGA, sin descifrado y sin cuenta para los archivos en sí. Elige el destino en el popup (autocompletado con `Tab` incluido), mira los medidores en vivo en el overlay de Descargas (`o`) y cancela con `c`. Las descargas parciales viven en un archivo `.part`, así que un intento fallido o cancelado nunca deja un zip roto. Si la máquina ya está descargada, simplemente se omite.

### Certificados

Los certificados los **emite manualmente el admin de DockerLabs** — aparecen cuando tu writeup es validado y la máquina está marcada como completada. `c`/`C` descargan los PDFs emitidos a `certificados/` dentro de tu carpeta de descargas y los abren; si un certificado aún no se ha emitido, el dashboard te dice el paso exacto que falta en lugar de un error crudo.

### Gestión de la cuenta

Pulsa `a` en cualquier parte del dashboard:

- **`Enter` — cambiar de cuenta**: abre el popup de login con el usuario actual precargado. Las nuevas credenciales se validan con un login real antes de reemplazar la cuenta guardada, y el dashboard se recarga con el nuevo perfil.
- **`l` — cerrar sesión**: borra la contraseña de la bóveda del sistema y el usuario de `~/.dl-tui/config.json` (tu preferencia de carpeta de descargas se conserva), vacía el dashboard y muestra el popup de login.
- Las descargas en curso nunca se ven afectadas — usan enlaces públicos, no tu sesión.

### Dónde viven tus datos

- `~/.dl-tui/config.json` — solo dos cosas: tu **usuario** y la **última carpeta de descargas**.
- La carpeta de descargas se gestiona **desde el dashboard**: lo que confirmes en el popup de descarga (`d`) se guarda automáticamente — no hace falta editar nada a mano. Los certificados van a una subcarpeta `certificados/` de esa carpeta.
- Bóveda del sistema — tu contraseña, bajo el servicio `dl-tui`. Nunca en disco en texto plano.

---

## Actualizar

```bash
cargo install --git https://github.com/setyanoegraha/dl-tui.git --force
```

o descarga el último binario desde la página de [Releases](https://github.com/setyanoegraha/dl-tui/releases).

### Desinstalación y limpieza

```bash
cargo uninstall dl
```

Borra `~/.dl-tui/` (y la entrada `dl-tui` de la bóveda) para eliminar todos los datos locales.

---

## Aviso

dl-tui es una herramienta comunitaria **no oficial** y no está afiliada a DockerLabs. Solo usa la API pública documentada de la plataforma y las mismas páginas que usa la web, a velocidad humana — sé amable con el servicio.

---

## Agradecimientos

Gracias a [El Pingüino de Mario](https://github.com/Maalfer) y a la comunidad de DockerLabs por la plataforma, y al equipo de [ratatui](https://github.com/ratatui/ratatui) por el toolkit.

Proyecto hermano: [hmv-tui](https://github.com/setyanoegraha/hmv-tui) — el mismo concepto de dashboard para HackMyVM.

---

Hecho con ❤️ por [Ouba](https://github.com/setyanoegraha).

*¡Feliz hacking en DockerLabs!*
