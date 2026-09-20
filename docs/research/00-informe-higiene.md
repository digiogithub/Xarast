# Informe de higiene de sala limpia — `docs/research/`

> **Fecha:** 2026-09-20
> **Alcance:** los seis documentos de investigación `01`–`06` de `docs/research/`.
> **Motivo:** Xarast se publicará bajo licencia **permisiva (MIT OR Apache-2.0)**.
> El original, Xara Xtreme / Xara LX, es **GPL-2.0-only**. La licencia permisiva
> solo es legítima si mantenemos disciplina de **sala limpia**: podemos documentar
> *hechos* (formatos de fichero, nombres y tipos de campo, semántica, números de
> tag, algoritmos descritos en prosa) pero **no reproducir *expresión*** del
> código fuente del original.

## Criterio de clasificación aplicado

| Clase | Qué es | Acción |
|---|---|---|
| **(A)** | Copia literal o casi literal del original: cuerpos de función, definiciones de clase con su sintaxis C++, listas de declaraciones de miembros, tablas de constantes tal cual. | **Reescrito** como prosa en español, tabla markdown o pseudocódigo neutro, **conservando** la referencia `fichero:línea`. |
| **(B)** | Hecho de interfaz o dato del formato: números de tag, layouts binarios, listas de campos con tipo y significado, constantes del formato de fichero. | **Conservado**, pero presentado como tabla markdown o pseudocódigo, nunca como C++ copiado. Comentarios reescritos en español propio. |
| **(C)** | Rust / WGSL / pseudocódigo propuesto por nosotros, XML de `.xarast`, TOML/YAML de nuestro build, ejemplos de otros proyectos, volcados de nuestras propias herramientas. | **Conservado tal cual**. |

## Resumen por documento

| Documento | Bloques revisados | Reescritos (A) | Conservados como hecho (B) | Propios (C) |
|---|---:|---:|---:|---:|
| `01-formato-xar.md` | 36 | 6 | 20 | 10 |
| `02-modelo-documento.md` | 122 (116 + 6 indentados) | 70 | 13 | 39 |
| `03-motor-render.md` | 31 | 10 | 13 | 8 |
| `04-inventario-funcionalidad.md` | 0 | 0 | 0 | 0 |
| `05-stack-tecnologico.md` | 15 | 0 | 1 | 14 |
| `06-formato-xarast.md` | 52 | 0 | 2 | 50 |
| **Total** | **256** | **86** | **49** | **121** |

Se añadió además el **aviso de sala limpia** justo bajo el título H1 de los seis
documentos, adaptado en una frase al contenido de cada uno.

Verificación posterior: **no queda ningún bloque con etiqueta `cpp`, `c` o `c++`**
en `docs/research/`, ni ningún bloque sin etiqueta que supere el detector de
sintaxis C++ del original (`BOOL`, `INT32`, `UINT32`, `TCHAR`, `DocRect`,
`DocCoord`, `virtual`, `public:`, `CC_DECLARE_*`, `#define`, `#include`, `::`,
`new X(`…). Todos los comentarios que quedan dentro de bloques conservados están
en español y son redacción propia.

---

## Detalle de los bloques reescritos

### `01-formato-xar.md` — 6 bloques

