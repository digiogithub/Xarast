# Xarast — memoria del proyecto

Reimplementación en Rust de **Xara Xtreme / Xara LX**. Editor vectorial + foto,
alto rendimiento, Linux/Wayland primero (AppImage), luego Windows y macOS.

## Hechos permanentes (no re-investigar)

- **Por qué no se porta el original:** todo el rasterizado vive en la librería
  **binaria cerrada** `libs/*/libCDraw.a` (licencia propia en `libs/LIBS-LICENSE`,
  nunca liberada). Solo existen las cabeceras `GDraw/gdraw.h`, `gdraw2.h`,
  `gconsts.h`. Sin CDraw el programa no dibuja. → **hay que reimplementar el motor**.
- El original usa **wxWidgets 2.6/2.8 + GTK2**, sin Wayland, binarios de 2006 solo
  x86/x86_64/ppc/darwin-ppc.
- El fork del original está en **`/home/user/xara-xtreme`** (rama
  `claude/xara-xtreme-rust-port-ktggmu`). Es **solo lectura**: referencia
  normativa de comportamiento, nunca se traduce línea a línea.
- Tamaño del original: `Kernel/` 559 `.cpp` + 627 `.h`; `wxOil/` 182 `.cpp`.
- **Formato `.xar`**: definido en `Kernel/cxf*.{cpp,h}`; **211 tags** en
  `Kernel/cxftags.h`. Corpus de validación real en `xara-xtreme/testfiles/` y
  `xara-xtreme/Designs/`.
- **Formato nativo `.xarast`**: contenedor ZIP con **SVG** dentro + recursos
  binarios deduplicados. Requisito: debe abrirse con degradación elegante en
  navegador/Inkscape y con fidelidad total en Xarast.

## Convenciones

- **Idioma:** documentación y comentarios de diseño en **español**; código,
  identificadores, mensajes de commit y doc-comments de API en **inglés**.
- **Rama de trabajo:** `claude/xara-xtreme-rust-port-ktggmu` en ambos repos.
  Nunca empujar a otra rama.
- **Unidades internas:** millipoints (`i32`) como en Xara para E/S; el motor
  trabaja en `f64`/`f32` según la capa (ver `docs/research/03-motor-render.md`).
- `unsafe` solo en fronteras FFI justificadas y documentadas.
- El parser de `.xar` nunca debe entrar en pánico ni desbordar con entrada
  corrupta: se desarrolla con `cargo-fuzz` desde el inicio.

## Estructura

- `docs/00-vision-y-alcance.md` — visión, alcance por entregas, principios.
- `docs/research/01..06` — especificaciones e investigación (xar, modelo, render,
  funcionalidad, stack, xarast).
- `docs/10-arquitectura.md` — arquitectura de crates.
- `docs/phases/` — plan de ejecución por fases.
- `docs/memory/` — notas persistentes por subsistema; **actualízalas al cerrar
  cada tarea relevante**.

## Reglas de trabajo para agentes

1. Antes de empezar, lee `docs/memory/INDEX.md` y la nota del subsistema que toques.
2. Al terminar, **escribe/actualiza** la nota de memoria correspondiente con:
   decisiones tomadas, callejones sin salida, invariantes descubiertos.
3. No dupliques investigación ya registrada en `docs/research/`.
4. Commits pequeños y descriptivos, en inglés, con prefijo de área
   (`xar:`, `core:`, `render:`, `ui:`, `pkg:`, `docs:`).
