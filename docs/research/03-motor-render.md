# El motor de render de Xara (CDraw/GDraw): ingeniería inversa y plan de reimplementación en Rust

> **Estado:** INVESTIGACIÓN — base para la fase «motor de render» de Xarast.
> **Documento:** `docs/research/03-motor-render.md`
> **Fecha:** 2026-09-19
> **Fuentes primarias:** `/home/user/xara-xtreme/GDraw/{gdraw.h,gdraw2.h,gconsts.h,GVersion.h}`,
> `/home/user/xara-xtreme/libs/x86_64/libCDraw.a` (binario, 1 037 494 bytes, 64 objetos),
> `/home/user/xara-xtreme/Kernel/*`, `/home/user/xara-xtreme/wxOil/*`.
> **Depende de:** `docs/00-vision-y-alcance.md`, `research/02-modelo-documento.md`.

## Resumen ejecutivo

`libCDraw.a` es el rasterizador vectorial cerrado de Xara (versión de API **4.000**,
`GDraw/GVersion.h:100-102`). Es la única razón real por la que Xara LX no se puede portar: toda la
aplicación dibuja a través de él y no existe código fuente.

Cifras extraídas del binario:

| Métrica | Valor |
|---|---|
| Símbolos `T` (texto, globales) totales | **926** |
| API pública `extern "C"` (`GDraw_`, `GDraw2_`, `GColour_`, `GBitmap_`) | **126** |
| — de ellos `GDraw_` / `GColour_` / `GBitmap_` / `GDraw2_` | 55 / 50 / 15 / 6 |
| Métodos internos `GDraw::` (C++ mangled) | **640** |
| Funciones C++ globales internas (no `GDraw::`) | 65 |
| Declaraciones en cabecera sin símbolo `extern "C"` en la librería | 28 |
| Entradas usadas por la app y **no** declaradas en `gdraw.h` | 4 (`ClipPathToPath`, `GenerateWallShadow`, `GenerateFloorShadow`, `ContourBitmap`) |
| Modos de mezcla (`TransparencyEnum`) | **36 valores enumerados** = 12 familias × 3 variantes (plana/gradiente/genérica) |

**Recomendación (detallada en la Tarea C):** motor híbrido propio, `vello`/`vello_cpu`
(arquitectura *sparse strips*) como rasterizador base sobre `wgpu`, con una **capa de
composición propia** que implementa los modos de mezcla exóticos de Xara mediante LUT 2D
(256×256) en WGSL, más un *fallback* `vello_cpu`/`tiny-skia` para CPU y exportación
determinista.

---

## Índice