| Bloque (§) | Contenido original | Justificación de la reescritura |
|---|---|---|
| §1.2 firma | dos `#define` de `CXF_IDWORD1/2` (`cxfdefs.h:109-110`) | Los valores son hecho del formato, pero la forma `#define` es expresión: pasado a tabla constante/valor/origen. |
| §1.6 tipos de fichero | tres `#define EXPORT_FILETYPE_*` (`camfiltr.h:143-145`) | Ídem: los literales `CXN`/`CXW`/`CXM` son hecho de interoperabilidad; la tabla los conserva sin la sintaxis C. |
| §2.3 records desconocidos | bucle de descarte de payload (`cxfile.cpp:1997-2065`) | Cuerpo de función: sustituido por pseudocódigo neutro de dos líneas que dice exactamente lo mismo. |
| §3.2 versión de compresión | dos sentencias de cálculo y escritura de la versión zlib (`cxfile.cpp:705-742`) | Cuerpo de función: sustituido por la fórmula en prosa (`major*100 + minor`, emitido como `u32`). |
| §3.3 parámetros de zlib | llamadas literales a `deflateInit2`/`inflateInit2` (`zstream.cpp:490-545`, `:760-770`) | Aunque son llamadas a una API de terceros, se copiaban textualmente del original: convertidas a tabla de parámetros con el valor y su efecto, que además es más útil para reimplementar. |
| §7.4 verbos de camino | cinco `const BYTE PT_*` (`gconsts.h:108-112`) | Constantes del formato (hecho) reexpresadas como tabla nombre/valor/papel, con la nota de que los bits 3-7 no se usan. |

### `02-modelo-documento.md` — 70 bloques

Los 65 bloques etiquetados `cpp` eran, en su práctica totalidad, **listas de
declaraciones de miembros y de métodos virtuales extraídas de las cabeceras del
original** (más cuatro cuerpos de función y tres bloques indentados dentro de
listas). Todos se han convertido en **tablas markdown** de tipo
«campo / tipo / significado / `fichero:línea`» o en prosa paso a paso. No se ha
perdido ningún nombre, ningún tipo ni ninguna referencia cruzada; en varios casos
se ha ganado información (rangos, unidades, notas de invariante).

