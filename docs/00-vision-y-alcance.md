# Xarast — Visión y alcance

> Documento raíz del proyecto. Define **qué** construimos, **por qué** de esta forma y
> **hasta dónde** llega cada entrega. El resto de documentos (`docs/research/*`,
> `docs/phases/*`) desarrollan las decisiones técnicas y el plan de ejecución.

---

## 1. Qué es Xarast

**Xarast** es una reimplementación nativa en **Rust** de *Xara Xtreme / Xara LX*:
un editor de gráficos **vectoriales** con capacidades de **retoque fotográfico**
integradas, enfocado en **rendimiento extremo** e **interacción directa** sobre el
lienzo.

Objetivo global: recuperar la experiencia que hizo único a Xara —redibujado
instantáneo, antialiasing de máxima calidad, manipulación *live* de rellenos,
transparencias, blends y efectos directamente en el lienzo— sobre una base
moderna, multiplataforma y mantenible.

**Plataformas, por orden de prioridad:**

| Orden | Plataforma | Entrega | Estado |
|---|---|---|---|
| 1 | **Linux** (Wayland nativo, X11 por XWayland/fallback) | **AppImage** x86_64 (después aarch64) | Prioridad absoluta |
| 2 | Windows 10/11 | MSI + portable | Posterior |
| 3 | macOS 12+ (Apple Silicon + Intel) | `.app` notarizado, universal | Posterior |

---

## 2. Por qué reimplementar y no portar

El análisis del código original (`/xara-xtreme`, fork de Xara LX) confirma que
**no es viable un port directo**:

1. **El motor de render es una librería binaria cerrada.** Todo el rasterizado
   vectorial vive en `libs/{x86,x86_64,ppc,darwin}/libCDraw.a`, distribuida
   **solo en binario** bajo una licencia temporal propia (`libs/LIBS-LICENSE`)
   que nunca llegó a abrirse. Las cabeceras públicas (`GDraw/gdraw.h`,
   `gdraw2.h`, `gconsts.h`) describen la API pero **ninguna implementación**.
   CDraw es exactamente "lo principal de Xara que lo hacía tan fácil de usar y a
   la vez potente": rasterizado antialias, gradientes con perfiles no lineales,
   transparencias con modos de mezcla propios, rellenos fractales, blur/bevel.
   Sin él, el programa no dibuja nada.
2. **Sin soporte de 64 bits real ni de plataformas modernas.** Los binarios son
   ELF x86/x86_64 de 2006, enlazados contra glibc y libstdc++ de la época; no hay
   versión aarch64, ni Apple Silicon, ni Windows de 64 bits.
3. **wxWidgets 2.6/2.8 y GTK2.** La capa de UI (`wxOil/`, 182 `.cpp`) depende de
   toolkits sin soporte Wayland y hoy sin mantenimiento en esas versiones.
4. **Base C++ de ~1.200 ficheros** (559 `.cpp` + 627 `.h` solo en `Kernel/`) con
   modismos de los 90 (macros propias, RTTI casero, `TRACE`, gestión manual de
   memoria, `MonotonicTime`, tipos `INT32`/`UINT32` propios) y dependencias de
   herramientas de build (`autogen.sh`, `configure.in`) que ya no funcionan.
5. **El código incompleto de por sí.** Xara LX era un port **inacabado** de la
   versión Windows; múltiples subsistemas están `#if 0` o con `PORTNOTE`.

**Pero el código original sigue siendo un activo enorme**, y lo usamos como
**especificación ejecutable**:

- Es la única documentación existente del **formato `.xar`** (`Kernel/cxf*.{cpp,h}`,
  211 tags en `Kernel/cxftags.h`).
- Define el **modelo de documento** (jerarquía de nodos, atributos, capas).
- Documenta la **semántica** de cada herramienta, relleno, transparencia y efecto.
- Los ficheros de `testfiles/` y `Designs/` son un **corpus de validación** real.

> **Regla del proyecto:** el C++ original es *referencia normativa de
> comportamiento*, nunca base de código a traducir línea a línea.

---

## 3. Principios de diseño

1. **Rendimiento primero.** El redibujado interactivo es la característica
   identitaria. Objetivo: 60 fps en pan/zoom sobre documentos de 100k objetos, en
   GPU integrada.
