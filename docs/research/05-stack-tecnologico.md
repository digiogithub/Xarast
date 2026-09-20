# Xarast — Stack tecnológico recomendado

> **Nota de sala limpia.** Este documento analiza las *implicaciones de licencia*
> y el stack técnico de Xarast frente a Xara Xtreme (GPL-2.0-only), con fines de
> interoperabilidad y de decisión de diseño. No reproduce código fuente del
> original —la única cita textual es el aviso de licencia del original, §1.1,
> reproducido como prueba documental—; las referencias `fichero:línea` apuntan al
> árbol de referencia en `xara-xtreme/` y sirven solo para localizar la lógica
> descrita. Xarast se implementa desde esta especificación, no traduciendo el
> original.

> **Documento:** `docs/research/05-stack-tecnologico.md`
> **Fecha de investigación:** 19 de septiembre de 2026
> **Método:** verificación directa contra la API de `crates.io`, `docs.rs`, `README`/`CHANGELOG` de los repositorios y búsqueda web. Todas las versiones, licencias y fechas de actualización de esta tabla se consultaron **el 19-09-2026**; no proceden de conocimiento previo del modelo.
> **Alcance:** Linux/Wayland como plataforma primaria; Windows y macOS en fases posteriores. Empaquetado inicial AppImage.

---

## 0. Resumen ejecutivo

| Área | Elección | Alternativa (plan B) |
|---|---|---|
| Licencia del proyecto | **GPL-3.0-or-later** (limpieza de sala respecto a XaraLX) | LGPL-3.0+ para el core |
| UI | **egui 0.36 + egui_tiles + egui_extras**, sin `eframe` | iced 0.14 |
| Ventana/entrada | **winit 0.31** (rama beta, API `Pointer`/`TabletTool`) | winit 0.30.13 + `octotablet` |
| Tableta/presión | **winit 0.31 `TabletToolData`** (Wayland/Windows) + `octotablet` en X11/macOS | `wintab_lite` (Win), `input` (libinput) |
| GPU | **wgpu 30.0** (Vulkan/Metal/DX12/GLES) | fallback GL + lavapipe |
| Rasterizado 2D | **vello_cpu 0.2 (referencia) + vello 0.10 (GPU)** tras fachada propia | lyon 1.0 + pipelines wgpu propias |
| Texto | **parley 0.11 + harfrust + fontique + skrifa + peniko** | cosmic-text 0.19 |
| Imagen | **image 0.25 + zune-jpeg + png + image-webp + ravif + kamadak-exif + resvg/usvg 0.48** | zune-image como fachada |
| Contenedor | **zip 8.6 (ZIP + zstd/deflate) + zstd 0.14** | tar + zstd |
| Geometría | **kurbo 0.13 + i_overlay 9.0 (+ i_curve en observación)** | lyon + flo_curves |
| Serialización | **serde 1.0 + serde_json (manifiestos) + rkyv 0.8 (autosave/scratch)** | postcard |
| Undo | **command pattern invertible + árbol persistente (imbl 7.0) + snapshots periódicos** | snapshots puros |
| Concurrencia | **rayon 1.12 + hilo de render dedicado + crossbeam-channel**; tokio sólo mínimo para portales | — |
| Empaquetado Linux | **AppImage** (linuxdeploy + appimagetool runtime estático), base glibc 2.35 (Ubuntu 22.04) | Flatpak/Flathub como 2º canal |
| Testing | **golden images (backend CPU) + image-compare + insta + criterion + cargo-fuzz + nextest** | divan, dify |

---

## 1. Restricción de licencia — LEER ANTES DE ELEGIR NADA

> **Nota de actualización (pasada de higiene de sala limpia).** El análisis de esta
> sección parte del supuesto de que Xarast podría ser un **trabajo derivado** de
> Xara LX y, por tanto, quedar cubierto por su GPL-2.0-only. Ese supuesto **ya no
> se sostiene**: Xarast se desarrolla en **sala limpia** a partir de las
> especificaciones de `docs/research/`, sin copiar ni traducir código del
> original, y la decisión de proyecto vigente es publicarlo bajo
> **MIT OR Apache-2.0**. Con esa premisa, la incompatibilidad GPL-2.0-only ↔
> Apache-2.0 descrita más abajo **no aplica a Xarast**, y tampoco aplica la
> «ACCIÓN INMEDIATA» de la sección DECISIONES (sustituir el `LICENSE` por
> GPL-3.0). El resto del análisis (licencias de cada crate, `cargo-deny`,
> `cargo-about`) sigue siendo válido y necesario.

Esta es la decisión más condicionante de todo el documento y hay un problema real que hay que resolver **hoy**, no en la fase 5.

### 1.1 Qué licencia tiene exactamente el original

Se verificó la cabecera de licencia del código fuente real de Xara LX (mirror `samuell/xara-xtreme`, `Kernel/group.h`). Dice literalmente:

```
Xara LX is free software; you can redistribute it and/or modify it
under the terms of the GNU General Public License version 2 as published
by the Free Software Foundation.
```

Es **GPL-2.0-only** ("version 2", sin "or later"). Además incluye una sección *ADDITIONAL RIGHTS* que permite enlazar con wxWidgets, wxXtra y la librería propietaria **CDraw** (el motor de render, que nunca se liberó como fuente).

### 1.2 La consecuencia práctica: Apache-2.0 es incompatible con GPL-2.0-only

Apache-2.0 **no es compatible** con GPLv2 (cláusula de patentes); sí lo es con GPLv3. Y resulta que buena parte del ecosistema Rust gráfico es Apache-2.0 **en solitario**, sin doble licencia MIT:

| Crate | Licencia | ¿Compatible con GPL-2.0-only? | ¿Compatible con GPL-3.0+? |
|---|---|---|---|
| `winit` | Apache-2.0 | ❌ **NO** | ✅ |
| `accesskit_winit` | Apache-2.0 | ❌ **NO** | ✅ |
| `xilem` / `masonry` | Apache-2.0 | ❌ NO | ✅ |
| `gpui` | Apache-2.0 | ❌ NO | ✅ |
| `parry2d`, `nalgebra` | Apache-2.0 | ❌ NO | ✅ |
| `flo_curves` | Apache-2.0 | ❌ NO | ✅ |
| `insta` | Apache-2.0 | ❌ NO | ✅ |
| `libdeflater` | Apache-2.0 | ❌ NO | ✅ |
| `unicode-linebreak` | Apache-2.0 | ❌ NO | ✅ |
| `slint` | **GPL-3.0-only** OR comercial | ❌ NO | ✅ (fuerza GPLv3) |
| `im` / `imbl` | MPL-2.0+ | ✅ (MPL es compatible GPL) | ✅ |
| `icu_properties` | Unicode-3.0 | ✅ | ✅ |
| `zstd` (envuelve zstd C) | BSD-3 (zstd upstream es BSD-3 **OR** GPL-2.0) | ✅ | ✅ |
| `ravif`, `rav1e`, `tiny-skia`, `kamadak-exif` | BSD-2/BSD-3 | ✅ | ✅ |
| resto del stack | MIT OR Apache-2.0 / MIT | ✅ | ✅ |

**`winit` es Apache-2.0 puro.** No hay alternativa realista a winit en Rust para Wayland moderno. Por tanto:

> ### ⚠️ DECISIÓN LEGAL OBLIGATORIA
> **Xarast NO puede ser GPL-2.0-only.** Debe licenciarse como **GPL-3.0-or-later** (o GPL-2.0-**or-later**, que permite relicenciar el combinado a v3). Y como consecuencia directa:
> **Xarast debe ser una reimplementación de sala limpia**: no se puede copiar, traducir línea a línea ni derivar código de XaraLX, porque ese código es GPL-2.0-**only** y arrastraría al proyecto a una licencia incompatible con `winit`.
>
> Lo que sí es lícito y seguro: estudiar el **formato de fichero `.xar`** (los formatos no son obra protegible), la documentación pública del formato, el comportamiento observable y la UX. Documentar el formato en un `docs/format/` propio antes de escribir el parser (procedimiento clean-room clásico: un equipo lee, otro implementa).

### 1.3 Incoherencia a corregir ya

`/home/user/Xarast/LICENSE` contiene actualmente **MIT** (Copyright 2026 Jose Francisco Rives). Eso contradice el objetivo declarado de distribuir bajo GPL. Hay que decidir explícitamente:

- **Opción A (recomendada):** `GPL-3.0-or-later` para la aplicación completa. Coherente con el espíritu del original, compatible con todo el stack, y permite que el resultado sea el "sucesor espiritual" de Xara Xtreme.
- **Opción B:** binario GPL-3.0+, pero los crates de bajo nivel reutilizables (`xarast-geom`, `xarast-xar`) publicados como `MIT OR Apache-2.0` para que la comunidad los use. Es lo que hacen Linebender y Graphite; maximiza el impacto del trabajo.
- **Opción C (no recomendada):** todo MIT. Legal (no hay obligación de heredar la GPL si es sala limpia), pero rompe la promesa implícita del proyecto.

### 1.4 Herramientas de cumplimiento (obligatorias en CI desde el día 1)

- **`cargo-deny` 0.20.2** (MIT OR Apache-2.0) — `deny.toml` con `[licenses] allow = [...]`, prohibiendo explícitamente `GPL-3.0-only`, `AGPL-*`, `LicenseRef-Slint-*` y cualquier licencia no listada.
- **`cargo-about` 0.9.2** — genera el `THIRD-PARTY-LICENSES.html` que debe ir dentro del AppImage y del diálogo "Acerca de".

---

## 2. Toolkit de UI

### 2.1 Qué necesita realmente un editor vectorial profesional

Antes de comparar: la UI de Xara/Illustrator/Affinity no es una UI "de aplicación", es una UI **de herramienta**. Las exigencias que de verdad discriminan:

1. **Densidad**: 400+ controles visibles, filas de 18–22 px, sin padding de app móvil.
2. **Acoplables (docking)** con pestañas, arrastre entre grupos y persistencia de layout.
3. **Canvas propio a 60–144 fps** con integración wgpu *en la misma superficie* (no un iframe/textura desincronizada).
4. **Actualización parcial**: mover un tirador no puede repintar 400 widgets.
5. **Galerías virtualizadas** (miles de miniaturas, capas, fuentes, colores).
6. **Campos numéricos con arrastre, unidades y expresiones** (`12mm + 3pt`).
7. **Accesibilidad real** (AccessKit → AT-SPI en Linux) — es requisito legal en contratación pública europea.
8. **IME y texto complejo** para la herramienta de texto.
9. **DPI fraccional Wayland** correcto (`wp_fractional_scale_v1`).