| §  | Bloque | Justificación |
|---|---|---|
| 1.2 | ~60 predicados virtuales de tipo rápido (`node.h:455-520`) | Declaraciones de clase → tabla predicado/pregunta/referencia. |
| 1.3 | `NodeFlags` + campos de enlace de `Node` (`node.h:758-784`) | Definición de estructura con campos de bits → dos tablas (banderas y campos). |
| 2.x | Estado paramétrico de `NodeRegularShape` (`nodershp.h:299-322`) | Lista de miembros → tabla, marcando qué es caché. |
| 3.1 | Cuerpo de `Document::InitTree` (`document.cpp:415`) | **Cuerpo de función**: reescrito como procedimiento numerado de 5 pasos con los `:línea` intactos. |
| 3.2 | Campos de `NodeDocument` (`nodedoc.h:164-168`) | Declaraciones → tabla. |
| 3.2 | Campos de `Spread` (`spread.h:329-383`) | Declaraciones → tabla. |
| 3.2 | Estado de `Layer` (`layer.h:341-380`) | Declaraciones → dos tablas (estado de capa y estado de frame GIF). |
| 3.2 | `switch` de `Layer::EnableLayerCacheing` (`layer.cpp:481`) | **Cuerpo de función**: reescrito como tabla de tres niveles de cacheo + una frase sobre el corte del recorrido. |
| 4.1 | Patrón `Value` / `GetAttributeValue()` (`fillattr2.h:132-138`) | Declaraciones → una frase que describe el patrón. |
| 4.1 | Interfaz de `AttributeValue` (`attrval.h:134-173`) | Declaraciones virtuales → tabla método/papel. |
| 4.3 | Campos de estado de `RenderRegion` (`rndrgn.h:904-935`) | Declaraciones → tabla. |
| 4.3 | Campos de `AttributeEntry` (`attrmgr.h:127`) | Declaraciones → tabla. |
| 4.3 | Cinco macros `RR_*` (`rndrgn.h:971-1008`) | **Macros del original**: sustituidas por una descripción del mecanismo (indexar el array, castear, devolver el campo) + tabla macro/slot/valor. |
| 4.3 | Interfaz de `RenderStack` (`rndstack.h:128-132`) | Declaraciones → tabla operación/efecto. |
| 4.4 | Registro estático de atributos por defecto (`attrmgr.h:251-276`) | Declaraciones → tabla. |
| 4.4 | Bucle de `Document::InitDefaultAttributeNodes` (`document.cpp:520`) | **Cuerpo de función**: reescrito en prosa. |
| 4.5 | `AttributeGroup` (`attrmgr.h:161`) + `NUM_ATTR_GROUPS` | Definición de clase → tabla + una frase. |
| 5.x | Accesores de puntos/colores/transparencias de rellenos graduados (`fillval.h`) | Declaraciones → tabla de familias de accesores. |
| 5.x | `enum RepeatType` | Enumerado → tabla valor/nombre/significado (los valores son hecho del formato). |
| 5.x | `enum TranspType` (`fillval.h:144`) | Ídem, con la nota de qué valores no son legales en el documento y remisión al §2.7 de `03`. |
| 5.x | `RampItem` / `ColRampItem` / `TranspRampItem` / `FillRamp` (`fillramp.h:134-243`) | Definiciones de clase → tabla clase/referencia/contenido. |
| 5.x | Accesores de perfil `CProfileBiasGain` (`fillval.h:328-332`) | Declaraciones → una frase. |
| 5.x | Interfaz de `CProfileBiasGain` (`biasgain.h:170-243`) | Declaraciones → tabla operación/papel. |
| 5.8 | Campos de `FractalFillAttribute` (`fillval.h:913`) | Declaraciones → tabla con rangos y unidades. |
| 5.8 | Campos de `NoiseFillAttribute` | Ídem. |
| 5.8 | Generación y caché de fractal/ruido (`fillval.h:274-344`) | Declaraciones → tabla operación/papel. |
| 5.x | Interfaz de deformación y mezcla de rellenos (`fillval.h:288-337`) | Declaraciones → tabla. |
| 5.x | Campos de `DocColour` (`doccolor.h`) | Declaraciones → tabla. |
| 6.x | Banderas de `ObjChangeFlags` (`objchge.h`) | Campos de bits → tabla bandera/qué anuncia. |
| 6.x | `enum OpPermissionState` | Enumerado → frase con los tres valores y dónde se codifica. |
| 6.x | Condición de `NodeCompound::OnChildChange` (`nodecomp.cpp:272-320`) | **Cuerpo de función**: reescrito como la regla que implementa, en una frase. |
| 6.x | Bucle de `Application::RegenerateNodesInList` (`app.cpp:1830-1880`) | **Cuerpo de función**: pseudocódigo neutro + explicación de por qué invalida la caja dos veces. |
| 6.5 | «Tight groups» (`group.h:204-209`, bloque indentado) | Declaraciones → prosa, conservando la fórmula 72 000/ppp. |
| 6.x | Cajas de `NodeRenderableBounded` (`node.h:1425-1440`) | Declaraciones → tabla. |
| 6.x | Interfaz de cacheo en bitmap (`node.h:1448-1450` y alrededores) | Declaraciones → tabla + frase sobre los tres interruptores estáticos. |
| 6.x | `CBitmapCacheKey`, `CCachedBitmap`, `CBitmapCache` (`bitmapcachekey.h:104`, `bitmapcache.h:114-161`) | Definiciones de clase → tres tablas + descripción de la política de tamaño máximo. |
| 6.6 | Estado de `NodeBlend` (`nodeblnd.h:129`) | Declaraciones → tabla. |
| 6.6 | `BlendPath` y `BlendRef` (`nodebldr.h:140`, `:287`) | Definiciones de clase → tabla + prosa. |
| 6.7 | Interfaz de `MouldGeometry` (`moldshap.h`) | Declaraciones virtuales → tabla operación/papel. |
| 6.8 | Estado de contorno (`ncntrcnt.h:153` y derivados) | Declaraciones → tabla, destacando que el signo de la anchura codifica dentro/fuera. |
| 6.9 | Estado de sombra (`nodecont.h:215` y derivados) | Declaraciones → tabla, agrupando los campos de «resolución con la que se generó». |
| 6.10 | Estado de bisel (`nbevcont.h:125` y derivados) | Declaraciones → tabla. |
| 6.11 | Estado de ClipView (`ndclpcnt.h:146`) | Declaraciones → tabla. |
| 6.12 | `NodeEffect` / `NodeBitmapEffect` (`nodepostpro.h:130`, `nodeliveeffect.h:163`) | Árbol con declaraciones de miembros → tabla clase/base/estado. |
| 6.13 | `CanBecomeA` / `DoBecomeA` (`node.h:656-657`) | Declaraciones → prosa, explicando qué lleva el parámetro. |
| 7.2 | Estado de `TextStory` (`nodetxts.h:456-460` y ss.) | Declaraciones → tabla, con las unidades explícitas. |
| 7.3 | `TextStoryInfo` | Declaraciones → tabla. |
| 7.3 | `TextLineInfo` (`nodetxtl.h:253`) | Declaraciones → tabla, conservando la nota de que la suma de avances excluye el último tracking. |
| 7.3 | Parámetros de posicionado de caracteres | Declaraciones → tabla, marcando cuáles son constantes de entrada. |
| 7.3 | Atributos cacheados de `TextLine` | Declaraciones → tabla, conservando la nota sobre por qué se cachean. |
| 7.4 | `VisibleTextNode` (`nodetext.h:166-201`) | Declaraciones → tabla + prosa de los predicados. |
| 7.4 | Métricas de `AbstractTextChar` (`nodetext.h:271-278`) | Declaraciones → tabla. |
| 7.5 | Primitivas abortadas de `FormatRegion` (`nodetxtl.h:130`) | **Cuerpo con `ERROR3`**: reescrito como la frase «la región de formato solo mide, nunca pinta». |
| 7.5 | Accesores de `FormatRegion` (`nodetxtl.h:173` y ss.) | Declaraciones con cuerpo en línea → tabla consulta/devuelve/origen. |
| 8.1 | Campos de `KernelBitmap` (`bitmap.h:627-634`) | Declaraciones → tabla. |
| 8.2 | Campos de `BitmapInfo` (`bitmpinf.h`) | Declaraciones → tabla. |
| 8.3 | JPEG sin recomprimir: `WritePalette` / `Convert24To8` (`bitmap.h:514-516`, bloque indentado) | Declaraciones → prosa. |
| 8.3 | `KernelBitmapRef` (`bitmap.h:664-673`) | Declaraciones → tabla + descripción del conteo de referencias por presencia en el árbol. |
| 8.5 | Campos de `NodeBitmap` (`nodebmp.h:180`) | Declaraciones → tabla. |
| 9.1 | `RangeControl` (`range.h:219`) + `Range` / `SelRange` | Campos de bits y clases → tabla + frase. |
| 9.1 | Caché de `SelRange` | Declaraciones → tabla, conservando el aviso de que el contador es inválido si el rango no está cacheado. |
| 9.2 | `Operation` (`ops.h:323-404`) | Definición de clase → tabla agrupada por familias de operación. |
| 9.2 | Banderas de `UndoableOperation` | Campos de bits → tabla. |
| 9.3 | `Action` (`ops.h:559-608`) | Definición de clase → tabla miembro/papel/referencia. |
| 9.3 | `ActionList` (`ops.h:196-208`) | Declaraciones → prosa. |
| 9.4 | Campos de `OperationHistory` (`ophist.h:219-232`) | Declaraciones → tabla, conservando que el presupuesto es **en bytes**. |
| 9.4 | Interfaz de `OperationHistory` | Declaraciones → tabla agrupada. |
| 9.5 | Enumerados y operaciones de copia (`node.h:245-256`, `:418-787`) | Enumerados y declaraciones → tabla + prosa. |
| 9.x | `NodeHidden` (`node.h:1475-1481`, bloque indentado) | Definición de clase → prosa. |
| 4.2 | Pseudocódigo del recorrido de render (`rndrgn.cpp:7076-7130`) | Bloque sin etiqueta que mezclaba pseudocódigo con sintaxis C++ (`pNode->…`, `;`): neutralizado a pseudocódigo puro en español. |