2. **Wayland nativo de primera clase.** Escalado fraccional, decoraciones del
   lado cliente, portales XDG para diálogos de fichero, tabletas con presión vía
   protocolo `tablet_v2`.
3. **Formato nativo abierto y legible.** `.xarast` = contenedor comprimido con
   **SVG** dentro. Un fichero de Xarast debe poder abrirse (con degradación
   elegante) en un navegador o en Inkscape, y con fidelidad total en Xarast.
4. **Importación fiel de `.xar`.** El legado de documentos de Xara debe abrirse
   sin pérdidas apreciables. Es requisito de producto, no un "nice to have".
5. **Seguridad de memoria y sin `unsafe` salvo en fronteras FFI justificadas.**
   Un parser de formato binario legacy es superficie de ataque: el importador
   `.xar` se desarrolla con *fuzzing* desde el primer día.
6. **Núcleo desacoplado de la UI.** Todo el modelo, la geometría, la E/S y el
   render viven en crates sin dependencia del toolkit gráfico, para poder:
   herramienta CLI de conversión, tests *headless*, y cambio de UI sin reescribir.
7. **Paridad funcional incremental y medible.** El inventario de funcionalidad
   (`docs/research/04-inventario-funcionalidad.md`) es el backlog de paridad; cada
   fase cierra un subconjunto verificable.
8. **Determinismo y tests de regresión visual.** Cada cambio del motor de render
   se valida contra imágenes *golden*.

---

## 4. Alcance por entregas

### v0.1 — "Abre y dibuja" (Linux AppImage) — MVP
- Importa `.xar` (subconjunto que cubre la mayoría de documentos reales).
- Lee y escribe `.xarast`.
- Visualiza con render de alta calidad: paths, rellenos planos y degradados,
  transparencia, trazos, grupos, capas, bitmaps.
- Navegación fluida (pan/zoom), panel de capas, panel de colores.
- Herramientas: selector (mover/escalar/rotar/sesgar), rectángulo, elipse,
  edición de nodos básica, texto simple.
- Exporta PNG y SVG.
- Undo/redo.
- AppImage x86_64 funcional en Wayland.

### v0.2 — "Editor de verdad"
- Herramientas de relleno y transparencia interactivas en lienzo.
- Operaciones booleanas de formas, alineación, orden Z, agrupado.
- QuickShapes, freehand/bezier completos, herramienta de recorte.
- Texto avanzado (en path, formato de párrafo/carácter, OpenType).
- Galerías: colores, capas, bitmaps, fuentes.
- Exporta JPEG, WebP, PDF.

### v0.3 — "Lo que hacía especial a Xara"
- Blends, moulds (envelope/perspective), contours.
- Sombras, biselados, plumeado (feather) — efectos vivos.
- Rellenos fractales y bitmap.
- Fotos: ajustes no destructivos, recorte, máscaras.

### v1.0 — Multiplataforma
- Windows y macOS.
- Importación/exportación ampliada (EPS/PDF, EMF, animación).
- Rendimiento y estabilidad de producción.

**No-objetivos (explícitos):** compatibilidad binaria con plugins de Xara;
reproducir la UI de Xara píxel a píxel; escritura del formato `.xar` (solo
lectura); edición colaborativa en tiempo real (v1.0+).

---

## 5. Mapa de documentación

| Documento | Contenido |
|---|---|
| `00-vision-y-alcance.md` | Este documento |
| `research/01-formato-xar.md` | Especificación del formato binario `.xar` |
| `research/02-modelo-documento.md` | Modelo de nodos, atributos, capas del original |
| `research/03-motor-render.md` | Qué hacía CDraw y cómo reimplementarlo |
| `research/04-inventario-funcionalidad.md` | Backlog completo de paridad funcional |
| `research/05-stack-tecnologico.md` | Elección de crates y herramientas |
| `research/06-formato-xarast.md` | Especificación del formato nativo `.xarast` |
| `10-arquitectura.md` | Arquitectura de crates y flujo de datos |
| `phases/*` | Plan de ejecución fase a fase |
| `memory/*` | Notas persistentes de conocimiento por subsistema |
| `../CLAUDE.md` | Memoria operativa para agentes que trabajen en el repo |