### 2.2 Comparativa

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **egui** | **0.36.2** (2026-09-08) | MIT OR Apache-2.0 | ⭐ Excelente. 5,4 M descargas/90 d. Releases cada ~2 meses | Modo inmediato = estado siempre coherente con el documento (ideal para un editor); integración wgpu de primera clase (`egui-wgpu`); AccessKit integrado (Win/macOS/**Linux AT-SPI**); desde 0.34 usa **skrifa + vello_cpu** (hinting + fuentes variables, texto nítido); densidad configurable al píxel; ecosistema enorme (`egui_tiles`, `egui_dock`, `egui_extras`, `egui_kittest`); precedente profesional real: **Rerun** | Repinta todo el frame (mitigable con `request_repaint_after` y áreas); layout "una pasada tarde" incómodo en diseños complejos; sin animaciones declarativas; el `TextEdit` no es un motor de texto profesional (habrá que hacer el nuestro con parley para el canvas) | ✅ **ELEGIDO** |
| **iced** | 0.14.0 (2025-12-07) | MIT | ⭐ Buena, pero releases lentas (0.14 lleva 9 meses) | Elm/retenido, actualización parcial nativa, `iced_wgpu` propio con culling de primitivas, *time-travel debugging*, *headless testing*, *hot reloading*, soporte IME; base de **COSMIC DE** (prueba de escala) | Curva Elm dura para 400 widgets; el widget `canvas` es 2D propio (no wgpu directo, hay que usar `shader` widget); docking no existe de serie; AccessKit aún no integrado; menos ecosistema de widgets "pro" | 🟡 **Plan B sólido** |
| **slint** | 1.18.0 (2026-09-16) | **GPL-3.0-only** OR royalty-free OR comercial | ⭐ Excelente (empresa detrás) | Muy pulido, DSL declarativo, previsualizador en vivo, buen rendimiento, backend `femtovg`/`skia` | ❌ **GPL-3.0-only fuerza GPLv3** e impide cualquier futuro relicenciamiento; la vía royalty-free exige mostrar badge "AboutSlint"; el DSL `.slint` es un lenguaje más que mantener; integración con un canvas wgpu propio es incómoda (hay que pasar por `Window::set_rendering_notifier`); no está pensado para densidad tipo CAD | ❌ Descartado (licencia + fit) |
| **gpui** | 0.2.2 (2025-10-22) | Apache-2.0 | 🟠 Se desarrolla **dentro de Zed**, no como producto independiente; pre-1.0 con roturas frecuentes; la comunidad reporta que se ha abandonado como esfuerzo open-source independiente (proliferación de forks tipo `zui`) | Rendimiento excepcional, texto excelente, arquitectura de elementos muy buena | Documentación casi inexistente; API inestable; en Linux su soporte es el que necesita Zed, no el que necesita un editor gráfico; no hay AccessKit | ❌ Descartado (riesgo de gobernanza) |
| **xilem** / **masonry** | 0.4.0 (2025-10-29) | Apache-2.0 | 🟡 Activo (Linebender) pero **10,7 K descargas totales** = aún no se usa en producción | Arquitectura reactiva de primera línea; misma familia que kurbo/parley/vello (integración natural); AccessKit de origen | No está listo para producción (lo dicen ellos); catálogo de widgets mínimo; sin docking; sin galerías; Apache-2.0 puro | 🔭 **Vigilar** (revisar en 2027) |
| **dioxus** (desktop) | 0.7.10 (2026-07-31) | MIT OR Apache-2.0 | ⭐ Muy activo, gran comunidad | DX excelente tipo React, hot reload, buen tooling | El *desktop* real es WebView (o `blitz`, aún inmaduro) → nada de canvas wgpu de alto rendimiento ni control de latencia de stylus; modelo web = densidad y atajos peleados | ❌ Descartado (arquitectura) |
| **floem** | 0.2.0 (**2024-11-14**) | MIT | 🔴 **Sin release en crates.io desde hace 22 meses** | Reactividad fina (signals), buen rendimiento, base de Lapce | Publicación estancada; ecosistema pequeño; sin AccessKit; sin docking | ❌ Descartado (mantenimiento) |
| **makepad** | `makepad-widgets` 1.0.0 (2025-05-13); el crate `makepad` es un placeholder de 2019 | MIT OR Apache-2.0 | 🟠 Activo en GitHub pero 1,2 K descargas/90 d | Shaders en el DSL, rendimiento brutal, diseñado justo para herramientas creativas | Ecosistema cerrado sobre sí mismo; casi nadie externo lo usa; sin AccessKit; documentación escasa; IME/texto complejo limitado | ❌ Descartado (riesgo) |
| **freya** | 0.4.3 / 0.5.0-rc.6 (2026-09-13) | MIT | 🟡 Activo, un mantenedor principal | Skia + Dioxus core, API agradable, accesibilidad vía AccessKit | 6,6 K descargas/90 d; depende de Skia (binario grande, build pesado); bus factor 1 | ❌ Descartado (madurez) |
| **Canvas propio + UI inmediata a medida** | — | — | — | Control total, cero dependencias de UI | 2–3 años-persona sólo para llegar a la paridad de widgets de egui; reimplementar IME, AccessKit, selección de texto, accesibilidad… | ❌ Descartado, **pero** ver §2.4: adoptamos su mitad buena |

### 2.3 Recomendación firme: **egui 0.36, usado como librería, no como framework**

Se elige **egui** por tres razones que pesan más que el resto:

1. **El modelo inmediato encaja con el problema.** En un editor, la UI es una *proyección* del documento (capas, atributos del objeto seleccionado, galerías). Con modo retenido hay que sincronizar dos árboles de estado y eso es la fuente número uno de bugs en editores. Con egui, si el documento cambia, la UI ya está bien en el siguiente frame. El coste (repintar) es asumible: un frame de UI de egui en una GPU moderna cuesta ~0,5–1,5 ms, y ya hay `request_repaint_after` para quedarse a 0 fps cuando no pasa nada.
2. **La integración con wgpu es directa y en la misma superficie.** `egui-wgpu` nos da un `Renderer` al que le pasamos *nuestro* `wgpu::Device` y *nuestro* `RenderPass`. El canvas de Xarast y la UI comparten dispositivo, cola y swapchain; no hay copias intermedias ni tearing entre el lienzo y los tiradores.
3. **Accesibilidad y texto ya resueltos y verificados.** AccessKit 0.25 tiene adaptador Unix (AT-SPI D-Bus) en "paridad aproximada" con Windows/macOS, incluidos campos de texto de una y varias líneas. Y desde egui 0.34 el texto se rasteriza con **skrifa + vello_cpu**, con hinting y fuentes variables.

### 2.4 Matiz arquitectónico importante: **no usar `eframe`**

Verificado: `eframe 0.36.2` y `egui-winit 0.36.2` dependen de **`winit ^0.30.13`**. Pero el soporte de **lápiz/tableta con presión** sólo existe en **winit 0.31** (ver §3). Es decir: si usamos `eframe`, renunciamos a la presión del stylus hasta que egui suba de winit, lo cual es inaceptable para una herramienta de dibujo.

**Decisión:** escribir nuestro propio *shell* (`xarast-shell`) que:

- posee el bucle `winit 0.31` (`ApplicationHandler`, `Window` como trait),
- posee el `wgpu::Surface`/`Device`/`Queue`,
- traduce eventos winit 0.31 → `egui::RawInput` (es una reimplementación de `egui-winit`, ~1000–1500 líneas; buena parte se puede portar desde el original MIT/Apache),
- conecta `accesskit_winit 0.34` directamente,
- y dibuja: `[pase canvas Xarast] → [pase egui-wgpu]` en el mismo `CommandEncoder`.

Esto es exactamente el enfoque "canvas propio + UI inmediata" de la lista, pero apoyándonos en egui para los widgets en vez de escribirlos. Coste: ~2 semanas de trabajo inicial + mantenimiento del shim en cada subida de versión. Beneficio: independencia total de la cadencia de releases de `eframe`, presión de stylus desde el día 1, y control del timing de presentación (crítico para la latencia del trazo).

### 2.5 Complementos de UI elegidos

| Crate | Versión | Licencia | Para qué | Notas |
|---|---|---|---|---|
| **egui_tiles** | 0.17.1 (2026-08-18) | MIT OR Apache-2.0 | **Docking / paneles acoplables** | De Rerun; contenedores tabs/linear/grid anidables, drag&drop entre grupos, serializable. Es lo que usa una app profesional real |
| `egui_dock` | 0.21.1 (2026-08-06) | MIT | Alternativa de docking | Más "IDE-like"; buen mantenimiento (4,7 M descargas) pero `egui_tiles` tiene mejor modelo de árbol |
| **egui_extras** | 0.36.2 | MIT OR Apache-2.0 | `TableBuilder` (árbol de capas, galerías virtualizadas), `DatePicker`, loaders de imagen | Oficial |
| **accesskit_winit** | 0.34.0 | Apache-2.0 | AT-SPI/UIA/NSAccessibility | Nota: Apache-2.0 puro → refuerza GPLv3 |
| **arboard** | 3.6.1 | MIT OR Apache-2.0 | Portapapeles (imágenes + texto) | Con `smithay-clipboard` en Wayland |
| **rfd** | 0.17.2 | MIT | Diálogos de fichero | Usa **portales XDG** en Wayland → funciona dentro de Flatpak/AppImage sandbox |
| **ashpd** | 0.13.13 | MIT | Portales XDG (settings, tema oscuro/claro, screenshot, file chooser) | Para detectar `org.freedesktop.appearance color-scheme` y respetar el tema del sistema |

### 2.6 Riesgos del área UI

| Riesgo | Probabilidad | Impacto | Mitigación |
|---|---|---|---|
| El shim propio winit 0.31 ↔ egui se desincroniza con upstream | Media | Medio | Mantenerlo en un crate aislado (`xarast-egui-winit`), con tests; volver a `egui-winit` upstream cuando suba a 0.31 |
| winit 0.31 sigue en beta y rompe API | **Alta** | Medio | Fijar `=0.31.0-beta.3`, encapsular todo winit tras `xarast-shell`; presupuestar 1 semana por cada beta |
| Modo inmediato insuficiente para paneles muy pesados (galería de 5000 fuentes) | Media | Bajo | Virtualización con `egui_extras::TableBuilder` + caché de miniaturas en textura atlas |
| AccessKit en Linux incompleto para widgets exóticos | Media | Bajo | Los controles críticos (menús, campos, árbol de capas) usan widgets estándar de egui |

---

## 3. Ventanas y entrada

### 3.1 winit: estado verificado en 2026

- **`winit 0.30.13`** — estable, publicado 2026-09-04, Apache-2.0, 10,4 M descargas/90 d.
- **`winit 0.31.0-beta.3`** — publicado 2026-09-04. Es una **reestructuración grande**: el crate se parte en `winit-core`, `winit-wayland`, `winit-x11`, `winit-win32`, `winit-appkit`, `winit-web`, `winit-android`, `winit-orbital`; `ActiveEventLoop` y `Window` pasan a ser **traits**; `inner_*` se renombra a `surface_*`.

Novedades de 0.31 verificadas en el changelog oficial, relevantes para Xarast:

**Entrada de lápiz/tableta (lo decisivo):**
- *"Add Pen input support on Wayland, Windows, and Web via new Pointer event."*
- *"Add `PointerKind`, `PointerSource`, `ButtonSource`, `FingerId`, `primary` and `position` to all pointer events."*
- `PointerSource::TabletTool { kind: TabletToolKind, data: TabletToolData }`, donde **`TabletToolData`** expone (verificado en docs.rs):
  - `force: Option<Force>` — presión
  - `tangential_force: Option<f32>` — presión del barrel (−1..1)
  - `twist: Option<u16>` — rotación del útil, 0..359°
  - `tilt: Option<TabletToolTilt>` — inclinación en grados
  - `angle: Option<TabletToolAngle>` — posición angular en radianes
- En iOS: *"Apple Pencil support with force, altitude, and azimuth data."*

**Wayland:**
- `HoldGesture`, `PanGesture`, `PinchGesture`, `RotationGesture` (gestos de trackpad → zoom/pan del lienzo gratis).
- `ext-background-effect-v1` (blur/vibrancy del compositor).
- `Window::set_window_icon` implementado.
- Escalado fraccional (`wp_fractional_scale_v1`) soportado; los cursores personalizados también se escalan fraccionalmente.
- Arreglo de error de protocolo con cursores personalizados en `wl_surface` < v3.
- CSD mediante SCTK + `sctk-adwaita` (winit lo gestiona; en GNOME es obligatorio).

**Drag & drop rediseñado:** desaparecen `DroppedFile`/`HoveredFile`/`HoveredFileCancelled`; entran `DragEntered`/`DragMoved`/`DragDropped`/`DragLeft`. Además se elimina `url` de `winit-core`: hay que usar `SendData::Uris` / `TypedData::try_as_uris` con URIs `file:`.

### 3.2 Comparativa de capa de ventanas

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **winit** | 0.31.0-beta.3 / 0.30.13 estable | Apache-2.0 | ⭐⭐ Referencia absoluta. 53,8 M descargas totales | Único con Wayland nativo completo (fractional scaling, CSD, gestos, portales indirectos), **tableta con presión/tilt/twist en 0.31**, DnD moderno, integración directa con wgpu y accesskit | Apache-2.0 puro (→ obliga GPLv3); API rota entre menores; 0.31 aún beta; no expone tableta en X11/macOS | ✅ **ELEGIDO** |
| **sdl3** | 0.20.0 (2026-09-07) | MIT (bindings) / SDL es Zlib | ⭐ Muy activo (207 K desc.) | SDL3 tiene API de tableta/pen unificada y madura en **todas** las plataformas, incluida macOS; gamepads, audio, portapapeles | Dependencia C externa (empaquetado AppImage más pesado); no encaja con el modelo de `ApplicationHandler`; su Wayland es bueno pero menos "nativo" en detalles (CSD delegadas a libdecor); duplicaría el bucle con egui | 🟡 Plan B (ver §3.4) |
| **smithay-client-toolkit** | 0.21.1 (2026-07-23) | MIT | ⭐⭐ Excelente, es la base de winit | Control absoluto de Wayland (`tablet_v2`, `text-input-v3`, `cursor-shape-v1`, fractional scale) | **Sólo Linux/Wayland**; habría que escribir el backend Windows/macOS a mano | 🔧 Uso quirúrgico: acceder a protocolos que winit no exponga |
| **glazier** | — | Apache-2.0 | 🔴 **Archivado** por Linebender (absorbido por winit/masonry) | — | Muerto | ❌ Descartado |

### 3.3 Tabletas gráficas con presión — el estado real en Rust (2026)

Este es el punto más débil del ecosistema y merece un plan explícito.

| Opción | Versión | Licencia | Plataformas | Datos | Veredicto |
|---|---|---|---|---|---|
| **winit 0.31 `TabletTool`** | 0.31.0-beta.3 | Apache-2.0 | Wayland ✅, Windows (Ink) ✅, Web ✅, iOS ✅ · X11 ❌, macOS ❌ | force, tangential_force, tilt, twist, angle | ✅ **Vía principal** |
| **octotablet** | 0.1.0 en crates.io (**2024-03-16**), repo con actividad | MIT | Wayland `tablet_unstable_v2` ✅ completo, Windows Ink/RTS ✅ completo, X11/XInput2 🟡 "intentado", macOS ❌ | presión, tilt, distancia, rueda, botones de pad, *pads* y *rings* | 🟡 **Complemento** para X11 y para botones/anillos del pad, que winit no expone. Riesgo: crates.io congelado en 0.1.0, 654 descargas/90 d, bus factor 1 → **vendorizar** |
| **`input` (libinput)** | 0.10.0 (2026-04-05) | MIT | Linux, requiere acceso a `/dev/input` o seat via logind | Todo lo de libinput | ❌ Para una app de escritorio bajo Wayland no procede (el compositor ya posee los dispositivos) |
| **`wintab_lite`** | 1.0.1 (2024-04-19) | MIT | Windows (Wintab) | presión, tilt | 🟡 Sólo si Windows Ink da problemas con Wacom/Huion externas (caso conocido en Photoshop) |
| **macOS NSEvent** | — | — | macOS | `NSEventTypeTabletPoint`: pressure, tilt, rotation, tangentialPressure | 🔧 **A implementar a mano** con `objc2`/`objc2-app-kit` en la fase macOS. Es ~200 líneas |

**Arquitectura recomendada:** un trait propio `xarast_input::TabletSource` con implementaciones intercambiables:

```
TabletSource
 ├── WinitTabletSource     (Wayland, Windows, Web, iOS)  ← por defecto
 ├── OctotabletSource      (X11, y pads/rings en Wayland) ← feature opcional
 └── AppKitTabletSource    (macOS, propio)                ← fase 3
```

Y un `StrokeSample { x, y, pressure, tilt_x, tilt_y, twist, timestamp, source }` normalizado, con **interpolación/predicción** propia. Importante: para latencia de trazo hay que muestrear a la frecuencia del dispositivo (Wacom ~200 Hz), no a la del frame; winit entrega eventos coalescidos por el compositor, así que hay que acumular todos los `PointerMoved` del frame, no sólo el último.

### 3.4 Riesgos del área de entrada

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| winit 0.31 no llega a estable en 6 meses | Media | **Alto** | Fijar la beta exacta; el shim aísla la API. Si se atasca: winit 0.30 + `octotablet` (ambos funcionan en paralelo) |
| Presión no disponible en X11 | Alta | Medio | `octotablet` (XInput2); documentar X11 como "soporte degradado" — la prioridad es Wayland |
| Compositores Wayland sin `tablet_v2` (algunos wlroots antiguos) | Baja | Bajo | Degradar a ratón con presión constante; avisar en la UI |
| Latencia de trazo perceptible | Media | **Alto** | Presentación con `PresentMode::Mailbox`/`Fifo` según caso; renderizar el trazo en curso en un pase separado ligero; considerar predicción de 1 frame |

---

## 4. GPU

### 4.1 wgpu 30

Verificado: **`wgpu 30.0.1`**, publicado **2026-08-22**, `MIT OR Apache-2.0`, 10,6 M descargas/90 d. **MSRV Rust 1.87** con política explícita de no superar `stable − 3`.

Backends soportados en v30: **Vulkan, Metal, DX12, GLES/OpenGL, WebGPU** (+ Vulkan en OpenHarmony).

Novedades de v30 relevantes:
- `SurfaceConfiguration.color_space` → **HDR y gama amplia** (importante para una app gráfica que algún día querrá Display-P3/Rec.2020).
- `TextureViewDescriptor.swizzle` (`TEXTURE_COMPONENT_SWIZZLE`) → útil para canales de máscara/alfa sin copias.
- `Queue::present(surface_texture)` sustituye a `SurfaceTexture::present()`.
- Roturas: la interpolación de enteros ya no es `flat` por defecto (hay que anotar `@interpolate(flat)`); slots de vertex buffer y bind group layouts pasan a `Option<_>`.

### 4.2 Comparativa de capa GPU

| Opción | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **wgpu** | 30.0.1 | MIT OR Apache-2.0 | ⭐⭐ Firefox + Deno + Bevy detrás | Un solo shader (WGSL) para Vulkan/Metal/DX12/GLES; validación excelente; `wgpu-profiler` y captura RenderDoc; downlevel automático a GLES para GPUs viejas; ya lo usa egui | Capa de abstracción = algún coste; compute shaders limitados en el downlevel GLES (afecta a Vello GPU) | ✅ **ELEGIDO** |
| `ash` (Vulkan directo) | — | MIT | Activo | Máximo control, sin sobrecoste | Sólo Vulkan → habría que escribir Metal y DX12 aparte. Multiplica el trabajo por 3 | ❌ |
| `glow` (GL 3.3/ES 3.0) | — | MIT | Activo | Funciona en todo lo que respire | Sin compute; sin las garantías de wgpu | 🟡 Sólo como backend de wgpu (`--features glow`) |
| CPU puro (`vello_cpu`/`tiny-skia`) | — | — | — | Cero dependencias GPU | 5–30× más lento | 🟡 Fallback obligatorio, ver §4.3 |

### 4.3 GPUs antiguas y renderizado por software — estrategia escalonada

Una app de diseño tiene que arrancar **siempre**, incluso por SSH+X11 o en una VM sin GPU. Plan de 4 escalones con degradación automática:

| Nivel | Requisito | Backend wgpu | Rasterizador de lienzo | Rendimiento esperado |
|---|---|---|---|---|
| **0 — Rápido** | Vulkan 1.1 + compute (GPU ≥ 2016) | Vulkan/Metal/DX12 | `vello` (compute) | 60–144 fps, documentos grandes |
| **1 — Estándar** | Vulkan/DX12 sin compute avanzado, o GL 4.3 | Vulkan/DX12/GLES | `vello_hybrid` (CPU stripping + GPU fill) | 60 fps |
| **2 — Compatible** | GL 3.3 / GLES 3.0 (Intel HD 2010+) | GLES vía `glow` | `vello_cpu` a texturas + composición GPU | 20–60 fps |
| **3 — Software** | Sin GPU: **lavapipe** (`VK_ICD_FILENAMES`) o **llvmpipe** | Vulkan (lavapipe) o GLES (llvmpipe) | `vello_cpu` con `rayon` | 5–20 fps, usable para edición |

Implementación:
- Al arrancar, `Instance::request_adapter` con `power_preference: HighPerformance`; si falla, reintento con `force_fallback_adapter: true` (que es justo lavapipe/llvmpipe/WARP).
- Inspeccionar `Adapter::get_downlevel_capabilities()` y `Features::…` para elegir el nivel.
- **Variable de entorno de escape** `XARAST_RENDERER=cpu|hybrid|gpu` y `WGPU_BACKEND=vulkan|gl` documentadas, porque el 80 % de los bugs de soporte en Linux se resuelven con eso.
- **No** empaquetar Mesa en el AppImage: se usa el del sistema (ver §11.3).

### 4.4 Rasterizado 2D del lienzo (el corazón del asunto)

Esto no estaba en la lista de áreas pero es la decisión técnica con más consecuencias, así que la documento.

| Opción | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **vello_cpu** | 0.2.0 (2026-08-07) | Apache-2.0 OR MIT | ⭐ Linebender; 3,07 M desc./90 d (¡lo usa egui!) | El README de Vello lo llama *"the most mature choice"*; **determinista** → perfecto para golden tests; SIMD; funciona en cualquier máquina | CPU-bound en documentos grandes | ✅ **Backend de referencia + fallback** |
| **vello** (GPU compute) | 0.10.0 (2026-08-14) | Apache-2.0 OR MIT | ⭐ Activo | Pipeline de prefix-sum: sorting y clipping en GPU sin texturas intermedias; escala a escenas enormes | *"intended to become the primary renderer… as it matures; Vello CPU is currently overall more mature"*; exige compute; artefactos de conflation conocidos | ✅ **Backend acelerado** (nivel 0) |
| **vello_hybrid** | 0.2.0 (2026-08-07) | Apache-2.0 OR MIT | 🟡 Nuevo (41 K desc. totales) | CPU hace el *stripping*, GPU rellena → funciona sin compute, incluso en WebGL2 | Muy joven | 🟡 Nivel 1 |
| **lyon** | 1.0.19 (2026-03-08) | MIT OR Apache-2.0 | ⭐ Mantenido (2 M desc./90 d) | Teselación a triángulos probadísima (base de Firefox/WebRender históricamente); control total del pipeline; MSAA sencillo | Hay que implementar a mano: clipping, grupos de transparencia, blend modes, gradientes complejos, feathering; calidad de antialiasing inferior al AA analítico | 🟡 **Plan B / teselador puntual** |
| **tiny-skia** | 0.12.0 (2026-02-02) | BSD-3 | 🟡 Mantenimiento (Linebender) | Port de Skia, muy correcto, base de resvg | Sólo CPU, sin SIMD moderno comparable a vello_cpu, API cerrada | 🟡 Lo arrastraremos vía resvg |
| Skia (`skia-safe`) | — | BSD-3 | ⭐ Google | Lo más completo que existe | +40 MB de binario, build de 40 min, C++ | ❌ Descartado (peso y build) |

**Recomendación:** definir una **fachada propia `xarast-raster`** con un `trait Rasterizer`, y **dos implementaciones desde el día 1** (CPU y GPU), ambas alimentadas por el mismo modelo de escena basado en **`peniko` 0.6** (tipos de brush/gradient/blend compartidos con kurbo y vello). Motivos:

1. La versión CPU es la **oráculo** de los golden tests (§12) y el fallback del nivel 3. Tenerla desde el principio evita que la GPU se convierta en "la única verdad" y que los tests sean irreproducibles en CI.
2. Los efectos característicos de Xara (**feathering**, mezclas/blends entre objetos, sombras, transparencias por objeto) **no** los cubre Vello. Serán **pases wgpu propios** que componen texturas de capa. Es decir: Vello rasteriza *paths* a texturas de capa; **nuestro compositor** (shaders propios) hace blend modes, feather y efectos en vivo. Esta separación hace que un cambio de rasterizador (Vello → lyon) sea localizado.

---

## 5. Texto

### 5.1 Requisitos

Texto en path, kerning manual (par a par, editable), features OpenType (`liga`, `smcp`, `onum`, `ss01`…), bidi, fuentes variables, fuentes del sistema en las 3 plataformas, y — crucial — **acceso a los contornos de los glifos** para convertir texto a curvas.

### 5.2 Comparativa

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **parley** | **0.11.1** (2026-08-16) | Apache-2.0 OR MIT | ⭐⭐ Linebender; 1,72 M desc./90 d | Pila completa y coherente: shaping con **harfrust**, bidi/segmentación con **ICU4X**, fuentes del sistema con **fontique**, parsing con **skrifa**, tipos con **peniko**; utilidades de selección/edición; feature `accesskit`; feature `complex-scripts` | Pre-1.0 (API puede romper); documentación de plataformas escasa; MSRV 1.88 | ✅ **ELEGIDO** |
| **harfrust** | **0.13.3** (2026-08-25) | MIT | ⭐⭐ De la **organización HarfBuzz**; 5,3 M desc./90 d | Port Rust de HarfBuzz por el propio equipo de HarfBuzz → es el sucesor de facto de rustybuzz; features OpenType completas; sin dependencias C | Relativamente nuevo como nombre (pero el código viene de rustybuzz/HarfBuzz) | ✅ **ELEGIDO** (vía parley) |
| **fontique** | 0.11.1 (2026-08-16) | Apache-2.0 OR MIT | ⭐ Linebender | Enumeración de fuentes del sistema + **fallback por script** en Linux (fontconfig), Windows (DirectWrite) y macOS (CoreText); colecciones y familias genéricas | Pre-1.0 | ✅ **ELEGIDO** |
| **skrifa** | **0.47.0** (2026-09-08) | MIT OR Apache-2.0 | ⭐⭐ **Google Fonts** (proyecto `fontations`); 13,9 M desc./90 d | Lectura de contornos, métricas, variaciones, hinting, COLRv1, bitmaps; es el motor de fuentes de Chrome/Skia en Rust | — | ✅ **ELEGIDO** (texto→curvas) |
| **cosmic-text** | 0.19.0 (2026-04-22) | MIT OR Apache-2.0 | ⭐ System76/COSMIC; 3,07 M desc./90 d | Muy probado (es el motor de texto de COSMIC DE); editor con cursor/selección; shaping con rustybuzz, swash para rasterizar | Orientado a *editores de texto*, no a *tipografía de diseño*: control de features OpenType menos directo, sin API pensada para texto en path ni kerning manual; arrastra swash+rustybuzz (superados por skrifa+harfrust) | 🟡 **Plan B** |
| **swash** | 0.2.10 (2026-07-17) | Apache-2.0 OR MIT | 🟡 Un autor (dfrg), 4,9 M desc./90 d | Shaping + scaling + rasterizado en un crate; muy rápido | Bus factor 1; el ecosistema (parley, egui) está migrando a skrifa/harfrust | ❌ |
| **rustybuzz** | 0.20.1 (**2024-11-12**) | MIT | 🟠 Congelado desde 2024; el esfuerzo se movió a **harfrust** (mismo org) | Port fiel de HarfBuzz, muy usado (10 M desc./90 d por inercia) | En modo mantenimiento; harfrust es su sucesor | ❌ (usar harfrust) |
| **harfbuzz_rs** | 2.0.1 (**2021-08-28**) | MIT | 🔴 **Abandonado hace 5 años** | Enlaza el HarfBuzz C real (máxima fidelidad) | Sin mantenimiento; dependencia C en el AppImage; 9 K desc./90 d | ❌ |
| **fontdb** | 0.24.0 (2026-07-29) | MIT | ⭐ RazrFalcon | Base de datos de fuentes simple y sólida; 11,3 M desc./90 d | Menos capaz que fontique en fallback por script | 🟡 Llega igualmente como dependencia de `usvg` |
| `font-kit` | 0.14.3 (2025-05-26) | MIT OR Apache-2.0 | 🟠 Servo, ritmo lento | API de sistema en 3 plataformas | Superado por fontique | ❌ |

### 5.3 Recomendación firme

**parley 0.11 + harfrust + fontique + skrifa + peniko.** La razón decisiva es la coherencia de la pila: kurbo (geometría), peniko (pintura), parley (texto), vello (rasterizado) y fontique (fuentes) son **la misma familia**, comparten tipos y se versionan juntos. Evitamos conversiones y desajustes de tipos en las fronteras, que es donde se pierde precisión en un editor vectorial.

**Funciones específicas de Xarast y cómo se construyen:**

| Requisito | Solución |
|---|---|
| **Texto en path** | Propio: `parley` da los *glyph runs* con avances; `kurbo::ParamCurveArclen` da la parametrización por longitud de arco del path; colocamos cada glifo con su transformación (posición + tangente). ~300 líneas. Ningún crate lo da hecho |
| **Kerning manual** | Propio: parley entrega posiciones tras el kerning OpenType; guardamos un `HashMap<(glyph_idx, glyph_idx), f64>` de ajustes del usuario en el documento y los aplicamos tras el shaping |
| **Features OpenType** | `parley` permite `FontFeature`/`FontVariation` por rango de estilo; harfrust las aplica |
| **Bidi** | ICU4X dentro de parley (`unicode-bidi` no hace falta explícito) |
| **Texto → curvas** | `skrifa::outline::OutlinePen` → `kurbo::BezPath`. Directo |
| **Fuentes del sistema** | `fontique` (fontconfig / DirectWrite / CoreText) |
| **Fuentes incrustadas en el documento** | `skrifa` lee de un `&[u8]`; guardamos el fichero dentro del ZIP `.xarast` |

### 5.4 Riesgos

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| parley pre-1.0 rompe API | Alta | Bajo | Envolver en `xarast-text`; las roturas de Linebender son mecánicas |
| Fallback de fuentes CJK/árabe imperfecto en Linux | Media | Medio | Tests con documentos multiscript; posibilidad de forzar cadena de fallback en preferencias |
| Rendimiento de shaping en documentos con mucho texto | Media | Medio | Caché de layout por objeto de texto, invalidada por hash del contenido+estilo |

---

## 6. Imagen

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **image** | 0.25.10 (2026-03-10) | MIT OR Apache-2.0 | ⭐⭐ 50,9 M desc./90 d | Fachada universal: PNG, JPEG, GIF, WebP, TIFF, BMP, TGA, DDS, HDR, EXR, QOI, farbfeld, AVIF (con features); operaciones de resize con filtros de calidad; `DynamicImage` cómodo | Algunos decodificadores propios más lentos que los especializados; API de color limitada (sin gestión ICC) | ✅ **ELEGIDO (fachada)** |
| **zune-jpeg** | 0.5.15 (2026-09-08) | MIT OR Apache-2.0 OR Zlib | ⭐⭐ 41,5 M desc./90 d | **El decodificador JPEG más rápido en Rust** (SIMD); `image` ya puede delegar en él | — | ✅ **ELEGIDO** |
| **png** | 0.18.1 (2026-02-14) | MIT OR Apache-2.0 | ⭐⭐ image-rs; 72 M desc./90 d | Completo (APNG, 16-bit, ICC, chunks), rápido | — | ✅ **ELEGIDO** |
| **image-webp** | 0.2.4 (2025-08-27) | MIT OR Apache-2.0 | ⭐ image-rs; 26,9 M desc./90 d | **Puro Rust**, decodifica lossy+lossless+animado, codifica lossless | No codifica WebP con pérdida | ✅ **ELEGIDO** (decode + encode lossless) |
| `webp` (libwebp) | 0.3.1 (2025-08-29) | MIT OR Apache-2.0 (libwebp: BSD-3) | 🟡 | Codificación con pérdida de referencia | Dependencia C en el AppImage | 🟡 **Feature opcional** `webp-lossy` |
| **ravif** + **rav1e** | 0.13.0 / 0.8.1 | BSD-3 / BSD-2 | ⭐ 15 M desc./90 d | Codificación AVIF puro Rust de calidad; GPL-compatible | Codificar AVIF es lento (usar `rayon` + hilo aparte) | ✅ **ELEGIDO** (exportar AVIF) |
| **kamadak-exif** | 0.6.1 (2024-11-06) | BSD-2 | 🟡 Estable, sin cambios desde 2024 (pero es un formato estable) | Lee/escribe EXIF de JPEG, TIFF, PNG, WebP, HEIF; 3,17 M desc./90 d | API algo verbosa | ✅ **ELEGIDO** |
| **resvg** + **usvg** | 0.48.1 (2026-08-02) | Apache-2.0 OR MIT | ⭐ **Ahora mantenido por Linebender** (buena noticia: antes era bus factor 1 con RazrFalcon); 9,9 M desc./90 d | El mejor parser/normalizador SVG de Rust; `usvg` da un árbol ya resuelto (referencias, `use`, estilos CSS, unidades) que es justo lo que necesita un **importador**, no un rasterizador | Arrastra `tiny-skia` y `fontdb`; SVG 2 parcial | ✅ **ELEGIDO** — pero usar **`usvg` como parser y traducir su árbol a nuestro modelo**, no usar `resvg` para pintar |
| **zune-image** | 0.5.0 (2026-01-24) | MIT OR Apache-2.0 OR Zlib | 🟡 100 K desc. totales — poco adoptado | Muy rápido, arquitectura de pipeline de operaciones | Ecosistema pequeño; menos formatos que `image`; API menos estable | 🟡 Usar sólo sus decodificadores sueltos (zune-jpeg) |
| `jpeg-decoder` | 0.3.2 (2025-06-21) | MIT OR Apache-2.0 | ⭐ image-rs | Correcto, soporta JPEG progresivo | Más lento que zune-jpeg | 🟡 Fallback para casos raros |
| **oxipng** | 10.2.1 (2026-09-02) | MIT | ⭐ Activo | Optimización PNG sin pérdida al exportar | Lento (correr en hilo de fondo, opcional) | ✅ Opcional en "Exportar optimizado" |

**Decisión adicional — gestión de color:** ninguno de estos hace CMS. Para una herramienta profesional hay que añadir **`qcms`** (Mozilla, MPL-2.0, GPL-compatible) o `lcms2` (MIT, pero es C) para perfiles ICC. Se difiere a fase 2 pero **hay que reservar el hueco en el modelo de color desde el día 1** (no asumir sRGB en todas partes: guardar el espacio de color en cada bitmap y en el documento).

---

## 7. Compresión y contenedor `.xarast`

### 7.1 Diseño del contenedor

Recomendación: **`.xarast` = fichero ZIP** con esta estructura (modelo OPC/ORA/KRA, probado en Krita, Blender `.blend` no, pero sí en `.ora`, `.kra`, `.sla`, `.afdesign`):

```
documento.xarast              (ZIP, sin cifrar)
├── mimetype                  (STORED, primer entry, sin comprimir → "magic bytes" identificables por file(1)/MIME)
├── manifest.json             (DEFLATE — versión de formato, índice de partes, checksums)
├── document.bin              (ZSTD  — árbol de objetos serializado, formato binario propio)
├── thumbnail.png             (STORED — 256×256, para gestores de archivos)
├── preview.png               (DEFLATE — render completo, para "abrir recientes")
├── resources/
│   ├── bitmaps/<uuid>.png|.jpg|.avif   (STORED — ya están comprimidos)
│   ├── fonts/<hash>.ttf                (DEFLATE)
│   └── profiles/<hash>.icc             (DEFLATE)
└── history/                  (opcional, ZSTD — pila de undo persistente)
```

Ventajas de ZIP frente a un formato monolítico propio: se puede inspeccionar con `unzip`, permite **lectura parcial** (abrir la miniatura sin descomprimir el documento), permite **escritura incremental** al guardar (reescribir sólo las entradas cambiadas), y las herramientas de recuperación de datos existentes funcionan.

### 7.2 Comparativa

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **zip** | **8.6.0** (2026-08-11) | MIT | ⭐⭐ 71 M desc./90 d; `zip-rs/zip2` es el fork activo y mantenido | API síncrona simple; **soporta de serie deflate (con backend `zlib-rs`), deflate64, bzip2, zstd, lzma, xz, zopfli**; AES; escritura en streaming; ZIP64 | 9.0 en pre-release (fijar a 8.x); muchas features por defecto → desactivar las que no usemos | ✅ **ELEGIDO** con `default-features = false, features = ["deflate", "zstd"]` |
| `rc-zip` | 5.4.1 (2025-11-19) | Apache-2.0 OR MIT | 🟡 545 K desc./90 d | Diseño sans-io elegante, muy tolerante a ZIPs corruptos | Sólo lectura (escritura vía `rc-zip-tokio`/sync separado); ecosistema menor | 🟡 Plan B para el **importador tolerante** de ficheros dañados |
| `async_zip` | 0.0.19 (2026-08-22) | MIT | 🟡 Activo pero 0.0.x | Async | **No necesitamos async** para ficheros locales; versión 0.0.x | ❌ |
| **zstd** | **0.14.0** (2026-09-04) | BSD-3 (zstd C es BSD-3 **OR** GPL-2.0) | ⭐⭐ 82 M desc./90 d | Mejor ratio/velocidad del mercado; niveles 1–22; **diccionarios** (clave: entrenar un diccionario con documentos típicos mejora mucho ficheros pequeños); multihilo | Dependencia C (se compila con `cc`, sin problema en AppImage al ser estática) | ✅ **ELEGIDO** para `document.bin` |
| **flate2** | 1.1.10 (2026-08-28) | MIT OR Apache-2.0 | ⭐⭐ 158 M desc./90 d | Backends: `miniz_oxide` (por defecto, puro Rust), **`zlib-rs` (puro Rust, el más rápido)**, `zlib-ng` (C), `zlib` (C), `cloudflare_zlib` | — | ✅ **ELEGIDO** con backend **`zlib-rs`**: velocidad de zlib-ng sin dependencia C, mejor para AppImage y para cross-compilación a aarch64 |
| `brotli` | 9.0.0 (2026-09-02) | BSD-3 AND MIT | ⭐⭐ 60 M desc./90 d | Mejor ratio que deflate en texto | Más lento que zstd a igual ratio; no aporta sobre zstd aquí | ❌ (sí útil si algún día hay exportación web) |
| `libdeflater` | 1.26.1 (2026-09-17) | Apache-2.0 | ⭐ | El deflate más rápido para PNG | Apache-2.0 puro; el `png` crate ya va bien | ❌ |
| `lz4_flex` | 0.14.0 (2026-07-14) | MIT | ⭐⭐ 34 M desc./90 d | Compresión **ultrarrápida** (GB/s) | Ratio pobre | ✅ **Sí, pero para otra cosa**: comprimir los *tiles* de la caché de render y los snapshots de undo en RAM, donde importa la latencia, no el tamaño |

### 7.3 Decisión

- **Contenedor:** `zip 8.6` (`deflate`+`zstd`), backend deflate = `zlib-rs`.
- **Payload principal:** `zstd 0.14` nivel 3 al guardar (rápido), nivel 19 en "Guardar comprimido al máximo".
- **Caché en memoria y undo:** `lz4_flex`.
- **Legacy `.xar`:** parser propio de sólo lectura, alimentado por `cargo-fuzz` (§12).

---

## 8. Geometría

### 8.1 Comparativa de la base

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **kurbo** | **0.13.1** (2026-05-13) | Apache-2.0 OR MIT | ⭐⭐ Linebender; 17,6 M desc./90 d | **Lo verifiqué en docs.rs y cubre casi todo lo que necesitamos**: `stroke()`/`stroke_with()` (**stroke-to-path / expansión de trazo**), módulo `offset` (offsetting de cúbicas), módulo `simplify` + `fit_to_bezpath`/`fit_to_bezpath_opt`/`fit_to_cubic` (**simplificación y refitting**), `dash()` (**patrones de guiones**), `ParamCurveArclen` (longitud de arco → texto en path), `ParamCurveArea`/`ParamCurveMoments` (área y momentos), `ParamCurveNearest` (snapping), elipses, arcos. Doble f64. Autor: Raph Levien | Booleanas **no** incluidas; intersecciones curva-curva limitadas | ✅ **ELEGIDO (núcleo)** |
| **lyon** | 1.0.19 (2026-03-08) | MIT OR Apache-2.0 | ⭐ Activo (2 M desc./90 d) | Teselación fill/stroke a triángulos, hit-testing, `lyon_algorithms` (walk, raycast, aabb) | f32; teselación no la necesitamos si usamos Vello | 🟡 **Plan B de rasterizado** + herramientas puntuales |
| `euclid` | 0.22.14 (2026-03-18) | MIT OR Apache-2.0 | ⭐ Servo; 27,7 M desc./90 d | **Tipado de espacios** (`Point2D<f64, DocumentSpace>` vs `ScreenSpace`) — evita bugs de transformación, muy valioso en un editor con varios sistemas de coordenadas | Duplica tipos con kurbo | 🟡 Considerar sólo para los *newtypes* de espacio; alternativa: newtypes propios sobre `kurbo::Point` |
| `parry2d` | 0.31.1 (2026-09-18) | **Apache-2.0** | ⭐ Dimforge, muy activo | Colisión, BVH, distancia, convex hull | Pensado para física, no para edición; **Apache-2.0 puro**; su modelo de "shape" no casa con paths de Bézier | ❌ Descartado |
| `geo` | 0.33.1 (2026-04-20) | MIT OR Apache-2.0 | ⭐ GeoRust | Predicados robustos, simplificación (Douglas-Peucker, VW), booleanas de polígonos | Mundo GIS: sólo polilíneas, nada de Béziers; coordenadas geográficas | ❌ Descartado |
| `glam` | 0.33.7 (2026-09-07) | MIT OR Apache-2.0 | ⭐⭐ 49 M desc./90 d | SIMD, es lo que espera wgpu/bytemuck | f32 | ✅ **Sólo para uniforms/GPU**. El modelo del documento va en f64 con kurbo |
| `nalgebra` | 0.35.0 | Apache-2.0 | ⭐ | Álgebra general | Pesado, Apache-2.0 puro, innecesario | ❌ |

### 8.2 Operaciones booleanas de paths — el análisis que pedías

Este es el subsistema más delicado de un editor vectorial. Estado verificado a 19-09-2026:

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **i_overlay** | **9.0.0** (publicado **hoy, 2026-09-19**) | MIT OR Apache-2.0 | ⭐⭐ **El más activo con diferencia**: 4,04 M desc./90 d sobre 8,39 M totales → casi la mitad de sus descargas históricas son de los últimos 90 días. Releases constantes | Union/intersection/difference/xor + **self-intersections**; reglas even-odd y non-zero; **APIs de enteros y de flotantes**, con modo *fixed-scale/grid_size* que da resultados **estables y predecibles** (esto es oro para un editor: booleanas reproducibles); API con buffers reutilizables (sin allocs en bucle); rendimiento de primera; usado en GIS/CAD | **Sólo polígonos**: hay que aplanar las Béziers antes y re-ajustar curvas después | ✅ **ELEGIDO** |
| **i_curve** | **0.2.0** (**2026-09-19**) | MIT | 🔴 Novísimo: **168 descargas totales**, publicado hoy | Booleanas **nativas sobre cúbicas de Bézier y arcos elípticos racionales** — exactamente lo ideal; construido sobre `i_overlay 9`; API `CurveBuilder`/`FloatCurveOverlay`; 100 % documentado | Demasiado nuevo para producción; sin historial de bugs; API cambiará | 🔭 **Vigilar de cerca.** Es el candidato natural para sustituir al pipeline aplanar/reajustar en 2027. Evaluar en un *spike* de 3 días en fase 2 |
| `flo_curves` | 0.8.1 (2026-08-25) | **Apache-2.0** | 🟡 Activo pero pequeño (67 K desc./90 d) | Sí hace booleanas sobre curvas; buena colección de algoritmos (offset, fit, intersection) | **Apache-2.0 puro** (→ obliga GPLv3, aceptable pero suma); robustez inferior en casos degenerados; el proyecto Graphite lo descartó explícitamente a favor de kurbo por "algoritmos ingenuos y no optimizados" (dicho de bezier-rs, del mismo espacio) | 🟡 Plan C |
| `geo-booleanop` | 0.3.2 (**2020-06-27**) | MIT | 🔴 **Abandonado hace 6 años** | Implementa Martínez-Rueda | Muerto; además `geo` ya integró booleanas | ❌ |
| `path-bool` / `path_bool` | **no existe en crates.io** (404 en ambos nombres) | — | — | Es el módulo **interno** del editor Graphite, no publicado | No es una dependencia disponible | ❌ (pero su código GPL es referencia de estudio, y Graphite es Apache-2.0/consultar) |
| `bezier-rs` | 0.5.0 (2025-08-15) | MIT OR Apache-2.0 | 🟠 Graphite **migró a kurbo** y lo dejó atrás | API amable para segmentos de Bézier | El propio equipo dice que kurbo es superior en rendimiento y corrección | ❌ |
| `cavalier_contours` | 0.9.0 (2026-08-20) | MIT OR Apache-2.0 | 🟡 Activo, nicho CAD | **Offsetting de polilíneas con arcos muy robusto** (mejor que el offset naive) y booleanas de polilíneas | Polilíneas con bulge, no Béziers | 🟡 **Candidato real para el "Offset de contorno"** (herramienta de inset/outset), que es distinto del stroke-to-path |

### 8.3 Arquitectura de geometría recomendada

```
xarast-geom (crate propio)
├── tipos: Path (Vec<SubPath>), SubPath (Vec<Segment>), Segment = Line|Quad|Cubic|Arc
│          — todo f64, conversión bidireccional con kurbo::BezPath
├── stroke_to_path()   → kurbo::stroke()          [verificado disponible]
├── offset()           → kurbo::offset + cavalier_contours para casos con arcos
├── simplify()/fit()   → kurbo::simplify + fit_to_bezpath_opt
├── dash()             → kurbo::dash
├── arclen/nearest     → kurbo ParamCurve*
└── boolean()          → PIPELINE PROPIO:
      1. flatten adaptativo a polilíneas con tolerancia = f(zoom, precisión del doc)
         y tabla de trazabilidad segmento_original → rango de puntos
      2. i_overlay 9 con grid_size fijo (determinismo)
      3. re-ajuste: para cada tramo del resultado que provenga íntegramente de un
         segmento original, restaurar la curva original; para los tramos mezclados,
         kurbo::fit_to_bezpath_opt con tolerancia
      4. limpieza: fusión de puntos coincidentes, eliminación de micro-segmentos
```

**Por qué este pipeline y no una booleana de curvas directa:** la aritmética de intersección curva-curva exacta es donde fallan todos los editores (incluido Illustrator). Aplanar con tolerancia controlada + booleana entera determinista + refitting es lo que hacen en la práctica Inkscape (livarot), Blender y — según su blog — Graphite. Y con `i_overlay` en modo `grid_size` obtenemos **reproducibilidad bit a bit**, que es indispensable para los golden tests y para que "deshacer + rehacer" dé el mismo resultado.

**Punto de decisión futuro:** si `i_curve` madura (digamos ≥ 1.0 y ≥ 6 meses sin bugs graves), el paso 1–3 se sustituye por una llamada. Diseñar `boolean()` como una fachada para que ese cambio sea de un día.

### 8.4 Riesgos

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| Degradación de calidad de curvas tras booleanas repetidas | **Alta** | **Alto** | Tabla de trazabilidad (paso 3); tests de propiedad con `proptest`: `union(A, ∅) == A`, `A ∩ A == A`, área conservada dentro de ε |
| `i_overlay` rompe API en la 10.0 | Media | Bajo | Está tras `xarast-geom::boolean()` |
| Casos degenerados (self-intersections, tangencias, subpaths anidados) | Alta | Alto | Corpus de regresión con SVGs patológicos; `i_overlay` declara soportar self-intersections |
| Rendimiento en paths con 100 K nodos | Media | Medio | `i_overlay` tiene API con buffers reutilizables; paralelizar por subpath con rayon |

---

## 9. Serialización y undo/redo

### 9.1 Serialización

| Crate | Versión | Licencia | Mantenimiento | Pros | Contras | Veredicto |
|---|---|---|---|---|---|---|
| **serde** | 1.0.229 (2026-07-18) | MIT OR Apache-2.0 | ⭐⭐ El estándar | Universal, `serde_json` para manifiestos y preferencias legibles | Deserializar un documento grande = construir todo el árbol | ✅ **ELEGIDO** (manifiestos, preferencias, clipboard, tests con insta) |
| **rkyv** | 0.8.18 (2026-08-05) | MIT | ⭐⭐ 38 M desc./90 d | **Zero-copy**: `mmap` del fichero y acceso directo sin deserializar → autosave y "abrir documento enorme" instantáneos; validación con `bytecheck` | Formato rígido (evolucionar el esquema requiere cuidado); menos ergonómico | ✅ **ELEGIDO** para **autosave/scratch/undo persistente**, NO para el formato público |
| `bincode` | 3.0.0 (2025-12-16) | MIT | ⭐⭐ | Simple y compacto | Sin evolución de esquema; sin zero-copy | 🟡 |
| `postcard` | 1.1.3 (2025-07-24) | MIT OR Apache-2.0 | ⭐ | Muy compacto (varint), diseñado para embebidos | Mismos límites de evolución | 🟡 |

**Decisión sobre el formato `.xarast` (`document.bin`):** **ni serde ni rkyv directamente**, sino un **formato de registros versionado escrito a mano** (cabecera mágica + versión + tabla de registros TLV), con serde para los *valores* dentro de cada registro donde convenga. Razón: un formato de documento debe sobrevivir 20 años y soportar *forward compatibility* (una versión vieja abre un fichero nuevo ignorando registros desconocidos). Ni serde ni rkyv dan eso gratis, y es exactamente el diseño que usa el `.xar` original (registros con ID y longitud). Esto además hace natural el **fuzzing** (§12).

### 9.2 Undo/redo

| Estrategia | Pros | Contras | Veredicto |
|---|---|---|---|
| **Command pattern (op + inversa)** | Memoria mínima; permite coalescing (arrastrar un nodo = 1 entrada); nombres legibles en el panel de historial; base natural para scripting y macros | Cada operación necesita su inversa escrita y **probada**; los bugs de inversa corrompen el documento silenciosamente | ✅ **Base elegida** |
| **Snapshots completos** | Trivialmente correcto | Inviable con bitmaps grandes | ❌ |
| **Snapshots persistentes (structural sharing)** | Correcto *y* barato: clonar el árbol es O(1) amortizado; undo = cambiar un puntero; permite ramas de historial y comparación de versiones | Overhead de indirección en acceso; los payloads pesados (bitmaps) hay que sacarlos fuera | ✅ **Complemento elegido** |
| **CRDT / operational transform** | Colaboración en tiempo real | Complejidad enorme, no es requisito | ❌ (no cerrar la puerta: el command pattern es el camino hacia ello) |

**Recomendación: híbrido en tres capas.**

1. **Documento** = árbol de nodos en estructura persistente (`imbl::Vector` / `imbl::HashMap`) con los *payloads pesados* (bitmaps, fuentes) detrás de `Arc<Resource>` (copy-on-write compartido).
2. **Historial** = pila de `Command` con `apply`/`invert`, **más** el puntero al snapshot persistente anterior. Deshacer normalmente restaura el puntero (O(1)); el `Command` sirve para la etiqueta legible, el coalescing y el replay.
3. **Persistencia del historial** = serializar la pila de comandos con `rkyv` en `history/` dentro del `.xarast` (opcional en preferencias) → "deshacer después de cerrar y reabrir", que es un diferenciador real.

| Crate | Versión | Licencia | Notas |
|---|---|---|---|
| **imbl** | 7.0.2 (2026-09-09) | **MPL-2.0+** (GPL-compatible ✅) | Fork mantenido de `im` (que lleva parado desde 2022). 2,58 M desc./90 d. Mantenido por jneem (Linebender). ✅ **ELEGIDO** |
| `im` | 15.1.0 (**2022-04-29**) | MPL-2.0+ | 🔴 Abandonado. Usar `imbl` |
| `rpds` | 1.2.1 (2026-05-15) | MIT | Alternativa mantenida y con licencia más simple; estructuras más "puras" (listas, tries) pero menos optimizadas para vectores grandes. 🟡 Plan B si MPL-2.0 incomoda |
| `slotmap` / `thunderdome` | — | Zlib/MIT | Arena de nodos con IDs estables (generacionales) — **imprescindible** para referencias entre objetos del documento |

---

## 10. Concurrencia

### 10.1 ¿Hace falta tokio?

**No para el núcleo.** Xarast no es un servidor: no hay miles de conexiones ni E/S concurrente masiva. Lo que hay es **paralelismo de datos** (rasterizar tiles, teselar, decodificar imágenes, booleanas por subpath) y **unos pocos hilos de larga vida**. Meter tokio arrastraría un runtime, colorearía funciones con `async` y complicaría el bucle de eventos, que debe ser síncrono y determinista.

**Sí hace falta un runtime async mínimo** en dos sitios: `ashpd` (portales XDG, que es D-Bus y async por naturaleza) y la comprobación de actualizaciones. Solución: `tokio` con `features = ["rt", "macros", "time"]` (runtime de 1 hilo, **current_thread**) confinado a un hilo "de servicios", o directamente `pollster::block_on` en un hilo de trabajo. Recomiendo lo primero por robustez de `ashpd`.

### 10.2 Modelo de hilos

```
┌─ Hilo principal (UI) ────────────────────────────────────────┐
│  winit event loop → egui → construye ScenePatch              │
│  NUNCA bloquea. Sin I/O de disco. Sin rasterizado pesado.    │
└──────────────┬───────────────────────────────────────────────┘
               │ crossbeam-channel (ScenePatch, prioridad)
┌──────────────▼── Hilo de render ─────────────────────────────┐
│  posee wgpu::Device/Queue; compila la escena; tiles;         │
│  submit + present. Usa rayon para tiles CPU.                 │
└──────────────┬───────────────────────────────────────────────┘
               │
┌──────────────▼── rayon global pool (N-2 hilos) ──────────────┐
│  teselación, booleanas, decodificación de imagen, filtros,   │
│  generación de miniaturas                                    │
└──────────────────────────────────────────────────────────────┘
┌─ Hilo de servicios (tokio current_thread) ───────────────────┐
│  portales XDG (ashpd), autosave, watcher de fuentes, updates │
└──────────────────────────────────────────────────────────────┘
```

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| **rayon** | 1.12.0 (2026-04-14) | MIT OR Apache-2.0 | Paralelismo de datos. ⭐⭐ 125 M desc./90 d |
| **crossbeam-channel** | (crossbeam 0.8.5) | MIT OR Apache-2.0 | Canales entre hilos con `select!` |
| **parking_lot** | 0.12.5 | MIT OR Apache-2.0 | Mutex/RwLock más rápidos y sin envenenamiento |
| **tokio** | 1.53.1 | MIT | Sólo `rt` + `macros` + `time`, hilo de servicios |
| **tracing** + **puffin** / **tracy-client** | 0.1.44 / 0.20.0 / 0.19.0 | MIT / MIT OR Apache-2.0 | Instrumentación; `puffin` para el profiler in-app (panel de egui), `tracy` para análisis profundo |
| **wgpu-profiler** | 0.28.0 | MIT OR Apache-2.0 | Timestamps de GPU por pase |

**Regla de oro a codificar en CI:** el hilo de UI no debe superar 8 ms por frame. Añadir un test de rendimiento con `criterion` sobre la función `build_ui_frame()` con un documento sintético de 10 000 objetos.

---

## 11. Empaquetado Linux

### 11.1 AppImage — herramientas verificadas (2026)

| Herramienta | Estado 2026 | Licencia | Pros | Contras | Veredicto |
|---|---|---|---|---|---|
| **appimagetool** (AppImageKit) | Activo. **Cambio clave: ahora usa el `runtime` estático por defecto**, lo que resuelve el fallo histórico de arranque en distros que sólo traen **libfuse3** (Ubuntu ≥ 24.04, Arch, Fedora reciente) | MIT | Es la referencia; control total del AppDir | Requiere que construyas el AppDir tú | ✅ **ELEGIDO** (paso final) |
| **linuxdeploy** | Activo; **también cambió al runtime estático por defecto** | MIT | Automatiza copia de librerías (`ldd`), desktop entry, iconos, `AppRun`; plugins (`appimage`, `gtk`, `qt`); soporta `UPDATE_INFORMATION` para **zsync** | **No cross-compila a ARM**: los AppImage aarch64 hay que construirlos en un runner ARM | ✅ **ELEGIDO** (paso de recolección) |
| `cargo-appimage` | 2.4.0 (2025-11-24) | **GPL-3.0** (es una herramienta, no se enlaza → no contamina) | Integración `cargo` cómoda | Muy delgado (envuelve linuxdeploy); 1,5 K desc./90 d; poco control | ❌ Preferimos un script explícito |
| `cargo-packager` | 0.11.8 (2025-11-27) | MIT OR Apache-2.0 | Multi-formato (AppImage, deb, MSI, NSIS, .app, dmg) desde una config | Menos control fino en AppImage; útil más adelante para Windows/macOS | 🟡 **Sí para las fases 2–3** (Windows/macOS) |
| `cargo-dist` | 0.32.0 (2026-05-22) | MIT OR Apache-2.0 | Genera los workflows de CI y los instaladores/releases | El proyecto axo cerró; el futuro es incierto | 🟡 Sólo como inspiración para el workflow |

### 11.2 Compatibilidad glibc — la decisión de la línea base

Los binarios enlazan contra símbolos versionados de glibc y **sólo funcionan en glibc igual o más nueva**. Por tanto la línea base la fija la imagen de compilación:

| Base de compilación | glibc | Cubre | Recomendación |
|---|---|---|---|
| Ubuntu 20.04 | 2.31 | Prácticamente todo lo vivo, incluidas RHEL 8 derivadas | 🟡 Ya sin soporte (EOL abril 2025); toolchain viejo |
| **Ubuntu 22.04** | **2.35** | Debian 12, Ubuntu 22.04+, Fedora 36+, RHEL 9, openSUSE 15.5+, SteamOS 3.5+ | ✅ **ELEGIDO.** Es el punto dulce en 2026 y hay runner ARM oficial (`ubuntu-22.04-arm`) |
| Ubuntu 24.04 | 2.39 | Sólo distros de 2024+ | ❌ Demasiado nuevo |
| `manylinux_2_28` (AlmaLinux 8) | 2.28 | Máxima compatibilidad | 🟡 Plan B si aparecen quejas; toolchain más incómodo para Rust+Wayland dev headers |

Refuerzos:
- Compilar en **contenedor Docker** `ubuntu:22.04` (no en el runner directamente) para que la línea base sea reproducible aunque GitHub cambie sus imágenes.
- `cargo build --release` con `target-cpu=x86-64-v2` (SSE4.2/POPCNT, seguro desde 2009) y detección en runtime para AVX2 en los kernels de imagen.
- **NO** enlazar musl: rompe `dlopen` de los drivers GL/Vulkan del sistema.

### 11.3 Qué NO meter en el AppImage

Regla: **todo lo que hable con el hardware o el compositor del usuario debe venir del sistema.** Lista de exclusión para `linuxdeploy` (`--exclude-library`):

```
libGL.so*, libGLX*, libEGL*, libgbm*, libdrm*, libvulkan.so*,
libwayland-client.so*, libwayland-egl.so*, libwayland-cursor.so*,
libX11*, libxcb*, libxkbcommon*,
libc.so*, libstdc++.so*, libgcc_s.so*, libm.so*, libpthread.so*, libdl.so*,
libfontconfig.so*, libfreetype.so*   (fontconfig debe ser el del sistema para ver las fuentes del usuario)
```

Sí se empaqueta: nuestro binario, `libzstd` estático (viene en el binario), los assets, iconos, `.desktop`, metainfo AppStream y `THIRD-PARTY-LICENSES.html`.

> Nota: al usar **fontique**, que usa fontconfig del sistema en Linux, es **crítico** no empaquetar fontconfig: si lo hacemos, el usuario no verá sus fuentes.

### 11.4 Contenido del AppDir

```
Xarast.AppDir/
├── AppRun                       (script: exporta XDG_DATA_DIRS y lanza el binario)
├── xarast.desktop               (raíz, obligatorio)
├── xarast.png                   (256×256, raíz, obligatorio)
├── .DirIcon -> xarast.png
└── usr/
    ├── bin/xarast
    ├── lib/                     (deps no excluidas)
    └── share/
        ├── applications/xarast.desktop
        ├── icons/hicolor/{16,22,24,32,48,64,128,256,512}x.../apps/xarast.png
        ├── icons/hicolor/scalable/apps/xarast.svg
        ├── mime/packages/xarast.xml          ← registro de .xarast y .xar
        ├── metainfo/es.digio.Xarast.metainfo.xml   ← AppStream (necesario para Flathub y para GNOME Software)
        └── doc/xarast/THIRD-PARTY-LICENSES.html
```

`xarast.desktop` mínimo:
```ini
[Desktop Entry]
Type=Application
Name=Xarast
GenericName=Vector Graphics Editor
Comment=Editor de gráficos vectoriales y fotografía
Exec=xarast %F
Icon=xarast
Categories=Graphics;VectorGraphics;RasterGraphics;2DGraphics;
MimeType=application/x-xarast;application/x-xara;image/svg+xml;image/png;image/jpeg;
StartupNotify=true
StartupWMClass=xarast
```

> Importante para Wayland: el `app_id` que pase winit (`WindowAttributes::with_name(app_id, _)` en X11 / `with_application_id` en Wayland) **debe coincidir** con el nombre del `.desktop` (`xarast`), o el icono no aparecerá en la barra de tareas de GNOME/KDE.

Tipo MIME (`xarast.xml`):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-xarast">
    <comment>Documento de Xarast</comment>
    <comment xml:lang="en">Xarast document</comment>
    <glob pattern="*.xarast"/>
    <magic priority="60">
      <match type="string" value="PK\003\004" offset="0">
        <match type="string" value="application/x-xarast" offset="38"/>
      </match>
    </magic>
    <icon name="application-x-xarast"/>
  </mime-type>
  <mime-type type="application/x-xara">
    <comment>Dibujo de Xara</comment>
    <glob pattern="*.xar"/>
    <magic priority="50"><match type="string" value="XARA" offset="0"/></magic>
  </mime-type>
</mime-info>
```

### 11.5 Actualizaciones delta con zsync

`linuxdeploy --output appimage` con `UPDATE_INFORMATION` embebe la cadena de actualización en el propio AppImage:

```bash
export UPDATE_INFORMATION="gh-releases-zsync|digio-es|Xarast|latest|Xarast-*-x86_64.AppImage.zsync"
```

Esto genera un `.zsync` junto al `.AppImage` que hay que subir a la misma release. `AppImageUpdate` (o `appimageupdatetool`) descarga **sólo los bloques cambiados** — típicamente 3–8 MB en vez de 60 MB. También podemos implementar el chequeo dentro de la app (leer la sección ELF `.upd_info`) y ofrecer "Buscar actualizaciones".

### 11.6 AppImage vs Flatpak vs Snap

| Criterio | AppImage | Flatpak | Snap |
|---|---|---|---|
| Instalación | Ninguna: `chmod +x` y ejecutar | `flatpak install` | `snap install` |
| Sandbox | **Ninguno** por defecto | **Bubblewrap + portales**, el modelo más maduro | AppArmor + interfaces |
| Acceso a ficheros del usuario | Directo (bueno para un editor) | Vía portales (el diálogo de `rfd` ya lo soporta) | Vía interfaces |
| Tamaño / disco | 1 fichero, ~60–90 MB, sin compartir | Runtime compartido (`org.freedesktop.Platform 24.08`) → +1 GB la primera app, luego barato | Mayor, un loop mount por snap |
| Actualizaciones delta | zsync (manual o AppImageUpdate) | OSTree, excelente | Delta, excelente |
| Descubrimiento | Ninguno (hay que ir a la web) | **Flathub**: 3200+ apps, 433 M descargas 2025; integrado en GNOME Software y KDE Discover | Snap Store (Ubuntu) |
| Tableta/stylus, GPU | Sin problemas (usa todo del sistema) | Funciona, pero requiere permisos `--device=all` y hay fricciones históricas con Wacom | Fricciones conocidas con hardware |
| Aceptación en la comunidad Linux | Buena entre "power users" y creativos | **Es el estándar de facto en 2026** | Polarizado (rechazo fuera de Ubuntu) |
| Esfuerzo de mantenimiento | Bajo | Medio (manifiesto YAML + revisión Flathub) | Medio-alto |

**Recomendación:** **AppImage como canal primario** (correcto para la fase temprana: sin sandbox, cero fricción con tabletas y GPUs, un fichero que el usuario prueba y tira). **Flatpak/Flathub como segundo canal en cuanto haya una beta pública** — es donde está hoy el descubrimiento de software en Linux, y su modelo de portales ya lo cubrimos con `ashpd`+`rfd`. **Snap: no**, no aporta nada sobre los otros dos y el coste de mantener un tercer canal no compensa.

### 11.7 CI — GitHub Actions

```yaml
# .github/workflows/appimage.yml
name: AppImage
on:
  push: { tags: ['v*'] }
  workflow_dispatch:

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { runner: ubuntu-22.04,     arch: x86_64,  target: x86_64-unknown-linux-gnu  }
          - { runner: ubuntu-22.04-arm, arch: aarch64, target: aarch64-unknown-linux-gnu }
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      # Contenedor 22.04 explícito para fijar la línea base de glibc (2.35)
      # aunque GitHub actualice la imagen del runner.
      - name: Dependencias de build
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            build-essential pkg-config cmake \
            libwayland-dev libxkbcommon-dev libx11-dev libxcursor-dev \
            libxrandr-dev libxi-dev libgl1-mesa-dev libvulkan-dev \
            libfontconfig-1-dev libdbus-1-dev libudev-dev \
            desktop-file-utils appstream file wget

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { key: ${{ matrix.arch }} }

      - name: Build
        env:
          RUSTFLAGS: "-C target-cpu=x86-64-v2"   # sólo x86_64; en ARM usar el default
        run: cargo build --release --locked --target ${{ matrix.target }}

      - name: Preparar AppDir
        run: |
          install -Dm755 target/${{ matrix.target }}/release/xarast     AppDir/usr/bin/xarast
          install -Dm644 packaging/linux/xarast.desktop                 AppDir/usr/share/applications/xarast.desktop
          install -Dm644 packaging/linux/xarast.xml                     AppDir/usr/share/mime/packages/xarast.xml
          install -Dm644 packaging/linux/es.digio.Xarast.metainfo.xml   AppDir/usr/share/metainfo/es.digio.Xarast.metainfo.xml
          for s in 16 22 24 32 48 64 128 256 512; do
            install -Dm644 assets/icons/${s}.png \
              AppDir/usr/share/icons/hicolor/${s}x${s}/apps/xarast.png
          done
          install -Dm644 assets/icons/xarast.svg \
            AppDir/usr/share/icons/hicolor/scalable/apps/xarast.svg
          cargo install --locked cargo-about
          cargo about generate packaging/about.hbs \
            > AppDir/usr/share/doc/xarast/THIRD-PARTY-LICENSES.html

      - name: Validar metadatos
        run: |
          desktop-file-validate AppDir/usr/share/applications/xarast.desktop
          appstreamcli validate --no-net AppDir/usr/share/metainfo/es.digio.Xarast.metainfo.xml

      - name: Herramientas AppImage
        run: |
          A=${{ matrix.arch }}
          wget -q https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$A.AppImage
          wget -q https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-$A.AppImage
          chmod +x linuxdeploy-*.AppImage

      - name: Construir AppImage
        env:
          UPDATE_INFORMATION: >-
            gh-releases-zsync|digio-es|Xarast|latest|Xarast-*-${{ matrix.arch }}.AppImage.zsync
          OUTPUT: Xarast-${{ github.ref_name }}-${{ matrix.arch }}.AppImage
          # runtime estático -> arranca con libfuse2 Y libfuse3
          LDAI_RUNTIME_FILE: ""
        run: |
          # --appimage-extract-and-run evita necesitar FUSE dentro del runner
          ./linuxdeploy-${{ matrix.arch }}.AppImage --appimage-extract-and-run \
            --appdir AppDir \
            --desktop-file AppDir/usr/share/applications/xarast.desktop \
            --icon-file AppDir/usr/share/icons/hicolor/256x256/apps/xarast.png \
            --exclude-library "libGL*" --exclude-library "libEGL*" \
            --exclude-library "libvulkan*" --exclude-library "libwayland-*" \
            --exclude-library "libX11*" --exclude-library "libxcb*" \
            --exclude-library "libxkbcommon*" --exclude-library "libdrm*" \
            --exclude-library "libgbm*" --exclude-library "libfontconfig*" \
            --exclude-library "libfreetype*" \
            --output appimage

      - name: Humo (arranque headless)
        run: |
          ./Xarast-*-${{ matrix.arch }}.AppImage --appimage-extract-and-run --version

      - uses: actions/upload-artifact@v4
        with:
          name: appimage-${{ matrix.arch }}
          path: |
            Xarast-*.AppImage
            Xarast-*.AppImage.zsync

  release:
    needs: build
    runs-on: ubuntu-latest
    permissions: { contents: write }
    steps:
      - uses: actions/download-artifact@v4
        with: { path: dist, merge-multiple: true }
      - run: |
          cd dist && sha256sum Xarast-* > SHA256SUMS
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            dist/Xarast-*.AppImage
            dist/Xarast-*.AppImage.zsync
            dist/SHA256SUMS
```

**Notas críticas del workflow:**
- `ubuntu-22.04-arm` es un runner **ARM nativo y gratuito para repos públicos** (~10 min por build). Esto resuelve la limitación de que `linuxdeploy` no cross-compila a ARM.
- `--appimage-extract-and-run` es obligatorio: los runners de GitHub no tienen FUSE.
- El **runtime estático** (por defecto desde 2026) es lo que garantiza que el AppImage arranque tanto en Ubuntu 22.04 (libfuse2) como en Arch/Ubuntu 24.04+ (sólo libfuse3). **Verificarlo en el smoke test.**
- Añadir un job separado que ejecute el AppImage en contenedores `debian:12`, `fedora:41` y `archlinux:latest` con `xvfb` para detectar dependencias olvidadas.

---

## 12. Empaquetado Windows y macOS (fases 2 y 3)

### 12.1 Windows

| Aspecto | Recomendación | Notas |
|---|---|---|
| Formato | **MSI** (empresas, despliegue por GPO) **+ NSIS/EXE** (usuarios) | `cargo-wix 0.3.9` (MIT/Apache) genera MSI con WiX; `cargo-packager 0.11.8` hace MSI y NSIS desde una sola config → **preferir cargo-packager** para no mantener dos toolchains |
| Toolchain | `x86_64-pc-windows-msvc` (**no** GNU) | Mejor compatibilidad con Windows Ink y depuración |
| ARM | `aarch64-pc-windows-msvc` en fase 3 | Surface Pro X y sucesores |
| Firma | **Azure Trusted Signing** (~10 €/mes, no requiere HSM físico) | Alternativa: certificado EV en token (~400 €/año). **Sin firma, SmartScreen bloquea las descargas**; con firma OV normal hace falta acumular reputación |
| Runtime | VCRedist estático: `RUSTFLAGS=-C target-feature=+crt-static` | Evita el instalador de VCRedist |
| Asociación de ficheros | `.xarast`, `.xar` en el registro; `ProgID` + icono | El MSI lo hace declarativamente |
| Tableta | Windows Ink vía winit 0.31 (ya verificado como soportado); **plan B `wintab_lite`** si hay quejas con Wacom/Huion externas | Es un problema conocido y real (ver la documentación de Photoshop sobre Ink vs Wintab) |
| Actualizaciones | Comprobación propia + descarga del instalador | Winget manifest como extra |

### 12.2 macOS

| Aspecto | Recomendación | Notas |
|---|---|---|
| Formato | **`.app` dentro de `.dmg`** | `cargo-packager` o `cargo-bundle 0.11` |
| Universal binary | `cargo build --target x86_64-apple-darwin` + `aarch64-apple-darwin`, luego **`lipo -create`** | O `cargo-packager` con `--target universal-apple-darwin` |
| Deployment target | `MACOSX_DEPLOYMENT_TARGET=11.0` (Big Sur) | Cubre desde 2020 |
| Firma | **Apple Developer Program, 99 $/año**, obligatorio | `codesign --deep --force --options runtime --timestamp` con *Developer ID Application* |
| Notarización | `xcrun notarytool submit --wait` + `xcrun stapler staple` | Sin esto, Gatekeeper lo bloquea. En CI: guardar el `.p12` y la app-specific password como secrets |
| Entitlements | `com.apple.security.cs.disable-library-validation` sólo si hace falta | Hardened runtime obligatorio para notarizar |
| Tableta | **winit NO soporta tableta en macOS.** Hay que implementarlo: `objc2-app-kit`, `NSEventTypeTabletPoint` / `NSEventSubtypeTabletPoint`, con `pressure`, `tilt`, `rotation`, `tangentialPressure` | ~200 líneas. Es trabajo obligado de la fase macOS |
| Backend GPU | Metal vía wgpu (sin MoltenVK) | |
| Notas Wayland-equivalentes | Trackpad gestures ya los da winit | |

---

## 13. Testing

### 13.1 Tests de render (golden images) — lo más importante

El problema: los renders GPU **no son bit-a-bit reproducibles** entre drivers. Solución en dos niveles:

| Nivel | Qué | Herramienta | Tolerancia |
|---|---|---|---|
| **A. Golden CPU (obligatorio en CI)** | Renderizar el corpus con `vello_cpu` (determinista) y comparar píxel a píxel | comparación exacta + `image-compare` para el informe de fallo | **0** diferencias |
| **B. Paridad GPU↔CPU (nightly, en runner con lavapipe)** | Mismo corpus con backend GPU vs CPU | `image-compare` (RMS / SSIM) o `dify` | ΔRMS < 0,5 %, sin píxel con Δ > 8/255 |
| **C. Regresión visual en PR** | Sólo los tests tocados; subir las imágenes diff como artifact | script propio + `actions/upload-artifact` | — |

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| **image-compare** | 0.5.0 (2025-08-18) | MIT | RMS, SSIM, hybrid comparison; devuelve mapa de diferencias → perfecto para el artifact de fallo |
| `dify` | 0.8.0 (2026-01-04) | MIT | Port de pixelmatch, rápido, buen anti-aliasing awareness | Alternativa a image-compare |
| **insta** | 1.48.0 (2026-06-11) | **Apache-2.0** | Snapshots de **texto**: árbol del documento serializado, salida del parser `.xar`, plan de render, geometría tras booleanas (como SVG path data). `cargo insta review` es excelente. ⚠️ Apache-2.0 puro → refuerza GPLv3 |
| **egui_kittest** | 0.36.2 | MIT OR Apache-2.0 | Tests de la UI de egui **con snapshots de imagen incluidos**; simula clics y teclado. Oficial de egui |

**Corpus de golden tests recomendado** (`tests/corpus/`): documentos `.xarast` que cubran cada primitiva y cada combinación problemática — degradados (lineal, radial, cónico, malla), transparencias por objeto, blend modes, feathering, clipping paths anidados, texto en path con OpenType, texto bidi, bitmaps con distintos espacios de color, trazos con extremos/uniones de todos los tipos, líneas discontinuas, booleanas sobre self-intersections. Objetivo: **≥ 120 casos antes de la beta**.

### 13.2 Benchmarks

| Crate | Versión | Licencia | Veredicto |
|---|---|---|---|
| **criterion** | 0.8.2 (2026-02-04) | Apache-2.0 OR MIT | ✅ Estándar; detección estadística de regresiones; gráficas HTML |
| `divan` | 0.1.21 (2025-04-10) | MIT OR Apache-2.0 | 🟡 API más agradable, mucho más rápido, pero sin releases desde abril 2025 |

Benchmarks obligatorios: booleanas sobre paths de 1 K/10 K/100 K nodos, shaping de 10 000 caracteres, decodificación JPEG 24 MP, render de un documento de 10 000 objetos, `build_ui_frame()`, serializar/deserializar un `.xarast` de 200 MB.

### 13.3 Fuzzing del parser `.xar`

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| **cargo-fuzz** | 0.13.2 (2026-06-09) | MIT OR Apache-2.0 | libFuzzer sobre el parser. **Imprescindible**: `.xar` es un formato binario de terceros; un parser en Rust no tiene UB pero sí `panic!`, OOM (longitud de registro maliciosa) y bucles infinitos, y todos son DoS |
| **arbitrary** | 1.4.2 (2025-08-14) | MIT OR Apache-2.0 | Generar documentos válidos estructurados para fuzzing de round-trip |
| **proptest** | 1.11.0 (2026-03-24) | MIT OR Apache-2.0 | **Invariantes de geometría**: `union(A,∅)==A`, `A∩A==A`, `difference(A,A)==∅`, área conservada, `stroke_to_path` produce paths cerrados, round-trip `serialize→deserialize==identidad` |

Targets de fuzz mínimos: `fuzz_xar_parse`, `fuzz_xarast_parse`, `fuzz_svg_import` (usvg), `fuzz_path_boolean`, `fuzz_text_shape`. Correr en CI **nightly** 30 min por target; el corpus se cachea entre ejecuciones.

Defensas que el fuzzing debe verificar: límite duro de memoria por documento, límite de profundidad de anidamiento de grupos, validación de longitudes antes de reservar, timeout por operación.

### 13.4 Resto

| Herramienta | Versión | Uso |
|---|---|---|
| **cargo-nextest** | 0.9.145 (2026-09-16) | Runner de tests paralelo, 2–3× más rápido, mejor salida, reintentos |
| `cargo-deny` | 0.20.2 | **Licencias** (§1.4) + avisos de seguridad (RustSec) + duplicados |
| `cargo-about` | 0.9.2 | Generar el fichero de licencias de terceros |
| `trybuild` | 1.0.121 | Sólo si escribimos macros derive propias |

---

## DECISIONES

### Licencia

> **Obsoleto — ver la nota de actualización del §1.** El bloque siguiente refleja la
> decisión anterior, tomada bajo el supuesto de trabajo derivado. La decisión vigente
> es **MIT OR Apache-2.0** para todo el proyecto, sostenida por la disciplina de sala
> limpia.

```
Xarast se publica bajo GPL-3.0-or-later.
Los crates reutilizables (xarast-geom, xarast-xar, xarast-raster) se publican
bajo MIT OR Apache-2.0 para maximizar su uso por terceros.
Xarast es una reimplementación de SALA LIMPIA: no se copia ni se traduce
código de XaraLX (GPL-2.0-only, incompatible con winit/Apache-2.0).
ACCIÓN INMEDIATA: sustituir /home/user/Xarast/LICENSE (hoy MIT) por GPL-3.0.
```

### Estructura del workspace

```
Xarast/
├── Cargo.toml                 # workspace
├── crates/
│   ├── xarast-app/            # binario: arranque, CLI, preferencias
│   ├── xarast-shell/          # winit 0.31 + wgpu 30 + puente a egui + accesskit
│   ├── xarast-ui/             # paneles, galerías, diálogos, herramientas (egui)
│   ├── xarast-doc/            # modelo del documento, undo, imbl, comandos
│   ├── xarast-geom/           # kurbo + booleanas (i_overlay) + stroke/offset/simplify
│   ├── xarast-raster/         # trait Rasterizer + backends vello / vello_cpu + compositor wgpu
│   ├── xarast-text/           # parley + skrifa: layout, texto en path, kerning manual
│   ├── xarast-image/          # decodificación/codificación, gestión de color
│   ├── xarast-format/         # .xarast (ZIP+zstd) lectura/escritura
│   ├── xarast-xar/            # parser .xar legacy (sólo lectura) + fuzz targets
│   └── xarast-input/          # trait TabletSource + winit/octotablet/appkit
├── fuzz/                      # cargo-fuzz
├── packaging/{linux,windows,macos}/
└── tests/corpus/              # golden tests
```

### `Cargo.toml` raíz — listo para copiar

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version       = "0.1.0"
edition       = "2024"
rust-version  = "1.90"
license       = "GPL-3.0-or-later"
repository    = "https://github.com/digio-es/Xarast"
authors       = ["Jose Francisco Rives <jose@digio.es>"]

[workspace.dependencies]

# ── Ventana, entrada, GPU ──────────────────────────────────────────────────
# winit 0.31 es OBLIGATORIO: es la única versión con TabletToolData
# (presión, tilt, twist). Fijado a la beta exacta a propósito.
winit            = { version = "=0.31.0-beta.3", default-features = false, features = ["wayland", "wayland-dlopen", "x11", "rwh_06"] }
wgpu             = { version = "30.0",  default-features = false, features = ["wgsl", "vulkan", "metal", "dx12", "gles", "fragile-send-sync-non-atomic-wasm"] }
raw-window-handle = "0.6"
bytemuck         = { version = "1.25", features = ["derive"] }
glam             = { version = "0.33", features = ["bytemuck"] }
pollster         = "1.0"

# Tableta: complemento a winit para X11 y para botones/anillos de pad.
# ATENCIÓN: crates.io congelado en 0.1.0 (2024). VENDORIZAR en third_party/.
octotablet       = { version = "0.1", optional = true }

# ── UI ─────────────────────────────────────────────────────────────────────
# NO se usa eframe: fija winit 0.30 y perderíamos el stylus.
egui             = { version = "0.36", features = ["accesskit", "serde", "rayon"] }
egui-wgpu        = { version = "0.36", features = ["winit"] }
egui_extras      = { version = "0.36", features = ["image", "svg", "serde"] }
egui_tiles       = "0.17"
accesskit        = "0.25"
accesskit_winit  = "0.34"
arboard          = { version = "3.6", features = ["wayland-data-control", "image-data"] }
rfd              = { version = "0.17", default-features = false, features = ["xdg-portal", "tokio"] }
ashpd            = { version = "0.13", default-features = false, features = ["tokio"] }

# ── Rasterizado 2D ─────────────────────────────────────────────────────────
peniko           = "0.6"                  # tipos de pintura compartidos
vello            = { version = "0.10", optional = true }   # backend GPU compute
vello_hybrid     = { version = "0.2",  optional = true }   # backend GPU sin compute
vello_cpu        = "0.2"                  # backend de referencia + fallback (SIEMPRE)
lyon             = { version = "1.0", optional = true }    # plan B / teselaciones puntuales

# ── Geometría ──────────────────────────────────────────────────────────────
kurbo            = { version = "0.13", features = ["serde"] }
i_overlay        = "9.0"                  # booleanas de polígonos (determinista con grid_size)
cavalier_contours = { version = "0.9", optional = true }   # offsetting robusto con arcos
# i_curve        = "0.2"                  # NO todavía: booleanas nativas sobre Bézier,
#                                         # publicado 2026-09-19, 168 descargas. Reevaluar en 2027.

# ── Texto ──────────────────────────────────────────────────────────────────
parley           = { version = "0.11", features = ["system", "accesskit"] }
fontique         = { version = "0.11", features = ["system"] }
skrifa           = "0.47"                 # contornos de glifo -> kurbo::BezPath
harfrust         = "0.13"                 # shaping (llega vía parley; explícito para control fino)

# ── Imagen ─────────────────────────────────────────────────────────────────
image            = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "tiff", "bmp", "webp", "hdr", "openexr", "qoi", "rayon"] }
zune-jpeg        = "0.5"                  # decodificación JPEG rápida (SIMD)
png              = "0.18"
image-webp       = "0.2"
ravif            = { version = "0.13", optional = true }   # exportar AVIF
kamadak-exif     = "0.6"
usvg             = "0.48"                 # parser/normalizador SVG (importador)
resvg            = { version = "0.48", optional = true }   # sólo para previews de referencia
oxipng           = { version = "10.2", optional = true }   # exportación PNG optimizada
qcms             = { version = "0.3", optional = true }    # gestión de color ICC (MPL-2.0)

# ── Formato / compresión ───────────────────────────────────────────────────
zip              = { version = "8.6", default-features = false, features = ["deflate", "zstd", "time"] }
zstd             = "0.14"
flate2           = { version = "1.1", default-features = false, features = ["zlib-rs"] }  # puro Rust y rápido
lz4_flex         = "0.14"                 # caché de tiles y undo en RAM

# ── Datos, undo, serialización ─────────────────────────────────────────────
serde            = { version = "1.0", features = ["derive", "rc"] }
serde_json       = "1.0"
rkyv             = { version = "0.8", features = ["bytecheck"] }   # autosave / historial persistente
imbl             = { version = "7.0", features = ["serde"] }       # árbol del documento (MPL-2.0, GPL-compatible)
slotmap          = "1.0"                  # arena con IDs generacionales
smallvec         = { version = "1.15", features = ["union", "const_generics"] }
memmap2          = "0.9"
uuid             = { version = "1.18", features = ["v4", "serde"] }

# ── Concurrencia ───────────────────────────────────────────────────────────
rayon            = "1.12"
crossbeam-channel = "0.5"
parking_lot      = "0.12"
# tokio SÓLO para el hilo de servicios (portales XDG, updates). Nada de async en el core.
tokio            = { version = "1.53", default-features = false, features = ["rt", "macros", "time", "sync"] }

# ── Observabilidad y errores ───────────────────────────────────────────────
tracing          = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
puffin           = { version = "0.20", optional = true }
puffin_egui      = { version = "0.30", optional = true }
wgpu-profiler    = { version = "0.28", optional = true }
thiserror        = "2.0"
anyhow           = "1.0"
directories      = "6.0"

# ── Dev / test ─────────────────────────────────────────────────────────────
[workspace.dependencies.criterion]
version = "0.8"
features = ["html_reports"]

[workspace.dependencies.insta]
version = "1.48"
features = ["json", "redactions", "filters"]

[workspace.dependencies.image-compare]
version = "0.5"

[workspace.dependencies.proptest]
version = "1.11"

[workspace.dependencies.arbitrary]
version = "1.4"
features = ["derive"]

[workspace.dependencies.egui_kittest]
version = "0.36"
features = ["wgpu", "snapshot"]

# ── Perfiles ───────────────────────────────────────────────────────────────
[profile.dev]
opt-level = 1            # el código propio, mínimamente optimizado

[profile.dev.package."*"]
opt-level = 3            # dependencias SIEMPRE optimizadas: sin esto, vello_cpu
                         # y zune-jpeg hacen el modo debug inusable

[profile.release]
opt-level     = 3
lto           = "thin"
codegen-units = 1
panic         = "unwind"   # NO "abort": queremos recuperar el documento tras un panic
strip         = "debuginfo"

[profile.dist]             # perfil de empaquetado
inherits      = "release"
lto           = "fat"
debug         = 1          # symbols para symbolicar crash reports
```

### `deny.toml` (cumplimiento de licencias, obligatorio en CI)

```toml
[licenses]
version = 2
confidence-threshold = 0.93
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "MPL-2.0",                # GPL-compatible
  "Unicode-3.0", "Unicode-DFS-2016",
  "CC0-1.0", "Unlicense",
  "OFL-1.1",                # fuentes empaquetadas
  "GPL-3.0-or-later",       # nuestro propio código
]
# Prohibido explícitamente: cualquier GPL-*-only de terceros (nos ataría a esa
# versión exacta) y las licencias no-libres.
exceptions = []

[bans]
multiple-versions = "warn"
deny = [
  { name = "openssl-sys" },   # usar rustls si algún día hay red
]

[advisories]
version = 2
yanked = "deny"
```

### Requisitos mínimos declarados al usuario

| | Mínimo | Recomendado |
|---|---|---|
| **Linux** | glibc 2.35 (Ubuntu 22.04, Debian 12, Fedora 36, RHEL 9), Wayland o X11, GL 3.3 | Wayland, Vulkan 1.1+, Mesa 23+ |
| CPU | x86-64-v2 (SSE4.2) o aarch64 | 4 núcleos, AVX2 |
| RAM | 4 GB | 16 GB |
| GPU | Cualquiera con GL 3.3, o **ninguna** (lavapipe/llvmpipe) | GPU dedicada con Vulkan 1.1 y compute |

---

## RIESGOS Y PLANES B

### Riesgos críticos (pueden hundir el proyecto)

| # | Riesgo | Prob. | Impacto | Señal temprana | Plan B |
|---|---|---|---|---|---|
| **R1** | **Licencia:** copiar/traducir código de XaraLX (GPL-2.0-only) contamina el proyecto y lo hace **incompatible con winit (Apache-2.0)**, obligando a reescribir la capa de ventanas | Media | **Fatal** | Cualquier PR que cite ficheros de XaraLX | Procedimiento clean-room documentado desde el día 1: un documento `docs/format/` escrito leyendo el original, y la implementación hecha **sólo** desde ese documento. Registrar quién ha leído qué. Si ya ocurriera: reescribir el módulo afectado con otra persona |
| **R2** | **winit 0.31 no estabiliza** o rompe la API repetidamente; sin él no hay presión de stylus | Alta | Alto | Cadencia de betas > 3 meses sin RC | (a) Quedarse en la beta fijada indefinidamente (es usable); (b) volver a **winit 0.30.13 + `octotablet`**, que cubre Wayland y Windows Ink completos; (c) en el peor caso, backend Wayland propio con `smithay-client-toolkit` (protocolo `tablet_v2` directo) |
| **R3** | **Vello GPU no madura** lo suficiente para documentos grandes, o presenta artefactos de conflation inaceptables | Media | Alto | Los golden tests GPU no convergen con los CPU | El backend `vello_cpu` **ya está en producción desde el día 1** (es la referencia). Plan B real: `lyon 1.0` + pipelines wgpu propias con MSAA 4×. La fachada `xarast-raster` hace que el cambio sea localizado |
| **R4** | **Calidad de las booleanas** insuficiente: degradación de curvas tras operaciones encadenadas | Alta | Alto | Tests de `proptest` de conservación de área fallando | (a) Tabla de trazabilidad para restaurar curvas originales (ya en el diseño); (b) migrar a **`i_curve`** cuando madure; (c) implementación propia basada en el algoritmo de Graphite (GPL, compatible con GPLv3) |
| **R5** | **Alcance:** Xara Xtreme tiene 15 años de funcionalidades; el proyecto se ahoga antes de ser útil | **Muy alta** | **Fatal** | Fase 1 que no termina en 6 meses | Definir un **MVP brutal**: abrir/guardar `.xarast`, dibujar/editar paths, rellenos y trazos básicos, texto simple, capas, undo, exportar PNG/SVG. **Todo lo demás es fase 2+.** Publicar una alfa usable a los 6 meses aunque haga poco |

### Riesgos altos

| # | Riesgo | Prob. | Impacto | Plan B |
|---|---|---|---|---|
| R6 | egui no aguanta la densidad de una UI profesional (100+ controles por panel) | Media | Medio | Ya está mitigado por la arquitectura: la UI está en `xarast-ui` sobre un trait; sustituir por `iced 0.14` costaría ~2 meses, no el proyecto. Prototipar **el panel más complejo (galería de colores + árbol de capas) en la semana 2** para descubrirlo pronto |
| R7 | El shim propio `winit 0.31 ↔ egui` consume más mantenimiento del previsto | Media | Medio | Contribuir el trabajo upstream a `egui-winit`; o migrar a `eframe` en cuanto egui suba a winit 0.31, conservando `octotablet` como fuente de presión |
| R8 | Wayland: bugs específicos por compositor (GNOME/Mutter vs KWin vs wlroots vs Hyprland) | **Alta** | Medio | Matriz de CI con los 4 compositores bajo `weston`/`sway` headless + `wlr-randr`; canal de bug reports con `XARAST_DEBUG_WAYLAND=1` que vuelque el protocolo |
| R9 | Rendimiento de arranque del AppImage (descompresión squashfs de 80 MB) | Media | Bajo | `appimagetool --comp zstd`; medir con `hyperfine`; objetivo < 800 ms en frío |
| R10 | glibc 2.35 deja fuera a usuarios de distros LTS antiguas (RHEL 8, Ubuntu 20.04) | Baja | Bajo | Segundo AppImage construido sobre `manylinux_2_28` sólo en releases mayores |
| R11 | Sin firma en Windows, SmartScreen bloquea las descargas y mata la adopción | Alta (fase 2) | Alto | Presupuestar Azure Trusted Signing (~120 €/año) desde el principio de la fase Windows |
| R12 | macOS: winit no da tableta; hay que escribir el backend AppKit | Cierta | Medio | Ya presupuestado (~1 semana). Alternativa: usar SDL3 sólo en macOS para la entrada de tableta |

### Riesgos medios / bajos

| # | Riesgo | Mitigación |
|---|---|---|
| R13 | `octotablet` sin publicar en crates.io desde 2024 (bus factor 1) | **Vendorizar** en `third_party/octotablet` con el commit fijado; es MIT |
| R14 | Crates pre-1.0 en el camino crítico (parley, vello, fontique, kurbo) | Todos son Linebender, con roturas mecánicas y buen changelog. Envolver cada uno en un crate propio. Fijar versiones exactas en `Cargo.lock` y subir en lotes trimestrales |
| R15 | `i_overlay` sube a 10.0 con roturas | Está tras `xarast-geom::boolean()`. Coste: 1–2 días |
| R16 | Tamaño del binario (Rust + wgpu + vello + parley + image) supera los 100 MB | `strip`, `lto = "fat"`, `opt-level = "s"` en los crates de codecs poco usados, features opcionales para AVIF/oxipng. Objetivo: AppImage < 80 MB |
| R17 | `zstd` es C: problemas de cross-compilación a aarch64 | Se compila con `cc` sin problemas en runner ARM nativo (que es lo que usamos). Alternativa pura Rust: `ruzstd` (sólo descompresión) + `zstd-safe` opcional |
| R18 | Golden tests inestables entre versiones de `vello_cpu` | Fijar versión exacta de `vello_cpu`; regenerar el corpus conscientemente en cada subida, revisando los diffs |
| R19 | Gestión de color ignorada hasta tarde → rediseño doloroso | Meter el campo `ColorSpace` en el modelo de color **desde el commit 1**, aunque sólo soporte sRGB al principio |
| R20 | AccessKit incompleto en Linux para el canvas (los objetos del dibujo no son accesibles) | Exponer sólo los controles de UI como accesibles; el lienzo como un único nodo con descripción. Es lo que hacen Inkscape y Krita |

### Decisiones que hay que tomar ANTES de escribir código

1. **Licencia definitiva** y sustitución del `LICENSE` MIT actual. (§1.3)
2. **Compromiso de sala limpia** documentado y firmado, con registro de quién lee el fuente original. (§1.2)
3. **Alcance del MVP** cerrado por escrito. (R5)
4. **Prototipo de 2 semanas** que valide de golpe los tres supuestos más arriesgados: (a) egui + winit 0.31 + wgpu 30 en la misma superficie; (b) presión del stylus llegando de extremo a extremo en Wayland; (c) `vello_cpu` renderizando un path con degradado y comparándose bit-a-bit en CI. Si algo de esto falla, el stack cambia ahora y no dentro de seis meses.

---

## Fuentes

Consultadas el 19 de septiembre de 2026.

**Datos de versión, licencia y mantenimiento:** API de crates.io (`https://crates.io/api/v1/crates/<crate>`) para los ~110 crates evaluados; `docs.rs` para features y APIs.

- [crates.io](https://crates.io/) — versiones, licencias, fechas de publicación y descargas
- [docs.rs/winit/0.31.0-beta.3](https://docs.rs/crate/winit/0.31.0-beta.3) y [`TabletToolData`](https://docs.rs/winit/0.31.0-beta.3/winit/event/struct.TabletToolData.html), [`PointerSource`](https://docs.rs/winit/0.31.0-beta.3/winit/event/enum.PointerSource.html)
- [winit CHANGELOG v0.31](https://raw.githubusercontent.com/rust-windowing/winit/master/winit/src/changelog/v0.31.md)
- [egui CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md) · [docs.rs/crate/eframe/0.36.2/features](https://docs.rs/crate/eframe/0.36.2/features) · [docs.rs/crate/egui-winit/0.36.2/features](https://docs.rs/crate/egui-winit/0.36.2/features)
- [AccessKit README](https://raw.githubusercontent.com/AccessKit/accesskit/main/README.md)
- [Slint LICENSE.md](https://github.com/slint-ui/slint/blob/master/LICENSE.md)
- [wgpu CHANGELOG](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/CHANGELOG.md)
- [Vello README](https://raw.githubusercontent.com/linebender/vello/main/README.md)
- [kurbo 0.13 docs](https://docs.rs/kurbo/latest/kurbo/)
- [Parley README](https://raw.githubusercontent.com/linebender/parley/main/README.md) · [docs.rs/crate/parley/0.11.1/features](https://docs.rs/crate/parley/0.11.1/features)
- [iOverlay](https://github.com/iShape-Rust/iOverlay) · [i_curve docs](https://docs.rs/i_curve/latest/i_curve/) · [iCurve](https://github.com/iShape-Rust/iCurve)
- [octotablet](https://github.com/Fuzzyzilla/octotablet) · [wintab_lite](https://github.com/thehappycheese/wintab_lite)
- [docs.rs/crate/zip/8.6.0/features](https://docs.rs/crate/zip/8.6.0/features) · [docs.rs/crate/flate2/1.1.10/features](https://docs.rs/crate/flate2/1.1.10/features)
- [Xara LX — cabecera de licencia (Kernel/group.h)](https://github.com/samuell/xara-xtreme/blob/master/Kernel/group.h) · [Xara Xtreme LX (Wikipedia)](https://en.wikipedia.org/wiki/Xara_Xtreme_LX) · [xaraxtreme.org — Licensing and contributing](http://www.xaraxtreme.org/Developers/developers-licensing-a-contributing.html)
- [AppImage — Best practices](https://docs.appimage.org/reference/best-practices.html) · [linuxdeploy user guide](https://github.com/AppImage/docs.appimage.org/blob/master/source/packaging-guide/from-source/linuxdeploy-user-guide.rst) · [AppImage (Wikipedia)](https://en.wikipedia.org/wiki/AppImage)
- [Snap vs Flatpak vs AppImage — comparativa 2026](https://computingforgeeks.com/snap-vs-flatpak-vs-appimage/) · [Flatpak vs Snap vs AppImage in 2026](https://sumguy.com/flatpak-vs-snap-vs-appimage/)
- [The Rust GUI Landscape in 2026](https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html) · [The State of Rust GUI — Rust Bytes](https://weeklyrust.substack.com/p/the-state-of-rust-gui-the-good-and) · [Thanks for All the Frames: Rust GUI Observations](https://tritium.legal/blog/desktop)
- [Iced 0.14 (Phoronix)](https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary) · [Release 0.14.0 · iced-rs/iced](https://github.com/iced-rs/iced/releases/tag/0.14.0)
- [Graphite — progress report](https://graphite.art/blog/graphite-progress-report-q3-2024/) · [Bezier-rs (lib.rs)](https://lib.rs/crates/bezier-rs)