### `03-motor-render.md` — 10 bloques

| § | Contenido original | Justificación |
|---|---|---|
| 1.2 | `struct GCONTEXT` (`gconsts.h:237`) | Definición de estructura **de una cabecera propietaria** (CDraw, no GPL): sustituida por su descripción (palabra de validación + bloque opaco) y el valor centinela. |
| 1.x (h) | Firma de `GColour_SetTilePattern` (`gdraw.h:351`) | Firma literal de cabecera propietaria → tabla parámetro/tipo/papel, que además explica para qué sirve cada tabla de traducción. |
| 1.x (j) | Tres firmas de `GDraw2_*` (`gdraw2.h`) | Ídem → tabla función/entradas/papel. |
| 2.1 | `struct GMATRIX` + `const INT32 FX` (`gconsts.h:310`, `:354`) | Definición de estructura → tabla campo/ancho/significado. |
| 2.1 | Construcción de la matriz (`grndrgn.cpp:5297-5340`) | **Cuerpo de función**: reescrito como procedimiento de 3 pasos, conservando el punto fijo 2.30, los 30 bits fraccionarios y el cambio de signo de la traslación. |
| 2.6.2 | Cuatro `struct` de tablas de gradiente (`gconsts.h:248`, `:262`) | Definiciones de estructura → tabla estructura/campos/uso. |
| 2.6.x | Bucle de construcción de la rampa con perfil (`gradtbl.cpp:1206`) | **Cuerpo de función**: pseudocódigo neutro. |
| 2.6.x | Interpolación en punto fijo (`gradtbl.cpp:1562`) | **Cuerpo de función**: pseudocódigo neutro, explicando que el sumando es el redondeo a +0,5. |
| 2.7 | `GRenderRegion::MapTranspTypeToGDraw` (`grndrgn.cpp:8844`) | **Cuerpo de función**: reescrito como la regla que implementa (cambio de base entre dos numeraciones contiguas, en dos tramos). |
| 2.8 | `PlasmaFractalFill::Adjust` (`fracfill.cpp:213-262`) | **Cuerpo de función**: pseudocódigo neutro con las dos componentes (ruido y atracción) nombradas. |