1. [Tarea A — Superficie de la API de CDraw](#1-tarea-a--superficie-de-la-api-de-cdraw)
2. [Tarea B — Semántica de render a replicar](#2-tarea-b--semántica-de-render-a-replicar)
3. [Tarea C — Plan de reimplementación en Rust](#3-tarea-c--plan-de-reimplementación-en-rust)
4. [Apéndices](#4-apéndices)

---

# 1. Tarea A — Superficie de la API de CDraw

## 1.1 Metodología

```bash
nm -g --defined-only libs/x86_64/libCDraw.a | grep ' T ' | sort    # 926 símbolos
nm -g --defined-only ... | c++filt                                  # desmangle
ar x libCDraw.a && objdump -d -r -C libCDraw_la-GScanT-D.o          # semántica real
```

Se cruzó la lista de símbolos con las declaraciones de `GDraw/gdraw.h` y `GDraw/gdraw2.h`, y
después se localizó cada punto de llamada en la aplicación (`Kernel/GDrawIntf.cpp`,
`wxOil/grndrgn.cpp`, `Kernel/beveler.cpp`, `Kernel/bshadow.cpp`, `Kernel/paths.cpp`).

**Arquitectura de acceso desde la app** (importante para el diseño del sustituto):

```
Kernel/*  y  wxOil/grndrgn.cpp
   └── GDrawContext / GDrawAsm      (Kernel/GDrawIntf.h:172, GDrawIntf.cpp)   ← fachada C++ con lock global
         └── XaDraw_* / XaColour_*  (Kernel/XaDraw.h:110 — capa de traza, se compila a nada)
               └── GDraw_* / GColour_* / GBitmap_*   (GDraw/gdraw.h)          ← libCDraw.a
```

Es decir: **existe ya un único punto de estrangulamiento** (`GDrawContext`, ~180 métodos
virtuales, `Kernel/GDrawIntf.h:180-433`). Cualquier reimplementación puede sustituir esa clase
y nada más — dato clave para una migración incremental.

## 1.2 Modelo de contexto

CDraw no es orientado a objetos hacia fuera: cada llamada recibe un `pGCONTEXT` opaco.

```c
struct GCONTEXT { DWORD Valid; DWORD Data[1]; };   // GDraw/gconsts.h:237, Valid == 0xC90FDAA2
```

El tamaño se pide en tiempo de ejecución con `GDraw_ContextLength()` y la app lo reserva con su
propio `malloc` (`Kernel/GDrawIntf.cpp:258-272`). Del desensamblado se deduce que el contexto es
**enorme** (los accesos a las tablas de gris están en `ctx+0x610f0`, `ctx+0x614f0`, `ctx+0x618f0`,
es decir ≥ 400 KiB): contiene todas las LUT de dither, tablas de gradiente, estado de estilo y
los punteros a las rutinas de scanline seleccionadas. Es un contexto **por hilo**
(`GDrawAsm::GetContextForCurrentThread`, `Kernel/GDrawIntf.cpp:183`), y toda la API está además
serializada por una `CriticalSection` global en la fachada.

## 1.3 API pública agrupada por funcionalidad

### (a) Inicialización, contexto, memoria, errores — 16 funciones

| Función | Firma (resumida) | Notas |
|---|---|---|
| `GDraw_GetVersion` | `DWORD()` | HIWORD=major (4), LOWORD=minor (0). La app aborta si no coincide (`GDrawIntf.cpp:236`). |
| `GDraw_GetSvnVersion` | `const char*()` | Solo no-MSW. |
| `GDraw_ContextLength` | `INT32()` | Tamaño a reservar para `GCONTEXT`. |
| `GDraw_Initialise` | `(pGCONTEXT, pcGCONTEXT pOld=NULL)` | |
| `GDraw_CopyContext` | `(pcGCONTEXT, pGCONTEXT)` | Clonado de estado (usado para contextos por hilo). |
| `GDraw_Terminate`, `GDraw_Clean` | `(pGCONTEXT)` | |
| `GDraw_SetMemoryHandlers` | `(pGCONTEXT, BYTE*(*alloc)(UINT32), void(*free)(BYTE*))` | La app inyecta `CCMalloc/CCFree`. |
| `GDraw_SetStackLimit` / `GDraw_SetStackSize` | `(pGCONTEXT, ...)` | El rasterizador recursa (subdivisión de Bézier) y necesita saber el límite de pila; la app le da 100 KiB (`grndrgn.cpp:660`). |
| `GDraw_ClearLastError` / `GDraw_GetLastError` / `GDraw_GetLastErrorText` | | 21 códigos `eError` (`gconsts.h:119`). |
| `GDraw_ComparePalettes` | `(pcGCONTEXT, pcLOGPALETTE, bool)` | |
| `GDraw_GetMaxBitmapWidth` / `GDraw_GetMaxBitmapDepth` | `INT32()` | 0x800 × 0x800 (`gconsts.h:355`). |

### (b) Destino de dibujo, matriz, transformación — 8 funciones

| Función | Firma | Notas |
|---|---|---|
| `GDraw_SetDIBitmap` | `(ctx, pcBITMAPINFOHEADER, pBYTE, UINT32 Format16BPP=FORMAT16BPP_555)` | `gdraw.h:443`. Bitmap DIB bottom-up; `biCompression=0x80000001` marca 32bpp con canal de transparencia (`grndrgn.cpp:7654`, `capturemanager.cpp:285`). |
| `GDraw_SetInvertedDIBitmap` | idem | Para DIB top-down. |
| `GDraw_SetMatrix` | `(ctx, pcGMATRIX)` | Ver §2.1. |
| `GDraw_MaxScale` | `(ctx, pcGMATRIX, pcRECT)` | Escala máxima de la matriz sobre un bbox (para elegir flatness/DPI). |
| `GDraw_TransformPath` | `(ctx, pcPOINT in, pPOINT out, UINT32 len, pcGMATRIX)` | |
| `GDraw_ScrollBitmap` | `(ctx, INT32 x, INT32 y)` | Desplazamiento del bitmap destino: optimización de *scroll* incremental. |
| `GDraw_SetFlatness` | `(ctx, UINT32)` | Tolerancia de aplanado de Bézier, en unidades del documento (millipuntos). |
| `GDraw_SetMiterLimit` | `(ctx, UINT32)` | |

### (c) Relleno y trazo de caminos — 12 funciones

| Función | Firma | Notas |
|---|---|---|
| `GDraw_FillPath` | `(ctx, pcPOINT, pcBYTE types, UINT32 len, UINT32 Winding)` | `Winding`: 0 = par/impar (alternate), 1 = nonzero. La app: `(RR_WINDINGRULE()==EvenOddWinding)?0:1` (`grndrgn.cpp:2769`). El bit 1 (`<<1`) selecciona el polígono **inverso** (usado por el beveler: `beveler.cpp:790`). |
| `GDraw_FillRectangle` | `(ctx, pcRECT)` | |
| `GDraw_FillPoint` | `(ctx, pcPOINT)` | Un píxel (blobs de edición). |
| `GDraw_FillRegion` | `(ctx, pcREGION, pcPOINT offset)` | |
| `GDraw_StrokePath` | `(ctx, pts, types, len, bool Close, UINT32 LineWidth, DWORD Caps, DWORD Join, pcDashType)` | `gdraw.h:492`. Caps: BUTT/ROUND/SQUARE; Join: MITER/ROUND/BEVEL (`gconsts.h:166,172`). |
| `GDraw_StrokePathToPath` | `(ctx, in…, out…, …)` | **Convierte el trazo en contorno relleno.** Base de «convertir línea en forma» y del contorno (`Kernel/paths.cpp:5646`). |
| `GDraw_CalcStrokeBBox` | `(ctx, …, pRECT, …)` | BBox exacto del trazo (`Kernel/combshps.cpp:1461`, `nodetext.cpp:2891`). |
| `GDraw_HintPath` | `(ctx, pPOINT, pcBYTE, UINT32, bool, UINT32 LineWidth)` | *Hinting* de rejilla: alinea tramos horizontales/verticales al píxel. |
| `GDraw_SetHintingFlag`, `GDraw_SetDashAdjustmentFlag` | `(ctx, bool)` | |
| `GDraw_IsOverlap`, `GDraw_IsStrokeOverlap` | | Detección de impacto (hit-testing) sin dibujar. |
| `GDraw_CalcBBox`, `GDraw_CalcSimpleBBox` | | BBox con/sin aplanado de curvas. |
| `GDraw_GetStatistics` | `(ctx, path…, pSTATISTICS)` | Momentos de orden 0/1/2 de color y posición sobre el área rellena (`STATISTICS`, `gconsts.h:321`): sumas R,R²,RX,RY,… Se usa para el color medio de una región. |

### (d) Regiones y recorte — 7 funciones

| Función | Notas |
|---|---|
| `GDraw_MakeRegion`, `GDraw_MakeUnclippedRegion` | Rasterizan un camino a una `REGION` (`gconsts.h:296`): `Type` 0 = sin AA, 1 = AA de 5 líneas, 2 = AA de 9 líneas; más `RECT` y datos RLE. |
| `GDraw_ClipRegion`, `GDraw_GetClipRegion` | Recorte por región arbitraria (clip paths, ClipView). |
| `GDraw_ClipRectangle`, `GDraw_DeviceClipRectangle`, `GDraw_GetDeviceClipRectangle` | Recorte rectangular en coordenadas de documento y de dispositivo. |

### (e) Color sólido, dither, paleta — 12 funciones

`GColour_SetColour` (COLORREF con dither), `GColour_SetSolidColour` (sin dither, con *hint* de
formato de pantalla), `GColour_SetWordColour` (valor crudo de 32 bits), `GColour_SetInvert`
(XOR), `GColour_SetDitherStyle` (8 estilos, `gconsts.h:224`: difusión, ordenado, gris ordenado,
Floyd-Steinberg, ninguno, gris simple, difusión gris, exacto), `GColour_SelectPalette`,
`GColour_InitialiseWithPalette`, `GColour_SetConversionPalette`, `GColour_SetHalftoneOrigin`
(origen del patrón de trama, se reajusta por captura: `grndrgn.cpp:8739`),
`GColour_ReturnBrush`, `GColour_ReturnBrushRGB`, `GColour_SetGreyConversionValues`
(pesos R,G,B de luminancia — **parámetro clave de todos los modos de mezcla**, §2.6).

### (f) Gradientes — 16 funciones

| Función | Descripción |
|---|---|
| `GColour_BuildGraduationTable` / `…32` | Construye la LUT de 256 (o 2048) entradas entre dos colores; `HSVFlag` elige espacio de interpolación. La variante `…32` produce `COLORREF` puro (32 bpp); la normal produce `DitherBlock` de 4 DWORD por entrada (dither 4×4 para pantallas de 8/16 bpp). |
| `GColour_AddToGraduationTable` | Rellena **una** entrada — así la app inyecta rampas arbitrarias multiparada y perfiles bias/gain (`Kernel/gradtbl.cpp:549`). |
| `GColour_SetGraduation` | `(ctx, Style, pcGraduationTable, A, B, C)` — 3 puntos de control. |
| `GColour_SetGraduation4` | + `D` → gradiente **en perspectiva** (cuadrilátero). |
| `GColour_Set3WayGraduation(4)` | Malla de 3 colores (interpolación baricéntrica). |
| `GColour_Set4WayGraduation(4)` | Malla de 4 colores (bilineal). |
| `GColour_SetGourand` | Triángulo Gouraud. |
| `GColour_SetTransparentGraduation(4)`, `…3Way…`, `…4Way…` | Los mismos cinco tipos pero sobre el canal de transparencia. |
| `GColour_GetGraduationTableSize/Length` y variantes `Long`/`Transparent` | Tamaños de tabla (256 vs 2048 entradas; ver `Kernel/gradtbl.cpp:208`). |

`Style` codifica (visto en `grndrgn.cpp:2480-2520` y `4150-4170`):
`bits 0-7` = forma (0 lineal, 1 radial/elíptico, 2 cónico, 3 cuadrado/diamante; para mallas
0/1/2 = simple 3-color / simple 4-color / teselado) `| 0x80` repetición HQ;
`bits 8-15` = tipo de transparencia (`TransparencyEnum`);
`bit 16` = repetición para transparencias.

### (g) Transparencia — 5 funciones + 36 estilos

`GColour_SetTransparency(ctx, COLORREF rgbt, DWORD Style)` — el byte alto de `rgbt` es el valor
de transparencia (0 = opaco, 255 = transparente) y `Style` es un `TransparencyEnum`
(`gconsts.h:193`). `GColour_SetTransparencyLookupTable(ctx, pcBYTE)` instala la LUT del «tipo
12» (transparencia por tabla). Las variantes graduadas se configuran con las funciones de la
sección (f). Tabla completa en §2.6.

### (h) Rellenos de bitmap (tile) — 6 funciones

`GColour_SetTilePattern(4)`, `GColour_SetTransparentTilePattern(4)`, `GBitmap_PlotTile(4)`.
Firma representativa (`gdraw.h:351`):

```c
INT32 GColour_SetTilePattern(pGCONTEXT, pcBITMAPINFOHEADER, pcBYTE Bitmap, DWORD Style,
    pcPOINT A, pcPOINT B, pcPOINT C,           // paralelogramo (o A..D en la variante «4»)
    COLORREF DefaultColour, pcBGRT TranslationTable,
    pcBYTE Red, pcBYTE Green, pcBYTE Blue, pcBYTE TransparencyTable, INT32 TileOffset);
```

Las cuatro tablas de traducción permiten *contone*, corrección de color y separación en el
propio muestreo. `GColour_SetTileSmoothingFlag` (interpolación bilineal) y
`GColour_SetTileFilteringFlag` (filtrado de alta calidad para impresión/exportación) eligen el
remuestreo (`grndrgn.cpp:3432`).

### (i) Procesado de bitmap / efectos de imagen — 15 funciones `GBitmap_`

`SetBrightness(double)`, `SetContrast(double)`, `SetGamma(double)`, `SetPostGamma(double)`,
`SetSaturation(double)`, `SetContone(UINT32 style, COLORREF start, COLORREF end)`,
`SetBias(UINT32 channel, double)`, `SetGain(UINT32 channel, double)`,
`SetInputRange(channel, BYTE, BYTE)`, `SetOutputRange(channel, BYTE, BYTE)`,
`SetMaxFilterSize(UINT32)`, `Sharpen(INT32)`, `Blur(INT32)`, `PlotTile(4)`.

El canal `3` es el de transparencia: la app aplica el perfil bias/gain del relleno graduado
justo ahí (`grndrgn.cpp:3730-3732` y `4313-4315`).

### (j) Conversión de color y separación — 8 funciones

`GColour_ConvertBitmap(ctx, srcInfo, src, dstInfo, dst, Dither)` — el conversor universal de
profundidad/paleta (usado por `wxOil/dibconv.cpp` en 6 sitios y por las capturas
`grndrgn.cpp:8055, 8270, 8461, 8961`). `GColour_SetSeparationTables(cyan, magenta, yellow,
black, UCR, blackGeneration)` — separación CMYK con *under-colour removal* y *black
generation* como LUT externas. `GColour_SetBitmapConversionTable(pcBGR)` — corrección de color
aplicada a bitmaps. `GColour_ConvertHSVtoRGB` / `GColour_ConvertRGBtoHSV` (sin contexto).
`GColour_SetMaxDiffusionError`, `GColour_ScaleBitmap` (ver §1.5: **declaradas pero no
exportadas como C**).

### (k) Bisel — 4 funciones en `gdraw.h` + 6 en `gdraw2.h`

`GDraw_SetBevelContrast/Lightness/Darkness(ctx, UINT32)` y
`GDraw_TranslateBevelValue(ctx, BYTE index, BYTE colour)`: controlan cómo el mapa de índices de
bisel (8 bits) se convierte en iluminación. La segunda API (`GDraw/gdraw2.h`, **sin contexto**,
estado global) genera ese mapa:

```c
INT32 GDraw2_SetDIBitmap(const BITMAPINFOHEADER*, const BYTE*, eBevelStyle, float LightAngle1, float LightAngle2);
INT32 GDraw2_FillTriangle (const POINT[3], double NormalX, double NormalY);
INT32 GDraw2_FillTrapezium(const POINT[4], double NormalX, double NormalY);
```

15 estilos de bisel (`gdraw2.h:104`): FLAT, ROUND, HALFROUND, FRAME, MESA_1/2, SMOOTH_1/2,
POINT_1/2a/2b, RUFFLE_2a/2b/3a/3b. Uso real en `Kernel/beveler.cpp:756, 764, 844`.

### (l) BBox de cambios (render incremental) — 3 funciones

`GDraw_ClearChangedBBox`, `GDraw_GetChangedBBox`, `GDraw_SetChangedBBox`. CDraw **acumula el
rectángulo de píxeles realmente tocados**; la app lo usa para saber qué blitear
(`grndrgn.cpp:5958, 6533, 7703`) y para dimensionar las capturas.

## 1.4 Entradas de la librería no declaradas en `gdraw.h`

Estas cuatro funciones son C++ globales (mangled) que la aplicación declara **en sus propias
cabeceras** y que, sin embargo, viven dentro de `libCDraw.a`:

| Símbolo (desmanglado) | Declarado en | Usado en | Función |
|---|---|---|---|
| `ClipPathToPath(POINT*,BYTE*,uint,double,POINT*,BYTE*,uint,double,uint,uint,POINT*,BYTE*,uint)` | `Kernel/gclip.h:108` y `:155` | `Kernel/pbecomea.cpp:285,305,314,411`, `Kernel/swfrndr.cpp:232` | **Booleanas de caminos** (AND, OR, EOR, NOT, intersección) con tolerancia; 8 estilos vía `CLIPPING_STYLE` (`Kernel/gclips.h:106`). |
| `GenerateWallShadow(...)` | `Kernel/bshadow2.h:113` | `Kernel/bshadow.cpp:526`, `Kernel/fthrconv.cpp:313` | **Desenfoque (blur) de máscara 8 bpp** por convolución circular. Base de sombra y *feather*. |
| `GenerateFloorShadow(...)` | `Kernel/bshadow2.h:128` | `Kernel/bshadow.cpp:749` | Sombra proyectada con inclinación/proyección. |
| `ContourBitmap(BYTE*,uint,BYTE*,BITMAPINFO*,double,uint)` | `Kernel/bshadow2.h:141` | `Kernel/bshadow.cpp:1158,1517`, `nodeliveeffect.cpp` | Contorno/dilatación de una máscara 8 bpp. |

Además, la librería contiene `G3D_*` (motor 3D de extrusión: `G3D_DefineView`,
`G3D_AddTriangleToView`, `G3D_PlotView`, `G3D_GetViewBBox`, `G3D_SetTruePerspectiveFlag`,
`G3D_AddFlatTriangleToView`) **solo como símbolos C++ mangled**, mientras `gdraw.h` los declara
`extern "C"` dentro de un bloque comentado: son inalcanzables desde la app tal cual está.

## 1.5 Discrepancias cabecera ↔ binario

Las 126 funciones `extern "C"` exportadas están **todas** documentadas en las cabeceras
(no hay API oculta en C). A la inversa, 28 declaraciones de cabecera no tienen símbolo C:

* 14 `G3D_*` — bloque comentado en `gdraw.h:638-720` (p. ej. `gdraw.h:649`), presentes mangled.
* `GDraw_FillTriangle`, `GDraw_InitialiseFillPath`, `GDraw_FillPathLine`, `GDraw_DoFillPath`,
  `GDraw_SaveContext/RestoreContext/SwapContext`, `GColour_Initialise`,
  `GColour_SetMultiRadial`, `GColour_SetSupersampleRate`, `GDraw2_SetMemoryHandlers`,
  `GDraw2_Terminate` — comentadas en la cabecera; algunas existen mangled
  (`GDraw_FillTriangle(GDraw*,const POINT*,const POINT*,const POINT*,uint)`).
* **`GColour_ScaleBitmap` y `GColour_SetMaxDiffusionError` están declaradas activas en
  `gdraw.h` pero solo existen mangled** (`_Z19GColour_ScaleBitmapP5GDraw…`): usarlas
  produciría un error de enlace. Son, de hecho, funcionalidad muerta en Xara LX.
* `GColour_SetMultiRadial` (relleno multirradial con «blobs») nunca llegó a publicarse: hay
  rastro de la estructura `GBLOB` comentada en `gconsts.h:340`.

## 1.6 Lo que revelan los 640 métodos internos (mapa de la implementación)

El desmanglado de los símbolos internos describe la arquitectura completa del rasterizador y es
la mejor especificación disponible del motor. Agrupados:

| Grupo | Símbolos representativos | Qué implica |
|---|---|---|
| Tabla de aristas | `GenEdgeTable`, `GenEdge`, `GenFastEdge`, `GenCurvedEdge(s)`, `ExtendEdgeTable`, `SortYEdges`, `SortOpenEdges`, `SortClosedEdges`, `TidyEdgeTable`, `ProcessEdges` | Rasterizado **scanline clásico con tabla de aristas activa** (no *tile-based*). |
| Relleno por regla | `FillWindingPolygon`, `FillAlternatePolygon`, `FillInverseWindingPolygon`, `FillInverseAlternatePolygon`, `FillAntialiasedPolygon`, `FillBevelPolygon` | 4 reglas (nonzero / evenodd × normal / inversa) + camino AA separado + camino de bisel. |
| Antialias | `AddScanline5x5`, `AddScanline17x5`, `AddScanline11x11`, `AddScanline12x11`, `ClipLineAA5/11/12/17`, `ClearScanlineAA`, `GenAntialiasedRegion` | Supermuestreo por *scanline* (§2.3). |
| Curvas | `FlattenCurve`, `DoFlattenCurve`, `Flatten`, `FlattenSplit`, `FlattenCCurve`, `CalcCurveLength`, `CBezier::Length` | Aplanado adaptativo por subdivisión. |
| Trazo | `DoStrokePath`, `StrokeLine`, `StrokeLineAndJoin`, `GenRoundCap`, `BBoxRoundCap`, `GenDashSection`, `MakeDashAdjustment`, `StrokeCurve_cbrt` | Trazo geométrico propio (offsetting con raíz cúbica para el error de curvas). |
| Píxel/blit por formato | `Pixel1/4/8/16/24/32`, `PixelCMYK`, `PixelT32`, y variantes `…C` (dither), `…_F` (filtrado), `…T` (transparencia), `…_X` (XOR) | ~120 rutinas de acceso a píxel: 1, 4, 8, 16 (555/565/655/664), 24, 32 bpp y **CMYK nativo**. |
| Gradientes | `LinearGradBlitLine`, `RLinearGrad…` (repetido), `RadialGradBlitLine(Start/End)`, `RRadialGrad…`, `SquareGrad…`, `RSquareGrad…`, `ConicalGradBlitLine{Start,End,Left,PosMid,NegMid}`, `…Grad4…` (perspectiva), `MGradX3/X34`, `DGradX3`, `GradX4/X44` | Un *blitter* especializado por (forma × repetición × perspectiva × con/sin transparencia). |
| Transparencia/mezcla | `CalcTransparency{A,B,C,D,H,L,Lu,S,Sn}[_T]`, `MergeTransparency…`, `Merge{,S,X,A,Bvl,Bvl2}Transparent[Grad]32[T]`, `MergeTransparencyMask` | 12 familias de mezcla × (plana, variable, graduada, bitmap) × (con/sin canal alfa destino). |
| Bisel | `FlatBevelBlitLine32N/P`, `RoundBevel…`, `MesaBevel…`, `SmoothBevel…`, `Ruffle2Bevel…`, `Ruffle3Bevel…`, `SimpleBevel…`, `GenBevel`, `TranslateBevelValue` | Un blitter por perfil de bisel, con rama N/P según el signo de la iluminación. |
| Tablas estáticas | `aContrastTable`, `aSaturationTable1/2`, `aChopTable`, `aDiv3Table`, `aSquareTable`, `aArcTable`, `aArcCosTable`, `a{Round,Mesa1,Mesa2,Smooth1,Smooth2,HalfRound,Ruffle2,Ruffle3}BevelTable`, `apMulTable`, `apIMulTable`, tablas de dither 4/8/16 bpp | **Todo se resuelve con LUT precalculadas**, no con aritmética por píxel. |
| Conversión | `ConvertBitmap32to{1,4,8,16,24,32}_{NoDither,Dither,Diffuse,Floyd,GreyDither}`, `Floyd_LR/RL`, `ConvertLineRGBto16_5xx` | Matriz completa de conversores. |
| Geometría auxiliar | `GenBevelFaces::BevelPath`, `GenPathContour::ContourPath`, `ClipPathToPath`, `GenClipPath`, `GenClipLines`, `Intersect` | Contorno de camino y booleanas. |
| Sombras | `GenerateWallShadow`, `GenerateFloorShadow`, `ContourBitmap`, `Blur`, `Sharpen`, `BlurBitmapSetup`, `BlurTransparencyBitmapSetup`, `CalcFilter`, `CalcFilterD` | Efectos vivos. |

---

# 2. Tarea B — Semántica de render a replicar

## 2.1 Coordenadas, matriz y precisión

El documento vive en **millipuntos** (`MILLIPOINT` = 1/1000 pt; `MILLIPOINTS_PER_INCH = 72000`),
enteros con signo de 32 bits. Un camino es un par de arrays estilo Win32: `POINT[]` (x,y `INT32`)
y `BYTE[]` de verbos `PT_MOVETO=6`, `PT_LINETO=2`, `PT_BEZIERTO=4`, `| PT_CLOSEFIGURE=1`
(`gconsts.h:106`).

La matriz documento→dispositivo es:

```c
struct GMATRIX { INT32 AX, AY, BX, BY; XLONG CX, CY; };   // gconsts.h:310  (XLONG = int64)
const INT32 FX = 14;                                      // gconsts.h:354
```

Construcción real (`wxOil/grndrgn.cpp:5297-5340`):

```c
const XLONG Mult = (INT32)(dPixelsPerInch * (1 << FX) + 0.5);
gmat.AX = ((XLONG)abcd[0].GetRawLong() * Mult) / 72000;   // abcd = FIXED16 (16.16)
...
gmat.CX = -((XLONG)xdisp * (XLONG)(1 << (FX + 16)));      // traslación: 30 bits fraccionarios
```

→ **a,b,c,d en punto fijo 2.30** (16 bits de `FIXED16` + `FX`=14) y **e,f en 64 bits con 30 bits
fraccionarios**. A 96 dpi, `AX = 2^30 · 96/72000 = 1 431 655`. El comentario del código
(`grndrgn.cpp:5290`) advierte de que CDraw **trunca** en vez de redondear, y la app compensa
sumando medio píxel. Esta sutileza es la causa de desalineamientos de 1 píxel al comparar
salidas: hay que decidir conscientemente la regla de redondeo del motor nuevo.

Consecuencias para el diseño: la escena se define en enteros de 32 bits (millipuntos), y el
espacio de dispositivo usa 2.30 fijo. **Un `f32` (24 bits de mantisa) no cubre el rango de
millipuntos de un documento grande** (±2^31 mp ≈ ±29 800 m): ver §3.7.

## 2.2 Rasterizado: modelo de barrido

Deducido de los símbolos internos y de `Kernel/gclips.h` (que documenta las estructuras
compartidas `Edge`, `Curve`, `Strip`):

1. **Aplanado**: las curvas se subdividen recursivamente hasta la tolerancia `Flatness`
   (en unidades del documento). La app fija `Flatness = (72000/dpi)/2 / escala`, y **la divide
   entre 5 cuando hay antialias** (`grndrgn.cpp:5626-5631`): medio píxel sin AA, una décima con.
2. **Tabla de aristas**: `GenEdgeTable` produce aristas con `XS,YS,XE,YE`, pendiente `DX` en
   `double`, flags y, para curvas, puntero a la `Curve` y parámetros `t` de inicio/fin
   (`Kernel/gclips.h:127`). Es decir: **las curvas no siempre se aplanan; el clipper mantiene la
   curva paramétrica** para poder devolver Béziers exactas en las booleanas.
3. **Barrido**: `SortYEdges` → `ProcessEdges` → `GenFilledStrips`/`GenLineStrips`. Las líneas se
   emiten a un *blitter* (`SolidBlitLine32`, `LinearGradBlitLine`, `TransparencyBlitLine32`, …)
   seleccionado por el estilo activo (`InitialiseStyle`, `STYLE`, `BlitSetup`).
4. **Regla de relleno**: `FillWindingPolygon` (nonzero) y `FillAlternatePolygon` (par/impar), más
   las variantes *inversas* (rellenan el complemento). El parámetro `Winding` de
   `GDraw_FillPath` es `bit0` = nonzero, `bit1` = inverso.

## 2.3 Antialiasing

CDraw hace **supermuestreo por sub-scanlines con cobertura horizontal cuantizada**, no
*analytic coverage*:

| Rutina | Sub-scanlines (V) | Sub-posiciones horizontales (H) | Peso por sub-scanline |
|---|---|---|---|
| `AddScanline5x5` | 5 | 5 | 5 |
| `AddScanline17x5` | 5 | 17 | 51 (= 255/5) |
| `AddScanline11x11` | 11 | 11 | 23 (≈ 255/11) |
| `AddScanline12x11` | 11 | 12 | 24 |

(los pesos son constantes `…::Word` en `.rodata` de `libCDraw_la-GScanAA.o`; la LUT principal de
ese objeto empieza en `0x33,0x30,0x2d,…` = 51, 48, 45 … es decir 17 pasos de 3 = 51, confirmando
el esquema 17×5).

* `GDraw_SetAntialiasFlag(bool)` activa/desactiva AA.
* `GDraw_SetAntialiasQualityFlag(bool)` alterna entre el modo normal (5 líneas) y el de alta
  calidad (11 líneas); la app lo expone como preferencia `HighQualityAA`
  (`grndrgn.cpp:313, 5622`).
* Las regiones también tienen AA: `REGION::Type` 0 = sin AA, 1 = 5 líneas, 2 = 9 líneas
  (`gconsts.h:296`), con `ClipLineAA5/11/12/17` para el recorte con cobertura.
* La cobertura final es un byte 0..255, aplicado como alfa en el *merge*.

**Implicación para Rust:** 5×17 = 85 niveles efectivos (normal) y 11×12 = 132 (alta calidad).
Un rasterizador moderno de cobertura analítica (vello, tiny-skia) es **estrictamente mejor**, de
modo que el objetivo no es replicar el artefacto sino igualar o superar la calidad; hay que
aceptar que la salida no será bit-exacta (ver §3.8, estrategia de validación).

## 2.4 Niveles de calidad de render

`Kernel/quality.h:100` define un escalar 0..110 (`QUALITY_GUIDELAYER = -1`, defecto 100) del que
se derivan cinco ejes (`Kernel/quality.cpp:157-255`):

| Eje | Umbrales |
|---|---|
| Línea | `>50` FullLine · `>30` ThinLine · resto BlackLine |
| Relleno | `>=60` Graduated · `>30` Solid · `>10` Bitmaps · resto NoFill |
| Blend | `<=20` solo extremos · resto completo |
| **Antialias** | `<=100` NO · `>100` sí |
| Transparencia | siempre «NoTransparency» (eje no usado) |

El AA solo se activa por encima de 100, y la calidad por defecto es exactamente 100: en Xara LX
**el antialias está apagado por defecto** y las herramientas suben la calidad a 110.

## 2.5 Bandas, capturas y caché de bitmap

**Bandas** (`wxOil/grnddib.cpp:409-600`): si la memoria «afinada» disponible no da para el bitmap
completo, la región se parte en bandas horizontales de `MaxScanLines = RAM / ScanlineSize`
(mínimo útil 16 líneas); `SetFirstBand`/`GetNextBand` recorren las bandas re-renderizando la
escena con distinto `CurrentClipRect`.

**Capturas** (`Kernel/capturemanager.h:131-215`): el mecanismo de *render a bitmap intermedio*
para grupos transparentes, efectos vivos, sombras y biseles. Tipos: `ctNESTABLE`, `ctREUSE`,
`ctRESTART`; 13 flags, entre ellos `cfGRABRENDERED` (reutiliza el bitmap padre),
`cfDIRECT`/`cfALLOWDIRECT` (un nodo hijo **entrega** su bitmap ya calculado — esto es la caché
de render por nodo), `cfFULLCOVERAGE`, `cfLOCKEDTRANSPARENT`, `cfQUALITYNORMAL`.

**BBox de cambios**: CDraw acumula el rectángulo tocado (§1.3.l) y la app lo usa para blitear
solo lo modificado y para recortar la captura al área real.

**Scroll**: `GDraw_ScrollBitmap(ctx, dx, dy)` mueve el contenido del bitmap destino para
reutilizar los píxeles válidos al hacer *pan*, dibujando solo la franja nueva.

## 2.6 Gradientes: matemática exacta

### 2.6.1 Formas

| `GradEnum` | Puntos | Geometría |
|---|---|---|
| `GRAD_LINEAR` (0) | A(inicio), B, C | Parámetro `s = ((P−A)·u)/‖u‖²` con `u = C−A`; B es el eje perpendicular que permite gradientes «sesgados». La app reordena: `A=Start, B=EndPoint2, C=End` (`grndrgn.cpp:2524`). |
| `GRAD_RADIAL` (1) | A(centro), B, C | Elíptico: dos radios independientes ⇒ `s = ‖M⁻¹(P−A)‖`. |
| `GRAD_CONICAL` (2) | A(centro), B, C | Angular: `s = atan2` normalizado. La app refleja B respecto de A (`grndrgn.cpp:2532`). Implementado con `CalcConicalGrad`, `aArcTable` y blitters por cuadrante (`ConicalGradBlitLine{Left,PosMid,NegMid,Start,End}`). |
| `GRAD_SQUARE` (3) | A, B, C | Diamante: `s = max(‖u‖∞, ‖v‖∞)` (métrica L∞ en el sistema A,B,C). |
| `GRAD_3COLOUR` | A, B, D + 3 colores | Malla baricéntrica. |
| `GRAD_4COLOUR` | A, B, C, D + 4 colores | Malla bilineal. |

Cada forma tiene además variante **repetida** (`RLinearGrad…`, `RRadialGrad…`, `RSquareGrad…`:
`s` en módulo, con o sin espejo) y variante **en perspectiva** (`…Grad4…`: se pasa un cuarto
punto D y la interpolación es proyectiva, validada por `MouldPerspective::WillBeValid`).

### 2.6.2 La tabla de gradiente

```c
struct GraduationTable   { DWORD Length; COLORREF Start, End; DitherBlock Table[0x100]; }; // gconsts.h:248
struct GraduationTable32 { DWORD Length; COLORREF Start, End; COLORREF   Table[0x100]; };
struct TransparentGradTable { BYTE Table[0x100]; };                                        // gconsts.h:262
struct DitherBlock { DWORD Data[4]; };
```

`Length` puede ser 256 o **2048** (tablas «largas», `LargeGradTables`, `Kernel/gradtbl.cpp:262`)
para evitar bandas en gradientes grandes. Para destinos de <32 bpp cada entrada no es un color
sino un `DitherBlock` de 4 DWORD = patrón de dither 4×4 precalculado para ese color.

La rampa se construye **en la aplicación** (`Kernel/gradtbl.cpp`), no en CDraw, salvo la
interpolación simple:

* `EFFECT_RGB` → interpolación lineal en RGB.
* `EFFECT_HSV_SHORT` / `EFFECT_HSV_LONG` → interpolación en HSV por el camino corto o largo del
  matiz (`gradtype.h:105`); la app fuerza RGB si hay colores de tinta plana
  (`gradtbl.cpp:453`).
* Rampas multiparada (`ColourRamp`) → se rellenan tramo a tramo con `AddToGraduationTable`.
* Con separación/corrección de color activas, **cada** entrada pasa por el `ColourContext`
  (`gradtbl.cpp:549`) — lento pero exacto.

### 2.6.3 Perfil (bias/gain) — fórmulas exactas

`CProfileBiasGain` (`Kernel/biasgain.cpp`) es el «perfil» de Xara (curva de distribución del
gradiente, del blend, del contorno y de la sombra). Es el clásico bias/gain de Schlick:

Parámetros de usuario `B, G ∈ [−1, +1]`, convertidos a `b, g ∈ (0,1)` con
(`biasgain.cpp:516`, `ε = 1e-5`):

```
b = (B + 1)·(0.5 − ε) + ε
```

Núcleo (`biasgain.cpp:592` y `616`):

```
bias(b, x) = x·b / ( (1 − 2b)·(1 − x) + b )

           ⎧ x·g / (C + g)                 si x < 0.5
gain(g, x) =⎨                                              con C = (1 − 2g)·(1 − 2x)
           ⎩ (C − x·g) / (C − g)           si x ≥ 0.5

profile(x) = gain( g, bias(b, x) )          (biasgain.cpp:572)
```

Con `B = G = 0` ⇒ `b = g = 0.5` ⇒ identidad (el código incluso cortocircuita ese caso,
`biasgain.cpp:341`). Aplicación a la rampa (`gradtbl.cpp:1206`, transparencia):

```c
BiasGain.SetIntervals(0, Length);
for (i = 0; i < Length; i++) {
    f = BiasGain.MapInterval(i) / Length;      // == profile(i/Length)
    Table[i] = Start·(1−f) + End·f;
}
```

Interpolación de transparencia sin perfil, en punto fijo (`gradtbl.cpp:1562`):

```c
t   = (start << 22) + 0x00200000;              // +0.5 para redondear
inc = ((end − start) << 22) / (endIdx − startIdx);
Table[i] = (t >> 22) & 0xFF;  t += inc;
```

## 2.7 Transparencia y modos de mezcla

### 2.7.1 Enumeración completa

`TransparencyEnum` (`gconsts.h:193`) — 36 valores. Las tres variantes de cada familia indican
cómo llega el valor de transparencia: **genérica**, **plana** (`T_FLAT_*`, valor constante) y
**graduada** (`T_GRAD_*`, del blitter de gradiente/bitmap):

| # | Valor | Familia | Nombre Xara (UI) |
|---|---|---|---|
| 0 | `T_NONE` | — | Ninguna |
| 1,4,7 | `T_REFLECTIVE`, `T_FLAT_REFLECTIVE`, `T_GRAD_REFLECTIVE` | Mix | **Mix** (alfa normal) |
| 2,5,8 | `T_SUBTRACTIVE`, `T_FLAT_SUBTRACTIVE`, `T_GRAD_SUBTRACTIVE` | Multiply | **Stained Glass** |
| 3,6,9 | `T_ADDITIVE`, `T_FLAT_ADDITIVE`, `T_GRAD_ADDITIVE` | Screen | **Bleach** |
| 10,11,12 | `T_SPECIAL_1/2/3` | — | Reservados (el 12 usa `SetTransparencyLookupTable`) |
| 13,14,15 | `T_CONTRAST`, `T_FLAT_CONTRAST`, `T_GRAD_CONTRAST` | Contrast | **Contrast** |
| 16,17,18 | `T_SATURATION`, … | Saturation | **Saturation** |
| 19,20,21 | `T_DARKEN`, … | Darken | **Darken** |
| 22,23,24 | `T_LIGHTEN`, … | Lighten | **Lighten** |
| 25,26,27 | `T_BRIGHTNESS`, … | Brightness | **Brightness** |
| 28,29,30 | `T_LUMINOSITY`, … | Luminosity | **Luminosity** |
| 31,32,33 | `T_HUE`, … | Hue | **Hue** |
| 34,35,36 | `T_BEVEL`, `T_FLAT_BEVEL`, `T_GRAD_BEVEL` | Bevel | Interno (iluminación de bisel) |

El modelo de documento usa su propio `TranspType` (`Kernel/fillval.h:144`) con
`TT_Mix=1, TT_StainGlass=2, TT_Bleach=3` y luego los valores de GDraw a partir de 13; la
traducción está en `GRenderRegion::MapTranspTypeToGDraw` (`wxOil/grndrgn.cpp:8844`):

```cpp
if (ttype <= TT_Bleach)  return ttype − TT_Mix   + (bGraduated ? T_GRAD_REFLECTIVE : T_FLAT_REFLECTIVE);
if (ttype >= TT_CONTRAST && ttype <= TT_BEVEL)
                         return ttype − TT_CONTRAST + (bGraduated ? T_GRAD_CONTRAST : T_FLAT_CONTRAST);
```

Convenio de valores: **0 = opaco, 255 = totalmente transparente** (inverso del alfa habitual).

### 2.7.2 Fórmulas de mezcla (derivadas del desensamblado)

Notación: `s` = color fuente (BGR), `d` = color destino, `t` = transparencia 0..255,
`Wr,Wg,Wb` = tablas de peso de luminancia del contexto (`GColour_SetGreyConversionValues`,
en `ctx+0x610f0/0x614f0/0x618f0`, 256 × u32 cada una), y

```
Y(c)      = (Wr[c.r] + Wg[c.g] + Wb[c.b]) >> 24        // luminancia 0..255
Mul[a][b] = a·b / 255                                   // GDraw::apMulTable
IMul[a][b]= (255−a)·b / 255                             // GDraw::apIMulTable
```

Cada familia es una función `(Y(s), t) → nivel L`, seguida de una LUT de 256 entradas aplicada a
cada canal del destino. Verificado instrucción a instrucción:

**Darken** (`libCDraw_la-GScanT-D.o`, `GDraw::CalcTransparencyD`):

```
L        = Y(s) + IMul[Y(s)][t]  =  Y(s) + (255 − Y(s))·t/255
out_c    = Mul[L][d_c]           =  d_c · L / 255
```
→ multiplica el destino por la luminancia de la fuente, desvanecida hacia 1 con `t`.

**Lighten** (`GScanT-L.o`, `CalcTransparencyL`):

```
L        = IMul[t][Y(s)]         =  Y(s)·(255 − t)/255
out_c    = L + IMul[L][d_c]      =  L + d_c·(255 − L)/255        // screen
```

**Brightness** (`GScanT-B.o`, `CalcTransparencyB`): igual que Lighten/Darken pero el nivel se
calcula sobre el valor **con signo** `v = (ΣW)>>23` (≈ `2·Y − 255`), eligiendo la rama
`screen` si `v ≥ 0` y `multiply` si `v < 0`; `L = IMul[t][|v|]`.

**Contrast** (`GScanT-C.o`): `v = ±(2Y−255)`, `L = t + IMul[t][v]`, y la salida se obtiene de la
LUT estática **`aContrastTable`** indexada por `L` y por el destino **centrado** (`d_c − 128`),
sumando 128 al resultado: es una curva S paramétrica, no una fórmula cerrada.

**Saturation** (`GScanT-Sn.o`, `CalcTransparencySn`):

```
k      = t·aSaturationTable2[Y(s)] + aSaturationTable1[Y(s)]      // u32
g      = Y(d)                                                      // luminancia del destino
out_c  = aChopTable[ g + (((d_c − g)·k) >> 21) ]                   // aChopTable clampa −1024..1023 → 0..255
```
→ escala la crominancia del destino alrededor de su propia luminancia.

**Luminosity** (`GScanT-Lu.o`, `CalcTransparencyLu`):

```
target = IMul[t][Y(s)]
m      = max(d.b, d.g, d.r)
si m == 0:  out = (target, target, target)
si no:      k     = target·Recip[m] + Off[t]          // Recip[] ≈ 2^24/m, Off[] tabla por t
            out_c = (d_c·k + 0x800000) >> 24
```
→ reescala el destino para que su canal máximo alcance la luminancia de la fuente
(preserva matiz y saturación).

**Hue** (`GScanT-H.o`, `CalcTransparencyH`): convierte fuente y destino con
`GDraw::ConvertRGBtoHSV`, interpola el canal H con `Mul`/`IMul` según `t`, y reconvierte con
`ConvertHSVtoRGB` (mismas funciones que exporta la API pública
`GColour_ConvertHSVtoRGB/RGBtoHSV`).

**Mix / Stained Glass / Bleach** (`GScanT-R.o`, `GScanT-S.o`, `GScanT-A.o`): usan solo
`apMulTable`/`apIMulTable` y `aChopTable`/`aDiv3Table`; corresponden a

```
Mix            out_c = IMul[t][s_c] + Mul[t][d_c]        = s_c(255−t)/255 + d_c·t/255
Stained Glass  out_c = d_c · (255 − IMul[t][255 − s_c]) / 255      ≈ multiply desvanecido por t
Bleach         out_c = 255 − (255 − d_c)·(255 − IMul[t][s_c])/255  ≈ screen desvanecido por t
```

(en la variante `_T` con canal alfa de destino, Stained Glass añade un término con
`aDiv3Table[r+g+b]`, la media de canales, para conservar el alfa acumulado).

**Bevel** (`GScanT-Bvl.o`): el «color fuente» es en realidad un **índice de bisel** de 8 bits
producido por GDraw2; `SetBevelContrast/Lightness/Darkness` parametrizan dos LUT
(`.rodata` + `.rodata+0x100`) que se combinan con `Mul`/`IMul` para aclarar u oscurecer el
destino.

> **Conclusión de diseño**: las 12 familias son «*(escalar derivado de la fuente) → LUT de tono
> aplicada al destino*». Eso se traduce **exactamente** a una textura LUT 2D de 256×256 R8 por
> familia en la GPU (§3.4), sin ramas ni funciones trascendentes.

### 2.7.3 Transparencia graduada y por bitmap

Las mismas 12 familias se combinan con las cinco formas de gradiente
(`GColour_SetTransparentGraduation`, `…4`, `…3Way…`, `…4Way…`) y con bitmaps
(`GColour_SetTransparentTilePattern`), que aportan el valor `t` por píxel. La tabla es
`TransparentGradTable` (256 bytes) y admite el mismo perfil bias/gain (`gradtbl.cpp:1206`) y
rampas multiparada (`TransparencyRamp`, `gradtbl.cpp:1251`).

## 2.8 Rellenos de bitmap y fractales

**Bitmap/tile** (`wxOil/grndrgn.cpp:3453-3800`): se define un paralelogramo A,B,C (o un
cuadrilátero A..D en perspectiva) y el tipo de repetición viene de `FillMappingAttribute::Repeat`
(`RepeatType`, `Kernel/fillval.h:132`): 1 simple, 2 repetida, 3 repetida invertida (espejo),
4 repetida de alta calidad. Se combinan con:

* `SetTileSmoothingFlag` (suavizado bilineal cuando hay rotación/escala) y
  `SetTileFilteringFlag` (filtro de máxima calidad al imprimir/exportar), decididos por
  `NeedToSmooth()` y por si se está imprimiendo (`grndrgn.cpp:3400-3433`).
* *Contone* (`GBitmap_SetContone(style, rgbStart, rgbEnd)`, `grndrgn.cpp:3725`): remapea un
  bitmap a una rampa de dos colores, con el `FillEffect` (RGB / HSV corto / HSV largo) como
  estilo.
* Perfil bias/gain sobre el canal 3 (transparencia) y `SetOutputRange(3, start, end)` para
  limitar el rango de transparencia (`grndrgn.cpp:3730-3732`, `4313-4315`).

**Fractal (plasma/nubes)**: `FILLSHAPE_CLOUDS = 9`, `FILLSHAPE_PLASMA = 10`
(`Kernel/fillval.h:129`). **No lo genera CDraw**: `Kernel/fracfill.cpp` implementa un
*midpoint displacement* (diamante-cuadrado) recursivo `PlasmaFractalFill::SubDivide`
(`fracfill.cpp:290`) con parámetros `Seed`, `Tileable`, `Squash`, `Graininess` (0..32) y
`Gravity` (0..255):

```c
// fracfill.cpp:213-262  (Adjust)
potential = ((aGraininess · (potential>>17)) >> RecursionLevel)     // ruido
          − (aGravity >> (RecursionLevel·2));                        // atracción al centro
```

El resultado es un bitmap que se pinta con la maquinaria de tile normal. El ruido Perlin
(`Kernel/noise1.cpp`, `noisef.cpp`) también es código de la aplicación.

## 2.9 Efectos vivos: qué parte hacía CDraw

| Efecto | Geometría / control (Kernel) | Parte que hace CDraw |
|---|---|---|
| **Sombra** (`nodeshad.cpp`, `bshadow.cpp`) | Elige radio, color, perfil bias/gain (`nodeshad.cpp:410`); construye la máscara de convolución circular y la tabla de normalización | `GenerateWallShadow` (desenfoque), `GenerateFloorShadow` (sombra proyectada con inclinación) |
| **Feather** (`fthrconv.cpp:313`) | Igual que la sombra pero aplicado al alfa del objeto | `GenerateWallShadow` |
| **Bisel** (`beveler.cpp`) | `GenBevelFaces::BevelPath` genera las caras (triángulos/trapecios) con sus normales 2D; `CBeveler` decide estilo, ángulo de luz y *tilt* | `GDraw2_SetDIBitmap` + `GDraw2_FillTriangle/FillTrapezium` rasterizan el mapa de iluminación al canal alto de un bitmap 32 bpp; luego `T_BEVEL` lo aplica |
| **Contorno** (`nodecntr.cpp`, `paths.cpp:5725`) | `Path::GetContourForStep` con perfil | `GDraw_StrokePathToPath` (offset del camino) y `GenPathContour::ContourPath` |
| **Booleanas** (`pbecomea.cpp`) | Elige el estilo de recorte | `ClipPathToPath` |
| **Mezcla/blend** (`nodebldr.cpp`) | Interpolación de caminos y atributos con perfil | nada (solo dibuja los pasos) |

El desenfoque real (`CBitmapShadow::Blur8BppBitmap`, `Kernel/bshadow.cpp:458-536`) es una
**convolución con disco**: para cada fila `r` del disco de radio `fBlur` se calculan los offsets
izquierdo/derecho (`aLeft/aRight`) y superior/inferior (`aLow/aHigh`), y se pasa una tabla de
normalización de `uArea·255` entradas comprimida a ≤ 0x800 (`TABLE_SIZE`) mediante un
desplazamiento `uShift`. CDraw ejecuta la suma acumulada. Radio máximo 100 px
(`MAX_SHADOW_BLUR`), diámetro mínimo `sqrt(0.5)`.

## 2.10 Gestión de color

* **Espacios** (`Kernel/colmodel.h:199`): `CIET` (XYZ+T), `RGBT`, `CMYK`, `HSVT`, `GREYT`,
  `WEBRGBT`, además de colores indexados y tintas planas.
* **CMYK**: conversión ingenua en el kernel (`colcontx.cpp:1755`, `2224`):
  `R = 1 − C`, `G = 1 − M`, `B = 1 − Y`, aplicando después `K`; el **UCR/GCR real** es
  responsabilidad de CDraw a través de `GColour_SetSeparationTables(cyan, magenta, yellow,
  black, UCR, blackGeneration)`, seis LUT que la app instala al imprimir/separar y retira
  después (`grndrgn.cpp:3843, 7591`). CDraw tiene además rutas de píxel CMYK nativas
  (`PixelCMYK`, `FPixelCMYK_C_F`).
* **Gamma**: `ColourContextRGBT(View*, double GammaValue)` (`colcontx.cpp:1160`) en el kernel, y
  `GBitmap_SetGamma` / `GBitmap_SetPostGamma` en CDraw (pre y post proceso de bitmaps).
* **Luminancia**: `GColour_SetGreyConversionValues(R,G,B)` define los pesos usados por *todos*
  los modos de mezcla y por la conversión a gris. El desensamblado de
  `GDraw::SetGreyConversionValues` (`libCDraw_la-GColour.o+0x1790`) muestra que construye tres
  tablas acumulativas de 256 × u32 en `ctx+0x610f0/0x614f0/0x618f0`, con
  `W_c[i] = i · (c · 0x010101) / (R+G+B)`, normalizadas para que la suma con los tres canales a
  255 dé `0xFF000000`. **Xara LX nunca llama a esta función**, así que rigen los pesos por
  defecto internos de CDraw: hay que recuperarlos empíricamente (renderizando un Darken sobre
  un degradado conocido) antes de dar por buenas las fórmulas de §2.7.2. La hipótesis de
  partida es ITU-R BT.601 (0,299 / 0,587 / 0,114).
* **Corrección/separación de bitmaps**: `GColour_SetBitmapConversionTable(pcBGR)`.
* Toda la composición interna de CDraw es en **sRGB no lineal de 8 bits por canal** (las LUT son
  de 256 entradas): replicar los blends en espacio lineal daría resultados distintos.

---

# 3. Tarea C — Plan de reimplementación en Rust

## 3.1 Requisitos derivados de las Tareas A y B

| # | Requisito | Origen |
|---|---|---|
| R1 | Relleno de caminos con Bézier cúbicas, reglas nonzero y evenodd (+ inversa) | §2.2 |
| R2 | AA de calidad ≥ 85 niveles, conmutable (modo borrador sin AA) | §2.3 |
| R3 | Trazo con caps/joins/miter/dash **y** `stroke→path` exacto (para contorno y booleanas) | §1.3.c |
| R4 | 5 formas de gradiente × (simple/repetido/espejo) × (afín/perspectiva) × (2 colores con rampa arbitraria / malla 3 / malla 4) | §2.6 |
| R5 | 12 familias de mezcla con semántica *LUT sobre destino*, no las de PDF/CSS | §2.7 |
| R6 | Transparencia graduada y por bitmap con las mismas 12 familias | §2.7.3 |
| R7 | Rellenos de bitmap con paralelogramo/perspectiva, 4 modos de repetición, suavizado y filtrado HQ, contone | §2.8 |
| R8 | Desenfoque de disco (sombra/feather), contorno de máscara, mapa de bisel iluminado | §2.9 |
| R9 | Booleanas de caminos con salida en Béziers | §1.4 |
| R10 | Render a bitmap intermedio anidado (capturas) con reutilización | §2.5 |
| R11 | BBox de cambios y scroll incremental | §2.5 |
| R12 | Coordenadas de documento en enteros de 32 bits (millipuntos) sin pérdida | §2.1 |
| R13 | Salida a 1/4/8/16/24/32 bpp, CMYK, dither (8 estilos), separación con UCR/GCR | §1.3.e,j |
| R14 | Determinismo total en la ruta de exportación (PNG/PDF) | nuevo, Xarast |

## 3.2 Comparativa del ecosistema Rust (estado a 2026-09)

| Criterio | **vello** (GPU, wgpu) | **vello_cpu** (sparse strips CPU) | **tiny-skia** | **lyon + wgpu propio** | **resvg** | **femtovg** | **skia-safe** |
|---|---|---|---|---|---|---|---|
| Naturaleza | Rasterizador *compute-centric*, escena declarativa | Mismo modelo, backend CPU SIMD | Port de un subconjunto de Skia (CPU) | Teselador a triángulos + pipeline propio | Renderizador SVG sobre tiny-skia | Canvas estilo NanoVG (GPU) | *Bindings* a Skia C++ |
| Calidad AA | Cobertura analítica exacta (conflation-free por *strips*) | Idéntica a vello | *Supersampling*/analítica estilo Skia, muy buena | MSAA (4–8×) o AA analítico manual | La de tiny-skia | Stencil+cover, mediocre en bordes finos | Excelente |
| Blend modes | Los 12+16 de PDF/CSS, en *shader* de composición | Ídem | `BlendMode` de Skia (16) | Los que programes | Solo los de SVG | Pocos | Todos los de Skia |
| Blends **personalizados** | Requiere tocar el shader de composición (fork) o post-pase | Fork del compositor Rust (más fácil) | Requiere post-pase CPU | **Trivial** (es tu shader) | No | No | Muy difícil (C++) |
| Gradientes con rampa arbitraria | Sí (`ColorStops`, hasta N paradas) | Sí | Sí (`GradientStop`) | A medida (textura LUT) | Sí | Limitado | Sí |
| Gradiente cónico / diamante | Sweep sí; diamante no | Ídem | Sweep no (solo lineal/radial/two-point) | A medida | No | No | Sweep sí |
| Gradiente en **perspectiva** | Vía transformada proyectiva no soportada de serie | No | No (solo afín) | **Sí** (interpolación proyectiva en el shader) | No | No | Parcial |
| Rendimiento (escena grande) | Muy alto en GPU discreta/integrada | 2.º puesto en *benchmarks* 2025, por delante de Skia y Cairo en muchos casos | Correcto; peor en ARM | Depende del teselado; malo con muchos caminos pequeños | = tiny-skia | Alto en 2D simple | Muy alto |
| Madurez | 0.5+/sparse-strips en evolución; API aún cambiante | **alpha** (declarado así por los autores) | Estable, mantenimiento tranquilo | Estable (lyon 1.x) | Estable | Estable | Estable (pero es C++) |
| Licencia | Apache-2.0 / MIT | Apache-2.0 / MIT | BSD-3 (herencia Skia) | MIT/Apache | MPL-2.0 | MIT | BSD-3 + build de Skia |
| Linux/Wayland | Sí (wgpu: Vulkan/GL) | N/A (CPU) | N/A | Sí | N/A | Sí (GL/GLES) | Sí |
| Windows / macOS | Sí (DX12 / Metal) | Sí | Sí | Sí | Sí | Sí | Sí |
| Tamaño binario | ~ 3–6 MB con wgpu | ~ 0,6 MB | **~ 0,4 MB** | ~ 3–6 MB | ~ 1,5 MB | ~ 2 MB | **~ 30–60 MB** + toolchain C++ |
| Coste de compilación | Medio | Bajo | Bajo | Medio | Bajo | Bajo | **Muy alto** (C++, ~1 h) |
| Riesgo para Xarast | API en movimiento | alpha | Falta cónico/perspectiva/blends | Todo a mano | No es un motor, es un consumidor | Insuficiente | Contradice el objetivo «100 % Rust», licencia y peso |

Notas de contexto: Vello CPU y Vello GPU comparten la arquitectura *sparse strips* e
infraestructura común en `vello_common`, y Vello CPU está declarado **alpha**; en los
*benchmarks* de 2025 `vello_cpu` queda segundo tras Blend2D, por delante de Skia y Cairo en
muchos casos. `tiny-skia` es explícitamente «un subconjunto pequeño de Skia» con toda la
lógica portada de Skia.

## 3.3 Recomendación firme: motor propio `xarast-render` con doble backend

```
                 ┌──────────────────────────── xarast-render (crate propio) ──────────┐
   Documento ──▶ │ Escena (retenida)  →  Display list  →  Planificador de tiles/bandas │
   (modelo)      │            ↑ caché por nodo          ↓                              │
                 │            └──────────────  Backend GPU  ──┬── Backend CPU          │
                 └───────────────────────────────────────────┼────────────────────────┘
                                                             │
        vello (sparse strips, wgpu)  ◀── geometría + paints ─┤
        + pases de composición WGSL propios (blends Xara)    │
                                                             └─▶ vello_cpu (mismos strips)
                                                                 + compositor CPU propio
```

**Decisiones y porqué:**

1. **No adoptar ninguna librería como «el motor»**, sino como *rasterizador de cobertura*.
   La razón es R5+R4: ninguna trae los blends de Xara ni el gradiente cónico/diamante en
   perspectiva. Lo que sí es reutilizable —y lo caro de escribir— es el rasterizador de
   cobertura AA de alta calidad. Ahí `vello`/`vello_cpu` gana: su representación intermedia
   (*sparse strips*: tiras de píxeles con cobertura por columna) es **exactamente** el mismo
   concepto que los `Strip` de CDraw (`Kernel/gclips.h:163`) y se presta a que el compositor
   sea nuestro.
2. **Un solo frontend, dos backends.** La escena y la display list son propias y neutras; GPU y
   CPU consumen la misma estructura. Esto da: (a) *fallback* sin GPU (servidores de
   render, CI, Wayland con drivers pobres), (b) **determinismo** para exportar (R14: siempre
   backend CPU), (c) tests de regresión comparando ambos backends píxel a píxel.
3. **`lyon` solo para `stroke→path` y utilidades geométricas** (R3), no para teselar a
   triángulos: `lyon_algorithms` + `kurbo` cubren offsetting, aplanado adaptativo y longitudes
   de arco. Las booleanas (R9) van con `kurbo` + un clipper propio o
   `path-bool`-equivalente; el rasterizador no interviene.
4. **`tiny-skia` queda como referencia de validación**, no como dependencia de producción: es
   útil para generar imágenes de control en los tests porque su AA es conocido.
5. **`skia-safe` descartado**: 30–60 MB, toolchain C++ y dependencia de terceros — repetiría el
   problema de `libCDraw.a` que estamos resolviendo.

### Contrato de fachada (sustituye 1:1 a `GDrawContext`)

```rust
pub trait Rasterizer {
    fn begin_frame(&mut self, target: &mut Surface, clip: DeviceRect);
    fn fill_path(&mut self, path: &PathRef, rule: FillRule, paint: &Paint, xf: &Transform2D);
    fn stroke_path(&mut self, path: &PathRef, style: &StrokeStyle, paint: &Paint, xf: &Transform2D);
    fn draw_image(&mut self, img: &ImageRef, mapping: &Mapping, paint: &ImagePaint);
    fn push_layer(&mut self, kind: LayerKind, bounds: DeviceRect) -> LayerId; // ≈ Capture
    fn pop_layer(&mut self, id: LayerId, blend: Blend, opacity: Transparency);
    fn end_frame(&mut self) -> DirtyRect;                                     // ≈ GetChangedBBox
}
```

## 3.4 Cómo cubrir los blend modes exóticos de Xara

La Tarea B demuestra que **las 12 familias son la misma máquina**:

```
nivel L = f_familia( Y(src), t )            // escalar 0..255
out_c   = LUT_familia[L][dst_c]             // tabla 256×256
```

Eso es literalmente una textura `R8Unorm` de 256×256 por familia (64 KiB), o un `TEXTURE_2D_ARRAY`
de 12 capas (768 KiB) residente en GPU. Dos familias necesitan algo más: **Saturation** y
**Luminosity** operan sobre la luminancia del **destino** (no solo por canal), y **Hue** requiere
RGB↔HSV; se implementan analíticamente en el shader.

Generación de las tablas (CPU, una vez, en `build.rs` o al iniciar):

```rust
/// Genera la LUT 2D de una familia de mezcla al estilo CDraw.
/// Eje X = componente del destino (0..=255); eje Y = nivel L (0..=255).
pub fn build_blend_lut(family: BlendFamily) -> [[u8; 256]; 256] {
    let mut lut = [[0u8; 256]; 256];
    for l in 0..=255usize {
        for d in 0..=255usize {
            lut[l][d] = match family {
                // out = d · L / 255
                BlendFamily::Darken   => mul(l as u8, d as u8),
                // out = L + d·(255−L)/255
                BlendFamily::Lighten  => l as u8 + imul(l as u8, d as u8),
                BlendFamily::Contrast => contrast_curve(l as u8, d as u8),
                _ => d as u8,
            };
        }
    }
    lut
}

#[inline] fn mul (a: u8, b: u8) -> u8 { ((a as u16 * b as u16 + 127) / 255) as u8 }
#[inline] fn imul(a: u8, b: u8) -> u8 { (((255 - a) as u16 * b as u16 + 127) / 255) as u8 }
```

Y el pase de composición en WGSL:

```wgsl
// Composición de una capa Xarast sobre el destino.
// src_col : color de la fuente ya resuelto (relleno/gradiente/bitmap), sRGB no lineal.
// t       : transparencia Xara, 0 = opaco … 1 = transparente.
// cov     : cobertura del antialias 0..1.
// blend_luts: texture_2d_array<f32>, capa = familia; x = destino, y = nivel.

const W: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);   // GColour_SetGreyConversionValues

fn xara_level(family: u32, y_src: f32, t: f32) -> f32 {
    switch family {
        case FAM_DARKEN:  { return y_src + (1.0 - y_src) * t; }
        case FAM_LIGHTEN: { return y_src * (1.0 - t); }
        case FAM_BRIGHT:  { return abs(2.0 * y_src - 1.0) * (1.0 - t); }
        default:          { return t; }
    }
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let dst = textureLoad(dst_tex, vec2<i32>(in.pos.xy), 0);
    let src = in.src_col;
    let t   = in.transparency;
    let y   = dot(src.rgb, W);

    var out: vec3<f32>;
    switch in.family {
        // Familias «LUT»: un solo muestreo por canal.
        case FAM_DARKEN, FAM_LIGHTEN, FAM_BRIGHT, FAM_CONTRAST, FAM_BEVEL: {
            let l = xara_level(in.family, y, t);
            out = vec3<f32>(
                lut(in.family, dst.r, l),
                lut(in.family, dst.g, l),
                lut(in.family, dst.b, l));
        }
        // Saturation: escala la crominancia del destino alrededor de su luminancia.
        case FAM_SATURATION: {
            let g = dot(dst.rgb, W);
            let k = sat_gain(y, t);                    // ≈ (t·Sat2[y] + Sat1[y]) / 2^21
            out = clamp(vec3<f32>(g) + (dst.rgb - vec3<f32>(g)) * k, vec3(0.0), vec3(1.0));
        }
        // Luminosity: reescala el destino hasta la luminancia de la fuente.
        case FAM_LUMINOSITY: {
            let target = y * (1.0 - t);
            let m = max(dst.r, max(dst.g, dst.b));
            out = select(dst.rgb * (target / m), vec3<f32>(target), m <= 0.0);
        }
        case FAM_HUE: {
            var h = rgb_to_hsv(dst.rgb);
            h.x = mix(h.x, rgb_to_hsv(src.rgb).x, 1.0 - t);
            out = hsv_to_rgb(h);
        }
        // Mix / Stained glass / Bleach: cerradas.
        case FAM_MIX:     { out = mix(dst.rgb, src.rgb, 1.0 - t); }
        case FAM_STAINED: { out = dst.rgb * (vec3(1.0) - (vec3(1.0) - src.rgb) * (1.0 - t)); }
        case FAM_BLEACH:  { out = vec3(1.0) - (vec3(1.0) - dst.rgb) * (vec3(1.0) - src.rgb * (1.0 - t)); }
        default:          { out = src.rgb; }
    }
    return vec4<f32>(mix(dst.rgb, out, cov(in)), max(dst.a, in.alpha));
}

fn lut(family: u32, d: f32, l: f32) -> f32 {
    return textureSampleLevel(blend_luts, lut_sampler,
                              vec2<f32>(d, l), i32(family), 0.0).r;
}
```

**Puntos críticos:**

* **Espacio de color**: estas fórmulas son en **sRGB codificado**, igual que CDraw. El *target*
  de composición debe ser `Rgba8Unorm` (no `…Srgb`) para que la GPU **no** linealice, o bien
  hay que deshacer la conversión explícitamente. Mezclar en lineal cambia visiblemente
  Stained Glass y Bleach.
* **Estas mezclas leen el destino** ⇒ no se pueden expresar con el *blend state* fijo de
  la GPU. Se resuelven con `textureLoad` sobre un *attachment* de entrada
  (subpass input / `TEXTURE_BINDING` con copia por tile) y **ordenación estricta** de la
  display list por capa. Como Xara ya renderiza a bitmaps intermedios (capturas, §2.5),
  esto encaja: cada capa con blend exótico se resuelve con un *ping-pong* de dos texturas de
  tile.
* **Fallback CPU**: el mismo código en Rust escalar/SIMD sobre las mismas LUT, compartido con
  el backend GPU por construcción (una sola fuente de verdad para las tablas).

## 3.5 Diseño del pipeline

```
 (1) Escena retenida            (2) Display list           (3) Planificación
 ─────────────────────         ──────────────────         ──────────────────
 Árbol de nodos del doc   ──▶  Lista plana ordenada  ──▶  Partición en tiles 256×256
 + atributos heredados         de DrawCmd (opaco:          (GPU) o bandas de N líneas
 + bbox por nodo               geometría+paint+blend       (CPU / impresión, ≈ grnddib)
 + hash de contenido           +layer push/pop)            + culling por bbox y por clip

 (4) Rasterización            (5) Composición            (6) Presentación
 ──────────────────           ─────────────────          ─────────────────
 vello / vello_cpu       ──▶  Pases propios:       ──▶   Blit del dirty rect a la
 → sparse strips               • paint (gradiente/           superficie de ventana
   (cobertura por span)          bitmap/contone)             (wgpu surface o wl_buffer)
                                • blend Xara (LUT)
                                • capas (capturas)
```

**(1) Escena retenida y caché por nodo.** Cada nodo lleva un `ContentHash` (geometría +
atributos + matriz relativa) y un `RenderCacheSlot`:

```rust
pub struct NodeCache {
    key:      CacheKey,        // hash contenido + escala cuantizada + calidad
    bounds:   DeviceRect,
    surface:  Option<CachedSurface>,   // textura GPU o buffer CPU premultiplicado
    cost:     u32,             // µs medidos al generarlo → política de expulsión
    epoch:    u64,
}
```

Política: se cachean **solo** los nodos caros — grupos transparentes, efectos vivos (sombra,
bisel, feather, contorno), rellenos fractales y grupos con >N primitivas —, que es exactamente
el criterio de `cfALLOWDIRECT`/`cfDIRECT` de las capturas de Xara (§2.5). Expulsión LRU
ponderada por `cost`. La escala se cuantiza a potencias de √2 para no invalidar la caché en cada
paso del zoom: se reescala la textura cacheada mientras el zoom esté dentro del ±41 % y se
regenera en segundo plano.

**(2) Display list.** Inmutable por fotograma, con *layer push/pop* explícito. Los comandos con
blend exótico marcan `needs_dst_read = true`, lo que obliga al planificador a cerrar el tile y
hacer ping-pong.

**(3) Tiles y bandas.** GPU: tiles de 256×256 con lista de comandos por tile (*binning* por
bbox) — esto permite que las lecturas de destino sean locales y quepan en memoria de grupo.
CPU: bandas horizontales con el mismo criterio de memoria que `GRenderDIB::SetFirstBand`
(`grnddib.cpp:451-500`), reutilizando su heurística (mínimo 16 líneas).

**(4) Render incremental para pan/zoom.**

* *Pan*: se reproyecta el contenido válido (equivalente a `GDraw_ScrollBitmap`) y solo se
  rasterizan las bandas nuevas. En GPU es un blit de textura a sí misma con desplazamiento.
* *Zoom*: se presenta inmediatamente la versión cacheada escalada (respuesta <16 ms) y se
  encola el re-render exacto.
* *Edición*: se recalcula el `dirty rect` como unión del bbox anterior y el nuevo del nodo
  modificado (equivalente a `GetChangedBBox`), y solo se rerasterizan los tiles intersectados,
  reutilizando la caché de los nodos no tocados.

**(5) Render de alta calidad diferido.** Dos niveles de calidad, como en Xara pero explícitos:

| Nivel | Cuándo | Diferencias |
|---|---|---|
| `Draft` | durante arrastre/zoom/scroll | AA activado pero *flatness* ×5, bitmaps con muestreo *nearest*, sombras con radio reducido y sin recalcular, gradientes con LUT de 256 |
| `Final` | 120 ms sin interacción (temporizador) o al exportar | *flatness* completo, filtrado HQ de bitmaps, LUT de 2048 entradas, efectos recalculados |

El paso a `Final` se hace por tiles, priorizando los visibles, y es cancelable.

## 3.6 Modelo de paints y capas

```rust
pub enum Paint {
    Solid(ColorU8),
    Gradient {
        shape:   GradShape,          // Linear | Radial | Conical | Diamond | Mesh3 | Mesh4
        mapping: GradMapping,        // Affine{a,b,c} | Perspective{a,b,c,d}
        repeat:  Repeat,             // Simple | Repeat | Mirror | RepeatHQ
        ramp:    RampId,             // LUT de 256 o 2048, generada con el perfil bias/gain
    },
    Image {
        image:   ImageId,
        mapping: GradMapping,        // el mismo paralelogramo/cuadrilátero que los gradientes
        repeat:  Repeat,
        filter:  Filter,             // Nearest | Bilinear | HighQuality
        contone: Option<(ColorU8, ColorU8, EffectSpace)>,
        adjust:  BitmapAdjust,       // brillo, contraste, gamma, saturación, bias/gain, rangos
    },
    Fractal(FractalParams),          // se materializa a Image; el generador es CPU
}

pub struct Transparency {
    pub family: BlendFamily,         // 12 familias (§2.7)
    pub source: TranspSource,        // Flat(u8) | Gradient{…} | Image{…}
}
```

La rampa se genera con el perfil exacto de §2.6.3:

```rust
pub fn build_ramp(stops: &[Stop], profile: Profile, space: RampSpace, len: usize) -> Vec<ColorU8> {
    (0..len).map(|i| {
        let x = i as f64 / (len - 1) as f64;
        let f = profile.map(x);                      // gain(g, bias(b, x))
        sample_stops(stops, f, space)                // Rgb | HsvShort | HsvLong
    }).collect()
}

impl Profile {
    /// Schlick bias/gain, idéntico a CProfileBiasGain (Kernel/biasgain.cpp:572-640).
    pub fn map(&self, x: f64) -> f64 {
        if self.bias == 0.0 && self.gain == 0.0 { return x; }
        let b = (self.bias + 1.0) * (0.5 - 1e-5) + 1e-5;
        let g = (self.gain + 1.0) * (0.5 - 1e-5) + 1e-5;
        let biased = x * b / ((1.0 - 2.0 * b) * (1.0 - x) + b);
        let c = (1.0 - 2.0 * g) * (1.0 - 2.0 * biased);
        if biased < 0.5 { biased * g / (c + g) } else { (c - biased * g) / (c - g) }
    }
}
```

## 3.7 Estrategia de precisión

| Etapa | Tipo | Justificación |
|---|---|---|
| Modelo de documento / ficheros | **`i32` millipuntos** | Compatibilidad con `.xar`/`.xarast` y con `DocCoord`; rango ±2 147 483 mp ≈ ±29,8 m; sin errores de acumulación al editar (R12). |
| Álgebra de transformaciones | **`f64`** | Una matriz 3×3 en `f64` tiene 52 bits de mantisa: cubre millipuntos exactos (31 bits) con margen para escala/rotación. `kurbo` ya es `f64`. Es lo que hace el propio Xara con `double DX` en las aristas (`Kernel/gclips.h:130`). |
| Geometría intermedia (aplanado, offset, booleanas) | **`f64` (`kurbo::BezPath`)** | Las booleanas y el `stroke→path` son numéricamente delicados; CDraw también usa `double`. |
| Coordenadas de dispositivo entregadas al rasterizador | **`f32` en espacio de tile** | Tras restar el origen del tile, el rango es ≤ 4096 px ⇒ `f32` da precisión sub-µpíxel. Es el requisito de `vello`. **La conversión `f64 → f32` se hace siempre relativa al tile, nunca en coordenadas absolutas de documento.** |
| Cobertura AA | `u8` (0..255) en CPU, `f32` en GPU | Compatibilidad con el modelo de 8 bits y con las LUT de mezcla. |
| Color de composición | **`u8` sRGB no lineal** | Obligado por §2.10: las LUT de mezcla son de 256 entradas y las fórmulas están definidas en sRGB codificado. |
| Color en efectos que sí lo requieren (blur, escalado HQ) | `u16` o `f32` lineal internamente, con vuelta a `u8` | Evita *banding* en desenfoques grandes sin cambiar la semántica de la mezcla. |

**Regla de oro del port:** *nunca* meter millipuntos absolutos en un `f32`. Un documento de 5 m
de ancho tiene 3,6·10⁸ mp; `f32` tiene 2⁻²³ de resolución relativa ⇒ error de ~43 mp ≈ 0,04 pt,
visible a zoom alto. Toda conversión a `f32` va precedida de una traslación al origen del tile.

También hay que fijar explícitamente la **regla de redondeo**: CDraw trunca y la app le suma
medio píxel (`grndrgn.cpp:5290-5300`). En Xarast se documentará *round-half-away-from-zero* en
la conversión a dispositivo y se eliminará la compensación heredada.

## 3.8 Validación: cómo saber que el motor nuevo es «el mismo»

1. **Corpus de referencia**: renderizar el conjunto de `.xar` de `/home/user/xara-xtreme/testfiles`
   y `Designs/` con Xara LX original (usando `libCDraw.a` en una VM x86-64) a PNG 32 bpp,
   varios zooms, con y sin AA.
2. **Métrica**: no exigir igualdad bit a bit (el AA es distinto por construcción, §2.3). Umbral
   propuesto: ΔE₀₀ medio < 1,0 y percentil 99 < 3,0; en zonas planas (interior de rellenos)
   **sí** exigir igualdad exacta, porque ahí solo interviene la fórmula de mezcla y la rampa.
3. **Tests unitarios de mezcla**: para cada una de las 12 familias, tabla de 256×256×(t) generada
   por el motor nuevo comparada con la extraída del binario original mediante un arnés que
   llame directamente a `GDraw::CalcTransparencyX` (es posible: son funciones exportadas,
   firma `(GDraw*, BGR*, BGR, u8)`).
4. **Paridad GPU/CPU**: los dos backends deben coincidir bit a bit en la ruta `Final`; cualquier
   divergencia es un bug (se logra manteniendo las LUT y el orden de operaciones idénticos).

## 3.9 Plan por fases

| Fase | Entregable | Dependencias | Riesgo |
|---|---|---|---|
| **M0** | Fachada `Rasterizer` + backend CPU mínimo (relleno nonzero/evenodd, trazo, color sólido) sobre `vello_cpu`; visor de `.xarast` | `kurbo`, `vello_cpu` | Bajo |
| **M1** | Gradientes completos (5 formas × repetición × perspectiva) + rampas con perfil bias/gain | M0 | Medio (cónico y perspectiva son código propio) |
| **M2** | Las 12 familias de mezcla con LUT + capas/capturas anidadas | M1, extracción de LUT del binario | **Alto** (semántica exacta) |
| **M3** | Rellenos de bitmap: paralelogramo/perspectiva, repetición, filtrado HQ, contone, ajustes | M1 | Medio |
| **M4** | Backend GPU (`vello` + pases WGSL propios) con paridad frente a CPU | M2, M3 | Alto (lectura de destino, ordenación) |
| **M5** | Tiles/bandas, dirty rect, scroll y caché por nodo; Draft/Final | M4 | Medio |
| **M6** | Efectos vivos: blur de disco, contorno de máscara, bisel iluminado, feather | M2 | Medio |
| **M7** | Booleanas de caminos y `stroke→path` (sustituyen `ClipPathToPath`/`GDraw_StrokePathToPath`) | M0 | **Alto** (robustez numérica) |
| **M8** | Salida: dither (8 estilos), reducción de profundidad, CMYK con UCR/GCR, separaciones | M2 | Medio (solo relevante para impresión) |

Nota sobre M8: los 8 estilos de dither y las conversiones a 1/4/8/16 bpp (§1.6) son hoy poco
relevantes para pantallas modernas. **Propuesta**: implementar solo `DITHER_NONE` y
Floyd-Steinberg en M8 y relegar el resto a un módulo opcional de exportación heredada.

---

# 4. Apéndices

## 4.1 Apéndice A — Las 126 funciones exportadas por `libCDraw.a`

```
GBitmap_  (15): Blur, PlotTile, PlotTile4, SetBias, SetBrightness, SetContone, SetContrast,
                SetGain, SetGamma, SetInputRange, SetMaxFilterSize, SetOutputRange,
                SetPostGamma, SetSaturation, Sharpen

GColour_  (50): AddToGraduationTable, BuildGraduationTable, BuildGraduationTable32,
                BuildTransparencyTable, ConvertBitmap, ConvertHSVtoRGB, ConvertRGBtoHSV,
                GetGraduationTableLength, GetGraduationTableSize,
                GetLongGraduationTableLength, GetLongGraduationTableSize,
                GetLongTransparentGraduationTableLength, GetLongTransparentGraduationTableSize,
                GetTransparentGraduationTableLength, GetTransparentGraduationTableSize,
                InitialiseWithPalette, ReturnBrush, ReturnBrushRGB, SelectPalette,
                Set3WayGraduation, Set3WayGraduation4, Set4WayGraduation, Set4WayGraduation4,
                SetBitmapConversionTable, SetColour, SetConversionPalette, SetDitherStyle,
                SetGourand, SetGraduation, SetGraduation4, SetGreyConversionValues,
                SetHalftoneOrigin, SetInvert, SetSeparationTables, SetSolidColour,
                SetTileFilteringFlag, SetTilePattern, SetTilePattern4, SetTileSmoothingFlag,
                SetTransparency, SetTransparencyLookupTable, SetTransparent3WayGraduation,
                SetTransparent3WayGraduation4, SetTransparent4WayGraduation,
                SetTransparent4WayGraduation4, SetTransparentGraduation,
                SetTransparentGraduation4, SetTransparentTilePattern,
                SetTransparentTilePattern4, SetWordColour

GDraw_    (55): CalcBBox, CalcSimpleBBox, CalcStrokeBBox, Clean, ClearChangedBBox,
                ClearLastError, ClipRectangle, ClipRegion, ComparePalettes, ContextLength,
                CopyContext, DeviceClipRectangle, FillPath, FillPoint, FillRectangle,
                FillRegion, GetChangedBBox, GetClipRegion, GetDeviceClipRectangle,
                GetLastError, GetLastErrorText, GetMaxBitmapDepth, GetMaxBitmapWidth,
                GetStatistics, GetSvnVersion, GetVersion, HintPath, Initialise, IsOverlap,
                IsStrokeOverlap, MakeRegion, MakeUnclippedRegion, MaxScale, ScrollBitmap,
                SetAntialiasFlag, SetAntialiasQualityFlag, SetBevelContrast, SetBevelDarkness,
                SetBevelLightness, SetChangedBBox, SetDIBitmap, SetDashAdjustmentFlag,
                SetFlatness, SetHintingFlag, SetInvertedDIBitmap, SetMatrix,
                SetMemoryHandlers, SetMiterLimit, SetStackLimit, SetStackSize, StrokePath,
                StrokePathToPath, Terminate, TransformPath, TranslateBevelValue

GDraw2_   (6):  ClearLastError, FillTrapezium, FillTriangle, GetLastError, GetVersion,
                SetDIBitmap
```

Más cuatro entradas C++ usadas por la aplicación y declaradas fuera de `gdraw.h`:
`ClipPathToPath`, `GenerateWallShadow`, `GenerateFloorShadow`, `ContourBitmap` (§1.4).

## 4.2 Apéndice B — Mapa de sustitución CDraw → módulos de Xarast

| Área CDraw | Módulo Rust propuesto | Base externa |
|---|---|---|
| Contexto, estado, errores | `xarast-render::context` | — |
| Matriz, transformación, bbox | `xarast-render::geom` | `kurbo` |
| Aplanado, tabla de aristas, AA | `xarast-render::raster` | `vello` / `vello_cpu` |
| Trazo, caps/joins/dash, `StrokePathToPath` | `xarast-geom::stroke` | `kurbo`, `lyon_algorithms` |
| `ClipPathToPath` (booleanas) | `xarast-geom::boolops` | propio sobre `kurbo` |
| Gradientes (5 formas, perspectiva, mallas) | `xarast-render::paint::gradient` | propio (WGSL + CPU) |
| Tablas de gradiente, perfil bias/gain | `xarast-render::ramp` | propio |
| 12 familias de mezcla | `xarast-render::blend` (+ LUT generadas) | propio (WGSL + SIMD) |
| Rellenos de bitmap, contone, ajustes | `xarast-render::paint::image` | `fast_image_resize` |
| Fractales plasma/nubes | `xarast-fx::fractal` | propio (port de `fracfill.cpp`) |
| Blur de disco, contorno, feather | `xarast-fx::blur`, `::contour` | propio (SAT/SIMD + compute) |
| Bisel (`GDraw2_*`) | `xarast-fx::bevel` | propio |
| Capturas / capas | `xarast-render::layer` | — |
| Bandas, tiles, dirty rect, scroll | `xarast-render::schedule` | — |
| Conversión de profundidad, dither, CMYK, separación | `xarast-color` | `qcms` o perfiles ICC propios |
| Regiones / clipping | `xarast-render::clip` | `vello` clip stack + propio |

## 4.3 Apéndice C — Reproducción del análisis

```bash
XARA=/home/user/xara-xtreme
nm -g --defined-only $XARA/libs/x86_64/libCDraw.a | grep ' T ' | sort > syms.txt      # 926
awk '{print $3}' syms.txt | grep -E '^(GDraw_|GDraw2_|GColour_|GBitmap_)' | sort -u   # 126
awk '{print $3}' syms.txt | c++filt | grep '^GDraw::' | sed 's/GDraw:://;s/(.*//' | sort -u  # 640
mkdir objs && cd objs && ar x $XARA/libs/x86_64/libCDraw.a
objdump -d -r --no-show-raw-insn -C libCDraw_la-GScanT-D.o   # Darken   (§2.7.2)
objdump -d -r --no-show-raw-insn -C libCDraw_la-GScanT-L.o   # Lighten
objdump -d -r --no-show-raw-insn -C libCDraw_la-GScanT-Lu.o  # Luminosity
objdump -d -r --no-show-raw-insn -C libCDraw_la-GScanT-Sn.o  # Saturation
objdump -s -j .rodata -C libCDraw_la-GScanAA.o               # pesos de antialias (§2.3)
```

Objetos del archivo, por familia funcional: `GDraw/GMain/GContext/GStyle/GError/GMemory/GMaths`
(núcleo), `GPath/GStroke/cstroke` (caminos), `GScan*/GScanAA` (barrido y AA),
`GScanL/L4/R/R4/RR/S/S4/Sq/Sq4/C/C4/X3/X34/X4/X44` (blitters de gradiente),
`GScanT-{A,B,Bvl,C,D,H,L,Lu,R,S,Sn}` (mezclas), `GGrad`, `GColour/GConvert/GTable*`
(color y LUT), `GBevel/GTableBevel` (bisel), `GRegion/gclip*` (regiones y booleanas),
`bshadow2` (sombras), `GSprite/GScroll`.

---

## Fuentes externas consultadas

- [Releases · linebender/vello](https://github.com/linebender/vello/releases)
- [Vello CPU (README, sparse_strips)](https://skia.googlesource.com/external/github.com/linebender/vello/+/refs/heads/main/sparse_strips/vello_cpu/README.md)
- [linebender/vello](https://github.com/linebender/vello)
- [vello_cpu — crates.io](https://crates.io/crates/vello_cpu)
- [Linebender in July 2025](https://linebender.org/blog/tmil-19/)
- [linebender/tiny-skia](https://github.com/linebender/tiny-skia)
- [tiny-skia — lib.rs](https://lib.rs/crates/tiny-skia)
- [Vello — lib.rs](https://lib.rs/crates/vello)