### `04-inventario-funcionalidad.md`

Sin bloques de código, como estaba previsto. Solo se añadió el aviso de sala limpia.

### `05-stack-tecnologico.md` y `06-formato-xarast.md`

Sin bloques de C++. Todo lo que contienen es material propio (TOML de nuestro
workspace, YAML de CI, XML de `.xarast`, esquemas RELAX NG, Rust propuesto,
salidas de consola) o hechos publicados de terceros. Solo se añadió el aviso.
En `05` se añadió además una nota de actualización sobre la licencia (ver
«ATENCIÓN»).

---

## ATENCIÓN

### 1. Contradicción de licencia dentro de `05-stack-tecnologico.md` — **resuelta con nota, pendiente de decisión formal**

El documento `05` concluye, en su §1 y en su sección «DECISIONES», que **Xarast
debe publicarse bajo GPL-3.0-or-later**, e incluso incluye una «ACCIÓN INMEDIATA:
sustituir `/home/user/Xarast/LICENSE` (hoy MIT) por GPL-3.0». Ese razonamiento
parte del supuesto de que Xarast sería un **trabajo derivado** de Xara LX; bajo
disciplina de sala limpia ese supuesto no se sostiene y la conclusión decae.

**Qué se ha hecho:** se han añadido dos notas (una al principio del §1 y otra
sobre el bloque de decisión) que marcan el razonamiento como obsoleto y fijan
MIT OR Apache-2.0 como decisión vigente. **No** se ha borrado el análisis, porque
la comparativa de licencias de cada crate sigue siendo útil.

**Estado en el repositorio:** durante esta pasada, el proyecto ya ha formalizado la
decisión fuera de `docs/research/`: existen `docs/11-licensing-and-clean-room.md`,
`LICENSE-MIT` y `LICENSE-APACHE`, y `CLAUDE.md` recoge `MIT OR Apache-2.0` como
licencia y la regla dura de sala limpia. Queda pendiente únicamente revisar la
lista `allow` de `deny.toml` (propuesta en `05` §1.4) para que refleje un proyecto
permisivo y no uno GPL-3.

### 1 bis. Idioma de estos documentos frente a la nueva regla de `CLAUDE.md`

`CLAUDE.md` establece ahora que **todo el repositorio se escribe en inglés, sin
excepciones**, incluidos los nombres de fichero (y de hecho `docs/` ya se ha
renombrado a inglés). Los seis documentos de `docs/research/` y este informe
siguen en **español**, con nombres de fichero en español, porque así se pidió
explícitamente esta tarea y porque su contenido íntegro lo está. **No es un
problema de sala limpia**, pero sí una incoherencia pendiente: traducir los
~780 KB de `01`–`06` y renombrarlos es una tarea aparte, que debe planificarse
como tal. Los avisos de sala limpia añadidos tendrían que traducirse con ellos.

### 2. Riesgo residual: densidad de nombres internos del original

Los documentos `02` y `03` siguen citando **muchísimos nombres de clases, de
miembros y de métodos internos** de Xara LX (`NodeRenderableBounded`,
`m_LastRequestedPixWidth`, `MapTranspTypeToGDraw`…). Esto es **legítimo**: son
hechos necesarios para leer el árbol de referencia y para razonar sobre el
comportamiento, y los nombres de API no son, por sí solos, expresión protegible
en el sentido relevante aquí. Pero conviene tener presente el riesgo práctico:

* **Recomendación operativa:** los identificadores de Xarast **no deben**
  calcarse de los del original. Donde las tablas de este informe usan el nombre
  del original, es como *referencia cruzada*, no como nombre a adoptar. El
  diseño en Rust de `02` §10 ya usa nomenclatura propia; mantener esa línea.

### 3. Cabeceras de CDraw (`GDraw/*.h`): licencia **propietaria**, no GPL

Las firmas y estructuras que documentaba `03` no venían de código GPL sino de las
cabeceras de la librería binaria cerrada `libCDraw.a`, cuya licencia
(`libs/LIBS-LICENSE`) es **más restrictiva** que la GPL. Reproducirlas
textualmente era el punto más delicado de todo el conjunto. Ya no queda ninguna:
todas se han convertido en tablas descriptivas. **Regla a mantener:** nunca
volver a pegar contenido de `GDraw/*.h` en la documentación ni en el código.

### 4. Cita textual conservada deliberadamente

`05` §1.1 reproduce tres líneas del **aviso de licencia** del original
(«Xara LX is free software…»). Se conserva a propósito: es prueba documental de
que el original es GPL-2.0-**only**, es el texto de una licencia (no expresión
creativa del programa) y su cita es el uso normal y esperado de un aviso de
licencia. No se considera un problema.

### 5. Corpus de prueba

Los ficheros `.xar` de validación citados (`xara-xtreme/testfiles/`,
`Designs/`, `Templates/`, `TextDesigns/`) **no** son código: son datos de prueba
del árbol original, cubiertos también por su licencia. Usarlos localmente para
validar el importador es legítimo; **no deben redistribuirse dentro del
repositorio de Xarast**. Los volcados de bytes que aparecen en `01` son
fragmentos mínimos de cabecera, extraídos con herramienta propia, y no plantean
problema.

---

## Regla permanente para futuras tareas

> En `docs/research/` y en cualquier documento nuevo: **describir, no transcribir**.
> Si hace falta enseñar cómo funciona algo del original, se escribe en español o
> en pseudocódigo/Rust propio, con la referencia `fichero:línea` para que quien
> lo implemente pueda comprobarlo. Nunca se pega C++ del árbol de referencia,
> y **nunca** contenido de `GDraw/*.h`.
