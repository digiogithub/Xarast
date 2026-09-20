# Especificación del formato nativo `.xarast` — versión 1.0

> **Nota de sala limpia.** Este documento especifica el formato nativo `.xarast`
> y describe, solo donde hace falta para la interoperabilidad, el
> *comportamiento* y los *formatos de datos* de Xara Xtreme (GPL-2.0-only). No
> reproduce código fuente del original; las referencias `fichero:línea` apuntan
> al árbol de referencia en `xara-xtreme/` y sirven solo para localizar la lógica
> descrita. Xarast se implementa desde esta especificación, no traduciendo el
> original.

> **Estado:** NORMATIVO — candidato a congelación para Xarast v0.1.
> **Documento:** `docs/research/06-formato-xarast.md`
> **Versión de formato descrita:** `1.0`
> **Fecha:** 2026-09-19
> **Depende de:** `docs/00-vision-y-alcance.md` (principio 3), `research/01-formato-xar.md`,
> `research/02-modelo-documento.md`.
> **Nota de memoria asociada:** `docs/memory/xarast-format.md` (crear al arrancar la fase).

## Convenciones normativas

Este documento usa las palabras clave siguientes con el significado del RFC 2119/8174,
traducidas al castellano:

| Término | Equivalente RFC | Significado |
|---|---|---|
| **DEBE** / **NO DEBE** | MUST / MUST NOT | Requisito absoluto. Un fichero o implementación que lo incumple **no es conforme**. |
| **DEBERÍA** / **NO DEBERÍA** | SHOULD / SHOULD NOT | Recomendación fuerte. Desviarse exige razón documentada. |
| **PUEDE** | MAY | Opcional; ambas opciones son conformes. |

Se usa **«escritor»** para una implementación que produce `.xarast`, **«lector»** para una
que lo consume, e **«implementación»** para ambas.

Los identificadores de tags entre paréntesis (`TAG_...`) son referencias al formato binario
original de Xara, definidos en `/home/user/xara-xtreme/Kernel/cxftags.h`, y sirven de
trazabilidad entre el modelo heredado y esta especificación.

---

## Índice

1. [Objetivos de diseño y no-objetivos](#1-objetivos-de-diseño-y-no-objetivos)
2. [Investigación previa: lecciones de otros formatos](#2-investigación-previa-lecciones-de-otros-formatos)
3. [Estructura del contenedor](#3-estructura-del-contenedor)
4. [Estrategia de compresión y deduplicación](#4-estrategia-de-compresión-y-deduplicación)
5. [El perfil SVG de Xarast](#5-el-perfil-svg-de-xarast)
6. [Tabla de mapeo completa: Xara → SVG + `xarast:`](#6-tabla-de-mapeo-completa)
7. [Metadatos del documento](#7-metadatos-del-documento)
8. [Preservación de datos desconocidos (round-trip sin pérdida)](#8-preservación-de-datos-desconocidos)
9. [Identificación: magic bytes, extensión, MIME, integración de escritorio](#9-identificación-magic-bytes-extensión-mime)
10. [Recuperación ante fallos: autoguardado, journal, bloqueo](#10-recuperación-ante-fallos)
11. [Esquemas formales](#11-esquemas-formales)
12. [Ejemplo completo de un `.xarast` mínimo](#12-ejemplo-completo)
13. [Plan de implementación en Rust](#13-plan-de-implementación-en-rust)
14. [Apéndices](#14-apéndices)

---

## 1. Objetivos de diseño y no-objetivos

### 1.1 Objetivos (por orden de prioridad, decreciente)

**O1 — Fidelidad total en Xarast.** Un documento guardado y vuelto a abrir en la misma
versión de Xarast DEBE ser indistinguible del original: misma geometría (hasta la
precisión de un milipunto), mismos atributos, mismos efectos vivos *editables*, mismo
undo-equivalente de estado del documento. Guardar no DEBE destruir nunca la
parametricidad de un objeto (una QuickShape sigue siendo QuickShape, un blend sigue
siendo un blend vivo).

**O2 — Degradación elegante fuera de Xarast.** El fichero `document.svg` contenido en el
paquete DEBE ser un documento **SVG 1.1 válido y autónomo**, renderizable por cualquier
navegador moderno o por Inkscape con un resultado *visualmente aproximado*. «Aproximado»
se define de forma medible en §5.6: SSIM ≥ 0,90 frente al render nativo de Xarast para el
corpus de referencia. Nada que sea privativo de Xarast DEBE impedir el render base.

**O3 — Eficiencia de espacio comparable a `.xar`.** El formato DEBE aplicar las mismas
estrategias que hacían compacto al `.xar` original —reutilización de definiciones de
bitmap, coordenadas relativas, elisión de valores por defecto, herencia de atributos y
compresión del flujo— traducidas al dominio ZIP+SVG (§4). Objetivo cuantitativo: un
`.xarast` NO DEBERÍA exceder en más de un **40 %** el tamaño del `.xar` equivalente para
documentos predominantemente vectoriales, y DEBERÍA ser **igual o menor** para documentos
dominados por bitmaps (gracias a la deduplicación por contenido).

**O4 — Round-trip sin pérdida entre versiones.** Un lector de la versión *N* que abre un
fichero de la versión *N+k*, lo edita parcialmente y lo guarda, DEBE preservar
íntegramente los datos que no comprende (§8). Esta es la propiedad que más críticamente
falla en los formatos de la competencia y es un requisito de primer nivel aquí.

**O5 — Inspeccionable y reparable con herramientas estándar.** `unzip`, `xmllint`,
`git diff`, `grep` y un editor de texto DEBEN bastar para auditar, diagnosticar y —en
casos simples— reparar un documento. Esto es una ventaja deliberada frente a formatos
binarios opacos y una garantía de longevidad del archivo.

**O6 — Lectura incremental y barata.** Abrir la miniatura, los metadatos o la lista de
capas NO DEBE requerir descomprimir ni parsear el documento completo. El directorio
central del ZIP y `META-INF/manifest.xml` DEBEN bastar.

**O7 — Escritura atómica y resistente a fallos.** Un corte de corriente durante el
guardado NO DEBE dejar nunca al usuario sin el documento anterior (§10).

**O8 — Estable frente a control de versiones.** Dos guardados sin cambios semánticos
DEBERÍAN producir ficheros byte-idénticos (escritura determinista: sin marcas de tiempo
variables en el ZIP salvo petición explícita, orden de entradas fijo, orden de atributos
XML fijo, IDs estables). Esto permite `git` y diffs útiles.

### 1.2 No-objetivos (explícitos)

**N1 — No es un formato de intercambio universal.** El objetivo no es que otras
aplicaciones *editen* `.xarast` con fidelidad. Para intercambio existen SVG plano, PDF y
—en el futuro— exportadores dedicados.

**N2 — No compatibilidad con el `.xar` binario en escritura.** Xarast lee `.xar`; no lo
escribe (ya declarado como no-objetivo en `00-vision-y-alcance.md`). `.xarast` no
comparte ni un byte de estructura con `.xar`.

**N3 — No edición colaborativa en tiempo real ni CRDTs en v1.0.** El contenedor reserva
espacio (`history/`, `META-INF/`) para futuras extensiones, pero v1.0 asume un único
escritor.

**N4 — No cifrado ni firma digital en v1.0.** Se reservan los nombres
`META-INF/encryption.xml` y `META-INF/signatures.xml`; su semántica se define en una
versión posterior. Un lector v1.0 que encuentre estas entradas DEBE rechazar el fichero
con un error claro en lugar de abrirlo parcialmente.

**N5 — No es un formato de imagen raster.** No se almacena el documento rasterizado como
representación primaria; sólo miniaturas y previsualizaciones derivadas.

**N6 — No SVG «puro» sin extensiones.** Se rechaza explícitamente el enfoque de intentar
expresarlo todo en SVG estándar: perdería la parametricidad (O1). La exportación a SVG
plano es una operación distinta y con pérdida.

**N7 — No se soporta streaming de escritura parcial en red.** El fichero se escribe
completo y se renombra atómicamente.

---

## 2. Investigación previa: lecciones de otros formatos

Se han analizado nueve formatos comparables. Esta sección resume qué se copia y qué se
evita, con la decisión de diseño concreta que se deriva de cada lección.

### 2.1 ODF / OpenDocument Graphics (`.odg`) — OASIS

Contenedor ZIP con `mimetype` como **primera entrada, almacenada sin comprimir y sin
campo extra**, de modo que la cadena del tipo MIME cae siempre en el offset fijo 38
(30 bytes de cabecera local + 8 del nombre `mimetype`). Un `META-INF/manifest.xml`
enumera todas las entradas con su `media-type`. El contenido va en `content.xml`, los
estilos en `styles.xml`, los metadatos en `meta.xml` y los binarios en `Pictures/`.

- **Se copia:** el truco del `mimetype` (detección por *magic* sin descomprimir), el
  manifiesto explícito, la separación metadatos/contenido, la miniatura estándar.
- **Se copia:** la regla de que la entrada `/` del manifiesto DEBE coincidir con el
  contenido de `mimetype`.
- **Se evita:** la fragmentación en `content.xml` + `styles.xml` + `settings.xml`, que
  obliga a resolver referencias cruzadas entre cuatro árboles XML para pintar un objeto.
  Xarast concentra el documento en **un solo** `document.svg`.
- **Se evita:** el vocabulario XML propio (`draw:`, `svg:`, `style:`) que *parece* SVG
  pero no lo es, y que por tanto no se puede abrir en ningún visor. Ese es precisamente
  el fallo que O2 corrige.

### 2.2 Krita (`.kra`)

ZIP con `mimetype`, `maindoc.xml` (árbol de capas y propiedades), `documentinfo.xml`,
`preview.png`, `mergedimage.png` y un directorio `layers/` con los píxeles binarios.

- **Se copia:** `mergedimage.png` es una idea excelente — una representación *plana y
  siempre legible* del resultado, que permite a gestores de archivos, importadores
  simples y a la propia aplicación mostrar el documento sin entender su modelo.
  Xarast adopta el equivalente vectorial: el propio `document.svg` es el «merged
  document», y además hay `thumbnail.png` y `previews/`.
- **Se copia:** la separación radical entre el árbol (XML pequeño, parseable rápido) y
  los datos pesados (entradas binarias independientes).
- **Se evita:** el binario opaco por capa (`layers/layerN`), que hace el formato
  inauditable y dependiente de la versión exacta de Krita.

### 2.3 Scribus (`.sla`)

XML plano (opcionalmente gzip como `.sla.gz`), con un único árbol donde los
`<PAGEOBJECT>` se anexan en orden de creación y referencian su página por atributo.

- **Se evita:** el orden de aparición desacoplado del orden de pintado/página. Es una
  fuente inagotable de errores y hace imposible razonar sobre orden Z leyendo el fichero.
  En Xarast **el orden del documento ES el orden Z**, como en SVG.
- **Se evita:** el fichero XML monolítico sin contenedor: obliga a incrustar todas las
  imágenes en base64 o a depender de rutas absolutas externas (Scribus enlaza las
  imágenes por ruta del sistema de ficheros: los documentos se rompen al moverlos).
  **Decisión derivada:** en Xarast todo recurso referenciado DEBE residir dentro del
  paquete; las referencias externas absolutas están prohibidas (§5.8).
- **Se copia:** la legibilidad del XML y la facilidad de transformarlo con XSLT/scripts.

### 2.4 Inkscape SVG (`sodipodi:` / `inkscape:`)

SVG estándar enriquecido con atributos y elementos de dos espacios de nombres propios
(`http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd` y
`http://www.inkscape.org/namespaces/inkscape`). Capas como
`<g inkscape:groupmode="layer" inkscape:label="...">`, guías como `<sodipodi:guide>`
dentro de `<sodipodi:namedview>`, formas paramétricas como `<path sodipodi:type="star"
sodipodi:sides="5" ... d="...">`.

- **Se copia, y es la inspiración central del diseño:** la técnica de *doble
  representación* — el atributo estándar (`d`) contiene la geometría «horneada» que
  cualquiera renderiza, y los atributos del espacio propio contienen los **parámetros**
  que permiten regenerarla. La especificación SVG 1.1 garantiza que un agente de usuario
  «incluirá los atributos desconocidos en el DOM pero por lo demás los ignorará», lo que
  hace esto seguro por construcción.
- **Se copia:** la compatibilidad deliberada de las capas: emitir `inkscape:groupmode` e
  `inkscape:label` cuesta ~40 bytes por capa y hace que Inkscape abra el documento con
  sus capas intactas. Es la mayor relación valor/coste de todo el formato.
- **Se evita:** la ausencia de versionado del vocabulario propio. Inkscape no marca qué
  versión escribió cada extensión, lo que ha causado roturas históricas.
  **Decisión derivada:** todo el vocabulario `xarast:` va versionado en el URI del
  espacio de nombres y, además, cada documento declara `xarast:version` y
  `xarast:min-reader`.
- **Se evita:** la pérdida de extensiones al pasar por herramientas de terceros. Es un
  problema conocido y bien documentado que limpiadores de SVG y librerías de parseo
  eliminan `sodipodi:`/`inkscape:` y dejan el fichero irrecuperable para el editor.
  **Decisión derivada:** §8 hace de la preservación una obligación normativa y añade un
  digest para *detectar* que alguien ha destruido los datos, en vez de fallar en silencio.

### 2.5 Figma (`.fig`)

Formato binario basado en **Kiwi**, un esquema binario tipo Protocol Buffers. El
documento se serializa como un **array plano de `nodeChanges`** con referencia al padre,
no como árbol; el árbol se reconstruye al cargar. Cada fichero **incrusta su propio
esquema** (más de 500 definiciones de tipo), lo que lo hace auto-descriptivo.

- **Se copia (conceptualmente):** la idea de auto-descripción. Xarast la implementa de
  forma mucho más barata: el fichero declara qué versión de vocabulario usa y qué
  capacidades requiere de un lector (`xarast:min-reader`, `xarast:requires`).
- **Se copia:** el modelo plano con referencias es muy bueno para sincronización
  incremental. **No** se adopta en v1.0 (N3) pero el diseño de IDs estables (§5.7) es una
  condición previa para poder adoptarlo después sin romper el formato.
- **Se evita:** el binario opaco. Contradice O5 directamente. El coste es tamaño y
  velocidad de parseo; se compensa con compresión (§4) y con parseo SAX en streaming.

### 2.6 SVGZ

Simplemente SVG comprimido con gzip. Ampliamente soportado (los navegadores lo
descomprimen si el servidor manda `Content-Encoding`, e Inkscape/libxml2 lo hacen al
vuelo).

- **Se evita como formato nativo:** un único flujo gzip no permite acceso aleatorio, ni
  miniatura barata (O6), ni almacenar binarios sin recomprimir, ni deduplicar. Obligaría
  a base64 para cada imagen: **+33 % de tamaño** sobre datos ya comprimidos.
- **Se conserva como formato de exportación:** Xarast DEBERÍA ofrecer «Exportar → SVG
  comprimido (`.svgz`)», produciendo un fichero único autocontenido con `data:` URIs.

### 2.7 Adobe Illustrator (`.ai` moderno)

Un PDF válido con los datos privativos de Illustrator incrustados en un flujo
`PieceInfo`/`AIPrivateData` dentro del propio PDF.

- **Se copia:** exactamente el mismo principio que O2/§5.1 — un formato estándar,
  ampliamente legible, con los datos ricos adjuntos en un canal que los lectores estándar
  ignoran. Es la validación en el mundo real de que la estrategia funciona a escala
  industrial durante décadas.
- **Se evita:** que el «canal privado» sea un blob binario opaco. En `.ai`, si Illustrator
  no puede leer su `AIPrivateData`, el usuario se queda con un PDF plano no editable, y no
  hay manera de diagnosticarlo. En Xarast el canal privado es XML legible.
- **Se evita:** la duplicación total de la geometría. `.ai` almacena el arte dos veces
  (PDF + privado), casi doblando el tamaño. Xarast **no duplica geometría** cuando el SVG
  estándar es suficiente; sólo hornea representaciones alternativas para lo que SVG no
  puede expresar, y esas subredes horneadas van marcadas con `xarast:generated` y
  **pueden descartarse y regenerarse** (§5.4).

### 2.8 Affinity (`.afdesign`, `.afphoto`, `.afpub`)

Formato binario privativo y no documentado (cabecera que empieza por `00 FF 4B 41`).

- **Se evita todo.** Se cita como contraejemplo: ninguna herramienta externa lo lee, no
  hay recuperación posible de ficheros dañados, y la interoperabilidad depende por
  completo del proveedor. Es el escenario que Xarast existe para no repetir.

### 2.9 El propio `.xar` de Xara

Formato de registros `(tag: u32, size: u32, data)` con dos etapas de compresión: tipos de
dato «ricos» que codifican información gráfica de alto nivel en muy pocos bytes, y
compresión zlib del flujo de registros entre `TAG_STARTCOMPRESSION` (30) y
`TAG_ENDCOMPRESSION` (31). Las lecciones concretas de eficiencia son cuatro, y las cuatro
se trasladan en §4.5:

1. **Definiciones compartidas referenciadas por índice.** `TAG_DEFINEBITMAP_*` (65-71)
   define un bitmap una sola vez; cada nodo `TAG_NODE_BITMAP` (198) lo referencia. Lo
   mismo con `TAG_DEFINERGBCOLOUR` (50) y `TAG_DEFINECOMPLEXCOLOUR` (51).
2. **Coordenadas relativas.** `TAG_PATH_RELATIVE*` (113-116) y `TAG_PATHREF_TRANSLATE`
   (4014) / `TAG_PATHREF_IDENTICAL` (4013) evitan repetir coordenadas absolutas.
3. **Variantes «simple» vs «complex» del mismo registro.** `TAG_ELLIPSE_SIMPLE` (1000) vs
   `TAG_ELLIPSE_COMPLEX` (1001), `TAG_RECTANGLE_SIMPLE` (1100) vs las 16 variantes
   redondeada/estrellada/reformada: el caso frecuente no paga por los campos del caso
   raro. También `TAG_FLATFILL_NONE/BLACK/WHITE` (190-192): registros de 0 bytes para los
   valores más comunes.
4. **Herencia de atributos por posición en el árbol.** Los atributos se aplican al
   subárbol siguiente (`TAG_UP`/`TAG_DOWN`), así que no se repiten por objeto. Es
   exactamente la semántica de herencia de propiedades de presentación de SVG en `<g>`.

- **Se evita:** el registro de tags global y congelado por rangos numéricos
  (`0-3999` Camelot 1.5, `4000-4999` Camelot 2.0), que obligó a comentarios del tipo
  «*DO NOT define tags outside this range. Ask Mark Neves*». Los espacios de nombres XML
  eliminan el problema de la asignación centralizada de identificadores.

### 2.10 Síntesis: las cinco decisiones estructurales

| # | Decisión | Deriva de |
|---|---|---|
| D1 | Contenedor **ZIP** con `mimetype` STORED en primera posición | ODF, KRA, EPUB |
| D2 | Un **único** `document.svg`, SVG 1.1 válido, con orden Z = orden del documento | Contra ODF/Scribus; a favor de SVG |
| D3 | **Doble representación**: SVG estándar horneado + parámetros `xarast:` | Inkscape, `.ai` |
| D4 | **Deduplicación por contenido** (BLAKE3) y elisión agresiva en el serializador SVG | `.xar` |
| D5 | **Preservación obligatoria** de lo desconocido, con digest de verificación | Contra Inkscape/todos |

---

## 3. Estructura del contenedor

### 3.1 Formato base

Un fichero `.xarast` **DEBE** ser un archivo ZIP conforme a *PKWARE APPNOTE 6.3.x*
(compatible con el subconjunto que implementan `zip`/`unzip`, `libarchive`, `libzip`,
`java.util.zip` y el crate `zip` de Rust).

- El archivo **DEBE** usar el «End of Central Directory» clásico cuando quepa; **DEBE**
  usar ZIP64 cuando el archivo exceda 4 GiB o 65 535 entradas, y los lectores **DEBEN**
  soportar ZIP64.
- Los nombres de entrada **DEBEN** codificarse en **UTF-8** con el bit 11 del flag de
  propósito general (*language encoding flag*, EFS) activado.
- Los nombres **DEBEN** usar `/` como separador, **NO DEBEN** empezar por `/`, **NO
  DEBEN** contener `..`, `\`, caracteres de control ni secuencias UTF-8 inválidas. Un
  lector **DEBE** rechazar (no sanear silenciosamente) cualquier entrada que incumpla
  esto: es la mitigación de *zip-slip*.
- El archivo **NO DEBE** contener entradas duplicadas. Un lector que las encuentre
  **DEBE** rechazar el fichero.
- Las entradas de directorio (nombre terminado en `/`, tamaño 0) **PUEDEN** omitirse; los
  lectores **NO DEBEN** depender de su existencia.
- **NO DEBE** usarse cifrado ZIP en v1.0 (N4).

### 3.2 Layout normativo de entradas

El orden de las entradas en el archivo **DEBE** ser el siguiente (las entradas ausentes
simplemente se saltan; el orden relativo de las presentes es obligatorio):

```
 1. mimetype                          OBLIGATORIA, primera, STORED, sin campo extra
 2. META-INF/manifest.xml             OBLIGATORIA
 3. meta.xml                          OBLIGATORIA
 4. document.svg                      OBLIGATORIA
 5. thumbnail.png                     RECOMENDADA
 6. previews/<...>.png                OPCIONAL
 7. resources/<...>                   OPCIONAL
 8. history/<...>                     OPCIONAL
 9. extensions/<...>                  OPCIONAL (extensiones de terceros)
10. cualquier otra entrada            PRESERVADA (§8.3)
```

Dentro de cada grupo, las entradas **DEBERÍAN** ordenarse lexicográficamente por nombre
(orden de bytes UTF-8) para cumplir O8 (determinismo).

#### 3.2.1 `mimetype`

- **DEBE** ser la primera entrada del archivo.
- **DEBE** almacenarse con método `STORED` (0), sin compresión.
- **NO DEBE** tener campo *extra* en la cabecera local (ni de alineación, ni timestamps
  extendidos): esto garantiza que el contenido empiece exactamente en el **offset 38**
  (30 bytes de cabecera local + 8 bytes del nombre `mimetype`).
- Su contenido **DEBE** ser exactamente los 26 bytes ASCII
  `application/vnd.xarast+zip`, **sin** salto de línea final, **sin** BOM.

#### 3.2.2 `META-INF/manifest.xml`

Inventario normativo de todas las entradas. Detalles y esquema en §3.4 y §11.1.
**DEBE** existir. **DEBE** describir *todas* las entradas del archivo, incluidas las
desconocidas preservadas (§8.3) y la propia `mimetype`.

#### 3.2.3 `meta.xml`

Metadatos del documento (autor, fechas, unidades, tamaño de página, paleta…). §7.
Se elige un fichero separado —en lugar de meterlo todo en el SVG— para cumplir O6:
un gestor de archivos puede leer `mimetype` + `meta.xml` + `thumbnail.png`
(típicamente < 4 KiB descomprimidos) sin tocar un `document.svg` de 20 MiB.

#### 3.2.4 `document.svg`

**El documento.** Un SVG 1.1 válido y autónomo. §5.
- **NO DEBE** comprimirse con gzip *dentro* del ZIP (doble compresión inútil): el ZIP ya
  lo comprime. La entrada se llama `.svg`, no `.svgz`.
- **DEBE** empezar con la declaración XML `<?xml version="1.0" encoding="UTF-8"?>`.
- **DEBERÍA** terminar con un salto de línea.

#### 3.2.5 `thumbnail.png`

Miniatura del documento (primer *spread*, área de página).
- **DEBERÍA** existir en todo fichero guardado interactivamente.
- **DEBE** ser un PNG válido, RGBA de 8 bits por canal.
- Su lado mayor **DEBERÍA** ser de **256 px**; **NO DEBE** exceder 512 px.
- **DEBE** representar el documento sobre fondo transparente, con el color de página
  compuesto debajo si la página tiene fondo opaco.
- Se ubica en la raíz (no en `Thumbnails/` como ODF) por simplicidad y porque el
  manifiesto declara su rol explícitamente.

#### 3.2.6 `previews/`

Previsualizaciones por *spread*, para navegación rápida en documentos multipágina y para
el diálogo «versiones».

```
previews/spread-1.png
previews/spread-2.png
```

- El nombre **DEBE** ser `previews/spread-<n>.png`, con `<n>` el índice 1-based del
  spread en orden de documento.
- Lado mayor **DEBERÍA** ser 512 px, **NO DEBE** exceder 1024 px.
- Son **derivados**: un lector **DEBE** poder funcionar sin ellos, y un escritor
  **PUEDE** omitirlos (p. ej. en guardado no interactivo o por CLI con `--no-previews`).

#### 3.2.7 `resources/`

Todos los binarios referenciados por el documento. Subdirectorios normativos:

| Subdirectorio | Contenido | Nombrado |
|---|---|---|
| `resources/images/` | Bitmaps *maestros*: los píxeles originales que el usuario importó | `b3-<hash32>.<ext>` |
| `resources/derived/` | Renditions derivadas (recortes, ajustes fotográficos horneados) | `b3-<hash32>.<ext>` |
| `resources/baked/` | Rasterizaciones de fallback para rellenos/efectos que SVG no expresa (§5.4) | `b3-<hash32>.png` |
| `resources/fonts/` | Subconjuntos de fuentes embebidas (WOFF2) | `b3-<hash32>.woff2` |
| `resources/profiles/` | Perfiles de color ICC | `b3-<hash32>.icc` |
| `resources/brushes/` | Definiciones de pincel/trazo (`TAG_BRUSHDEFINITION`, 4080) | `b3-<hash32>.xml` |
| `resources/blobs/` | Cualquier otro binario opaco (incluye datos preservados) | `b3-<hash32>.bin` |

- `<hash32>` **DEBE** ser los **primeros 32 caracteres hexadecimales en minúsculas** del
  hash BLAKE3-256 del contenido **sin comprimir** del recurso (128 bits: colisión
  despreciable para el dominio). El hash completo de 64 hex **DEBE** figurar en el
  manifiesto.
- `<ext>` **DEBE** corresponder al tipo real del contenido (`png`, `jpg`, `webp`, `avif`,
  `jxl`, `tiff`, `gif`, `svg`, `woff2`, `icc`, `bin`).
- Un escritor **NO DEBE** conservar en `resources/` entradas no referenciadas por el
  documento **salvo** que estén marcadas como preservadas (§8.3) o como historial.

#### 3.2.8 `history/` (opcional)

Instantáneas de versiones anteriores del documento, para «deshacer entre sesiones» y para
el diálogo de historial.

```
history/index.xml
history/0007/document.svg
history/0007/meta.xml
```

- **DEBE** estar desactivado por defecto en v1.0 (activable por preferencia).
- `history/index.xml` **DEBE** listar cada instantánea con `id`, `timestamp` (RFC 3339,
  UTC), `label` opcional, y el tamaño acumulado.
- Los recursos binarios **NO DEBEN** duplicarse: las instantáneas referencian las mismas
  rutas `resources/` (esto es exactamente el pago que hace rentable la deduplicación).
- El escritor **DEBE** imponer un límite configurable (por defecto: 10 instantáneas o
  25 % del tamaño del documento, lo que se alcance antes) y podar las más antiguas.

#### 3.2.9 `extensions/`

Espacio reservado para datos de plugins o de aplicaciones de terceros. Un escritor
**NO DEBE** modificar ni eliminar entradas bajo `extensions/` que no haya creado él
(§8.3).

### 3.3 Reglas de referencia entre entradas

- Todas las referencias desde `document.svg` a recursos **DEBEN** ser **rutas relativas
  al directorio del propio `document.svg`**, es decir, a la raíz del paquete
  (`resources/images/b3-....png`).
- Las referencias **NO DEBEN** ser absolutas (`/resources/...`), ni `file://`, ni `http(s)://`,
  ni contener `..`. Un lector **DEBE** tratar una referencia externa como recurso ausente
  y **DEBE** avisar al usuario, no cargarla (mitigación de exfiltración y de documentos
  que se rompen al moverlos, el fallo de Scribus, §2.3).
- Excepción explícita: `data:` URIs **PUEDEN** usarse para recursos menores de 4 KiB
  (p. ej. un patrón diminuto), pero **NO DEBERÍAN** usarse por encima de ese umbral
  (§4.3).
- Consecuencia útil de la ruta relativa: al **descomprimir el paquete a un directorio**,
  `document.svg` se abre en un navegador (`file://`) y resuelve correctamente todas sus
  imágenes. Esta es la vía operativa concreta para cumplir O2 (§5.9).

### 3.4 `META-INF/manifest.xml`

Espacio de nombres: `https://xarast.org/ns/manifest/1.0`, prefijo convencional `mf`.

Ejemplo (el ejemplo completo está en §12):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0"
             mf:version="1.0"
             mf:min-reader="1.0"
             mf:generator="Xarast/0.1.0 (linux; x86_64)"
             mf:profile="portable">
  <mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/>
  <mf:file-entry mf:full-path="mimetype"
                 mf:media-type="text/plain"
                 mf:role="mimetype"
                 mf:size="26" mf:method="stored"/>
  <mf:file-entry mf:full-path="meta.xml"
                 mf:media-type="application/xml" mf:role="meta"
                 mf:size="1184" mf:method="deflate"
                 mf:digest="blake3-256" mf:digest-value="a3c9...e1"/>
  <mf:file-entry mf:full-path="document.svg"
                 mf:media-type="image/svg+xml" mf:role="document"
                 mf:size="20714" mf:method="deflate"
                 mf:digest="blake3-256" mf:digest-value="7f10...bd"/>
  <mf:file-entry mf:full-path="thumbnail.png"
                 mf:media-type="image/png" mf:role="thumbnail"
                 mf:size="9210" mf:method="stored"/>
  <mf:file-entry mf:full-path="resources/images/b3-4a91c0de5f73b8a2.png"
                 mf:media-type="image/png" mf:role="resource"
                 mf:size="482113" mf:method="stored"
                 mf:digest="blake3-256" mf:digest-value="4a91c0de5f73b8a2...9c"
                 mf:refcount="3"/>
</mf:manifest>
```

Reglas normativas del manifiesto:

1. **DEBE** existir exactamente un `<mf:file-entry>` con `mf:full-path="/"`; su
   `mf:media-type` **DEBE** ser idéntico al contenido de la entrada `mimetype` (regla
   heredada de ODF).
2. **DEBE** existir un `<mf:file-entry>` por **cada** entrada del ZIP (salvo entradas de
   directorio).
3. `mf:role` **DEBE** ser uno de: `mimetype`, `manifest`, `meta`, `document`,
   `thumbnail`, `preview`, `resource`, `history`, `extension`, `unknown`.
   Un lector que encuentre un `mf:role` desconocido **DEBE** tratarlo como `unknown` y
   preservar la entrada.
4. `mf:digest`/`mf:digest-value` **DEBEN** estar presentes para `document`, `meta` y todo
   `resource`; **PUEDEN** omitirse para `mimetype` y previsualizaciones.
   El único algoritmo definido en v1.0 es `blake3-256` (64 hex, minúsculas).
5. `mf:size` es el tamaño **descomprimido** en bytes.
6. `mf:method` es informativo (`stored`, `deflate`, `zstd`); el ZIP es la fuente de
   verdad. Sirve para auditar sin abrir cada cabecera local.
7. `mf:refcount` es informativo: número de referencias al recurso desde `document.svg`.
   Útil para diagnosticar la eficacia de la deduplicación.
8. `mf:profile` **DEBE** ser `portable` (sólo `stored`+`deflate`) o `compact`
   (permite `zstd`). Ver §4.2.
9. Si un lector detecta discrepancia entre el manifiesto y el contenido real del ZIP
   (entrada faltante, sobrante o digest que no cuadra), **DEBE** avisar al usuario y
   **DEBERÍA** ofrecer abrir en modo sólo lectura. **NO DEBE** abrir silenciosamente.

### 3.5 Por qué XML y no JSON para el manifiesto

Se ha considerado `META-INF/manifest.json`. Se elige XML por tres razones concretas:

1. **Homogeneidad de la cadena de herramientas.** El documento ya es XML (SVG) y los
   metadatos también. Un único parser (`quick-xml`) cubre los tres; con JSON harían falta
   dos, y dos modelos de escape/codificación distintos.
2. **Precedente y herramientas.** `xmllint --relaxng` valida el manifiesto en CI sin
   escribir código; el ecosistema ODF/OPC ha validado este diseño durante 20 años.
3. **Espacios de nombres.** La extensibilidad por terceros con garantías de no colisión
   es nativa en XML. En JSON hay que inventar una convención de prefijos.

Se reconoce la desventaja (verbosidad, ~1,6× frente a JSON) y se considera irrelevante:
el manifiesto típico ocupa 0,5-3 KiB antes de comprimir y < 500 bytes después, frente a
documentos de cientos de KiB.

---

## 4. Estrategia de compresión y deduplicación

### 4.1 Principio rector

**No recomprimir lo que ya está comprimido, comprimir agresivamente lo que es texto, y
no almacenar dos veces lo mismo.** Las tres reglas, aplicadas juntas, reproducen en el
dominio ZIP+SVG lo que `.xar` conseguía con registros ricos y zlib (§2.9).

### 4.2 Métodos de compresión permitidos

| Método ZIP | ID | Perfil `portable` | Perfil `compact` | Uso |
|---|---|---|---|---|
| `Stored` | 0 | **DEBE** soportarse | **DEBE** soportarse | `mimetype`, binarios ya comprimidos |
| `Deflate` | 8 | **DEBE** soportarse | **DEBE** soportarse | XML, SVG, texto |
| `Zstandard` | **93** | **NO DEBE** usarse | **PUEDE** usarse | XML/SVG grandes, binarios incompresibles pero no ya-comprimidos |

Notas normativas:

- Un **lector conforme DEBE** soportar `Stored` y `Deflate`, y **DEBERÍA** soportar
  `Zstandard` (método 93).
- Un **escritor DEBE** usar el perfil `portable` por defecto. El perfil `compact`
  **DEBE** activarse explícitamente (preferencia o `--profile compact`) y **DEBE**
  declararse en `mf:profile` y elevar `mf:min-reader` a `1.0` con la capacidad
  `zstd` en `<mf:requires>` (§8.4).
- Se usa el ID **93** y **NO** el 20. El 20 fue asignado a Zstandard en APPNOTE 6.3.7 y
  **reemplazado por el 93** en 6.3.8 para evitar conflictos; 93 es lo que escriben WinZip,
  libzip, libarchive, 7-Zip y el módulo `zipfile` de Python. Un lector **PUEDE** aceptar
  el 20 en lectura por tolerancia, pero **NO DEBE** escribirlo nunca.
- **NO DEBEN** usarse BZip2, LZMA, XZ, PPMd, Deflate64 ni Shrink/Implode. Motivo:
  interoperabilidad pobre y ganancia marginal frente a zstd.
- Nivel recomendado: `Deflate` nivel 6 en guardado interactivo, nivel 9 en
  «Guardar como… → optimizado»; `Zstandard` nivel 10 interactivo, 19 optimizado.
  El nivel **NO** afecta a la conformidad del fichero.

### 4.3 Qué comprimir y qué no (tabla normativa)

| Tipo de entrada | Método | Justificación |
|---|---|---|
| `mimetype` | **STORED** (obligatorio) | Requisito de detección por offset fijo (§3.2.1) |
| `document.svg`, `meta.xml`, `META-INF/manifest.xml`, `history/**/*.xml`, `*.svg` | **DEFLATE** (o ZSTD en `compact`) | Texto: ratio típico 6:1 a 12:1 |
| PNG, JPEG, WebP (lossy y lossless), AVIF, JXL, GIF | **STORED** | Ya llevan Deflate/DCT/VP8L/AV1 dentro. Recomprimir cuesta CPU y **no** ahorra: medido, `deflate -6` sobre un JPEG típico ahorra 0,1-0,5 % y sobre un PNG 0,0-0,2 %. |
| WOFF2 | **STORED** | Brotli interno |
| ICC | **DEFLATE** | Perfiles con tablas repetitivas: 2:1 a 4:1 |
| TIFF sin comprimir, BMP, RAW | **DEFLATE** (o ZSTD) | Píxeles sin comprimir. **Pero**: un escritor **DEBERÍA** convertirlos a PNG/JPEG al importar en vez de almacenarlos crudos |
| `resources/blobs/*.bin` (opaco preservado) | **DEFLATE** | Desconocido: la heurística de §4.4 decide |
| `thumbnail.png`, `previews/*.png` | **STORED** | PNG |

**Regla de escape (heurística obligatoria).** Para cualquier entrada no cubierta
explícitamente arriba, el escritor **DEBE** aplicar:

1. Si el tipo MIME está en la lista de «ya comprimidos» → `STORED`.
2. En otro caso, comprimir los primeros **64 KiB** con `Deflate` nivel 1. Si la ratio
   resultante es **< 1,05** (menos del 5 % de ahorro) → almacenar toda la entrada como
   `STORED`. Si no → comprimir entera con el método y nivel del perfil.

Esto evita el patrón patológico de los ZIP «ingenuos» que gastan minutos recomprimiendo
gigabytes de JPEG para ahorrar kilobytes.

**Sobre `data:` URIs.** Incrustar una imagen en el SVG como base64 la infla un **33 %**
*y* la mete dentro de un flujo que sí se recomprime, destruyendo las dos optimizaciones
anteriores *y* haciendo imposible la deduplicación entre documentos y el acceso aleatorio.
Por eso §3.3 las limita a 4 KiB.

### 4.4 Deduplicación por contenido

**Mecanismo.** Todo recurso binario se identifica por el **BLAKE3-256 de su contenido
sin comprimir**. Se elige BLAKE3 y no SHA-256 por velocidad (del orden de 1-3 GB/s por
núcleo, con paralelización interna por árbol de Merkle), lo que permite hashear al vuelo
durante la importación sin coste perceptible, y porque es criptográficamente sólido —
descartando colisiones adversarias, no sólo accidentales.

Reglas:

1. Al importar un binario, el escritor **DEBE** calcular su BLAKE3-256 y consultar el
   índice de recursos del documento. Si ya existe un recurso con ese hash, **DEBE**
   reutilizarlo e incrementar su `refcount` en lugar de añadir una entrada nueva.
2. El nombre de la entrada **DEBE** derivarse del hash (§3.2.7), lo que hace la
   deduplicación **estructural**: es imposible tener dos entradas con el mismo contenido
   y nombres distintos dentro de la misma carpeta.
3. Al guardar, el escritor **DEBE** recorrer el documento, contar referencias reales y
   **omitir** los recursos con `refcount == 0` (recolección de basura), excepto los
   referenciados por `history/` o marcados como preservados.
4. El escritor **DEBERÍA** mantener un índice en memoria `hash → (ruta, refcount)`
   mientras el documento está abierto, y **NO DEBERÍA** rehashear recursos sin cambios
   al guardar (guardar un documento de 300 MB no debe leer 300 MB).

**Deduplicación de segundo nivel: maestro + derivadas.** Esta es la traducción directa de
`TAG_DEFINEBITMAP_*` + `TAG_XPE_BITMAP_PROPERTIES` (4117) + `TAG_DEFINEBITMAP_XPE` (4118)
de Xara, donde el bitmap se guarda una vez y las variantes procesadas se regeneran al
vuelo desde el maestro más los parámetros de proceso.

- El **maestro** (los píxeles tal como los importó el usuario) va en `resources/images/`.
- Cada **derivada** (recorte, ajuste de brillo/contraste horneado, versión reescalada
  para pantalla) va en `resources/derived/` y **DEBE** declarar en el manifiesto
  `mf:derived-from="<hash del maestro>"` y `mf:derivation="<id de la cadena de ops>"`.
- Un escritor **PUEDE** omitir por completo las derivadas (`--no-derived`) si la cadena
  de operaciones es determinista y está descrita en `<xarast:photo-ops>` (§6.9): el
  lector las regenera. Es un intercambio tamaño/tiempo de apertura explícito.
- Consecuencia: 8 usos de la misma foto con 3 recortes distintos ocupan
  **1 maestro + 3 derivadas**, no 8 copias.

**Deduplicación de geometría.** Además de los binarios, el escritor **DEBERÍA**
deduplicar subárboles vectoriales idénticos: si dos o más objetos tienen geometría y
atributos idénticos módulo una transformación afín, **DEBERÍA** emitir uno en
`<defs>` y referenciarlo con `<use>`. Esto es el equivalente SVG de
`TAG_PATHREF_IDENTICAL` (4013) y `TAG_PATHREF_TRANSLATE` (4014). El umbral recomendado es
**≥ 3 repeticiones** o **≥ 512 bytes** de `d` repetido, para que la indirección compense.

### 4.5 Las cuatro optimizaciones de `.xar`, traducidas

| Técnica de `.xar` | Traducción en `.xarast` | Ahorro medido/estimado |
|---|---|---|
| Definiciones de bitmap referenciadas por índice (`TAG_DEFINEBITMAP_*`) | Deduplicación BLAKE3 + `<pattern>`/`<image>` reutilizados (§4.4) | Hasta **N×** en documentos con repetición |
| Coordenadas relativas (`TAG_PATH_RELATIVE*`) | Comandos de path relativos (`m`,`l`,`c`,`s`,`h`,`v`) + cuantización a 3 decimales | **18-30 %** sobre el `d` absoluto |
| Variantes «simple/complex» y registros vacíos (`TAG_FLATFILL_BLACK`) | Elisión de atributos con valor por defecto SVG; atributos de presentación en vez de `style=`; IDs cortos | **10-20 %** del SVG |
| Herencia por posición en el árbol (`TAG_UP`/`TAG_DOWN`) | Hoisting de atributos comunes al `<g>` padre y uso de `<style>` con clases cuando ≥ 8 objetos comparten pintura | **8-25 %** del SVG |
| Compresión del flujo (`TAG_STARTCOMPRESSION`) | Deflate/zstd de la entrada `document.svg` | **6:1 a 12:1** |

#### 4.5.1 Pasadas normativas del serializador SVG

El escritor **DEBE** implementar, en este orden, las pasadas siguientes al emitir
`document.svg`. Cada una **DEBE** ser semánticamente neutra (verificable por el test de
round-trip de §13.5).

1. **Normalización de números.** Coordenadas en unidades de usuario = puntos PostScript.
   Se emiten con **hasta 3 decimales** (1 milipunto exacto, la unidad interna de Xara) y
   **sin ceros finales** (`12.5`, no `12.500`). Se omite el `0` inicial (`.5`, no `0.5`).
   Exponentes sólo si acortan.
2. **Path relativo.** Para cada path se generan la variante absoluta y la relativa y se
   emite la más corta. Se colapsan comandos repetidos (`l 1,0 l 2,0` → `l 1,0 2,0`), se
   usan `h`/`v` para segmentos axiales y `s`/`t` para curvas con control reflejado.
3. **Elisión de valores por defecto.** No se emite `fill="black"`, `fill-opacity="1"`,
   `stroke="none"`, `stroke-width="1"`, `opacity="1"`, `stroke-linecap="butt"`,
   `stroke-linejoin="miter"`, `stroke-miterlimit="4"`, `fill-rule="nonzero"`,
   `transform="matrix(1,0,0,1,0,0)"`, `preserveAspectRatio="xMidYMid meet"`.
   **Excepción:** si el valor por defecto SVG difiere del valor heredado del `<g>` padre,
   **DEBE** emitirse explícitamente.
4. **Hoisting de atributos.** Si todos los hijos directos de un `<g>` comparten el mismo
   valor de una propiedad heredable, se sube al `<g>` y se borra de los hijos.
5. **Clases CSS.** Si ≥ 8 elementos comparten el mismo conjunto de ≥ 3 propiedades de
   pintura, se emite una regla en un único `<style type="text/css">` dentro de `<defs>`
   y se sustituyen por `class="cN"`. **NO DEBE** usarse CSS para nada que afecte a la
   geometría o a la semántica del modelo (sólo pintura), para que la eliminación del
   `<style>` por una herramienta de terceros degrade el color, nunca la estructura.
6. **IDs cortos y estables.** Los IDs emitidos **DEBEN** ser los IDs persistentes del
   modelo (§5.7), no índices de posición. Para elementos internos autogenerados
   (gradientes, marcadores, clips) se usan IDs cortos derivados del hash del contenido
   (`g7a3`, `m1f`, `c22`), lo que deduplica automáticamente definiciones idénticas.
7. **Deduplicación de `<defs>`.** Dos gradientes, patrones, marcadores, filtros o
   `clipPath` con contenido idéntico **DEBEN** colapsarse en uno.
8. **Indentación mínima.** En guardado normal, sin indentación (un salto de línea por
   elemento de alto nivel). Con `--pretty` se indenta a 1 espacio para diff en git.
   La indentación afecta al tamaño **antes** de comprimir mucho más que después (deflate
   come el espacio en blanco casi gratis: ~2 % del comprimido), así que es una opción de
   usuario, no un requisito.

### 4.6 Estimaciones de tamaño

Cifras estimadas sobre el corpus `xara-xtreme/Designs/` y `testfiles/`, con la
metodología: SVG generado por las 8 pasadas de §4.5.1, deflate-9, binarios STORED.

#### Caso A — Ilustración vectorial media (≈ 1 800 objetos, 14 capas, sin bitmaps)

| Componente | Sin comprimir | En el `.xarast` |
|---|---|---|
| `document.svg` | 1 240 KiB | 118 KiB (deflate-9) / 84 KiB (zstd-19) |
| `meta.xml` | 2 KiB | 0,7 KiB |
| `META-INF/manifest.xml` | 1 KiB | 0,4 KiB |
| `thumbnail.png` | 11 KiB | 11 KiB (STORED) |
| `previews/spread-1.png` | 42 KiB | 42 KiB (STORED) |
| Sobrecarga ZIP (5 entradas × ~110 B) | — | 0,6 KiB |
| **Total** | | **≈ 173 KiB** (portable) / **139 KiB** (compact) |
| `.xar` equivalente de referencia | | ≈ 126 KiB |

Ratio frente a `.xar`: **1,37×** en perfil portable, **1,10×** en compact. Dentro del
objetivo O3 (≤ 1,40×). El SVG sin las 8 pasadas de optimización pesaría ~2 100 KiB sin
comprimir y ~186 KiB comprimido: **las pasadas ahorran un 37 % del tamaño final**.

#### Caso B — Documento fotográfico (1 JPEG de 12 MP, 3,2 MiB, usado 8 veces con 3 recortes)

| Estrategia | Tamaño |
|---|---|
| Ingenua (8 copias incrustadas en base64, SVG deflate) | ≈ 34,1 MiB |
| Ingenua (8 copias como entradas ZIP, deflate) | ≈ 25,5 MiB |
| 8 copias como entradas ZIP, STORED | ≈ 25,6 MiB |
| **`.xarast`: 1 maestro + 3 derivadas, todo STORED** | **≈ 4,9 MiB** |
| `.xarast` con `--no-derived` (derivadas regeneradas al abrir) | ≈ 3,3 MiB |

La deduplicación aporta aquí un factor **5,2×**. Este es el caso donde el formato
*supera* al `.xar` original, que deduplicaba el bitmap maestro pero guardaba cada
variante XPE procesada.

#### Caso C — Documento con efectos vivos (blend de 40 pasos + 3 sombras + 2 contornos)

| Componente | Nota |
|---|---|
| Geometría fuente (2 objetos del blend + 5 objetos base) | 3 KiB |
| Parámetros `xarast:` de los efectos | 1,2 KiB |
| **Subárboles horneados** (40 pasos del blend + contornos como paths) | 96 KiB sin comprimir |
| `document.svg` total | 104 KiB → **9,1 KiB** comprimido |

El horneado es lo que domina el tamaño *antes* de comprimir, pero es texto extremadamente
repetitivo (40 paths casi idénticos) y deflate lo reduce ~11:1. **Alternativa normativa:**
si el subárbol horneado supera **256 KiB** sin comprimir, el escritor **DEBERÍA** sustituirlo
por una rasterización en `resources/baked/` referenciada con `<image>` (§5.4.3), cuyo
tamaño está acotado por la resolución elegida.

#### Caso D — Guardado incremental con `history/` activo (10 instantáneas)

Al no duplicarse los binarios (§3.2.8), el coste de 10 instantáneas de un documento
del Caso A es ≈ 10 × 118 KiB = 1,15 MiB, es decir **7,8× el documento base**. Por eso
`history/` está desactivado por defecto y tiene poda obligatoria. Una versión 1.1 del
formato **DEBERÍA** almacenar deltas (`history/0007/document.svg.vcdiff`) en vez de
copias completas; el nombre de entrada ya está previsto.

---

## 5. El perfil SVG de Xarast

### 5.1 El principio de doble representación

**Es la decisión arquitectónica central del formato.** Todo objeto o atributo de Xarast
se escribe en `document.svg` como la conjunción de dos cosas:

- **(a) La representación base**: SVG 1.1 estándar, autosuficiente, que cualquier
  renderizador conforme pinta razonablemente. Puede ser *exacta* (un path con relleno
  plano) o *horneada* (una aproximación generada por Xarast: 40 paths para un blend, un
  `<filter>` para una sombra, una rasterización para un relleno fractal).
- **(b) La representación paramétrica**: atributos y elementos del espacio de nombres
  `xarast:`, que describen el objeto *como lo entiende el modelo de Xarast*, con todos
  sus parámetros de edición.

Reglas normativas del principio:

1. La representación base **DEBE** existir siempre. **NO DEBE** haber nunca un objeto
   visible en Xarast que sea invisible o vacío en un renderizador SVG estándar.
2. La representación paramétrica, cuando existe, es la **fuente de verdad** para Xarast.
   Al abrir, Xarast **DEBE** reconstruir el objeto desde (b) y **DEBE** descartar y
   regenerar (a).
3. Toda subred SVG que sea puro producto del horneado **DEBE** marcarse con
   `xarast:generated="<tipo>"` en su elemento raíz. Esto le dice a Xarast «esto es
   derivado, bórralo y recalcúlalo», y le dice a una herramienta de análisis «esto no
   es contenido de autor».
4. Cuando (a) y (b) discrepan (porque un tercero editó el SVG), **manda (b)**, salvo que
   el elemento lleve `xarast:base-authoritative="true"` (§8.5).
5. Si un objeto es expresable **exactamente** en SVG estándar, el escritor **NO DEBE**
   añadir representación paramétrica redundante. Un rectángulo con relleno plano se
   escribe `<rect ... fill="#c33"/>` y nada más. La extensión se paga sólo cuando aporta.

### 5.2 Espacios de nombres

```xml
<svg xmlns="http://www.w3.org/2000/svg"
     xmlns:xlink="http://www.w3.org/1999/xlink"
     xmlns:xarast="https://xarast.org/ns/document/1.0"
     xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:cc="http://creativecommons.org/ns#"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
```

| Prefijo | URI | Obligatoriedad |
|---|---|---|
| (defecto) | `http://www.w3.org/2000/svg` | **DEBE** |
| `xarast` | `https://xarast.org/ns/document/1.0` | **DEBE** |
| `xlink` | `http://www.w3.org/1999/xlink` | **DEBE** (SVG 1.1 lo necesita para `xlink:href`) |
| `inkscape`, `sodipodi` | (ver arriba) | **DEBERÍA** (interoperabilidad con Inkscape) |
| `dc`, `cc`, `rdf` | (ver arriba) | **DEBERÍA** (metadatos en `<metadata>`) |

- El URI del espacio `xarast` **DEBE** contener la versión **mayor** del vocabulario
  (`/1.0`). Una versión 2 incompatible usaría `https://xarast.org/ns/document/2.0`;
  las adiciones compatibles dentro de la serie 1.x **NO** cambian el URI.
- El **prefijo** es convencional: un lector **DEBE** resolver por URI, no por prefijo.
- `xlink:href` y `href` (SVG2): el escritor **DEBE** emitir **ambos** en los elementos
  que los usan (`<use>`, `<image>`, `<textPath>`, `<mpath>`, referencias de gradiente),
  porque SVG 1.1 y los renderizadores antiguos sólo entienden `xlink:href` y algunos
  sanitizadores modernos sólo conservan `href`. Coste: ~15 bytes por referencia, que
  deflate reduce a casi nada.

### 5.3 Subconjunto SVG utilizado

**Perfil base: SVG 1.1 Second Edition, perfil Full.** El escritor **NO DEBE** usar
funcionalidades fuera de esta lista blanca sin marcarlas como opcionales:

**Estructura:** `svg`, `g`, `defs`, `symbol`, `use`, `switch`, `title`, `desc`,
`metadata`, `style`.
**Geometría:** `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`.
**Pintura:** `linearGradient`, `radialGradient`, `stop`, `pattern`, `marker`,
`solidColor` (SVG2 — sólo con fallback).
**Imagen:** `image`.
**Texto:** `text`, `tspan`, `textPath`.
**Recorte y máscara:** `clipPath`, `mask`.
**Filtros:** `filter` y las primitivas `feGaussianBlur`, `feOffset`, `feFlood`,
`feComposite`, `feMerge`, `feMergeNode`, `feBlend`, `feColorMatrix`,
`feComponentTransfer` (+ `feFuncR/G/B/A`), `feTurbulence`, `feDisplacementMap`,
`feSpecularLighting`, `feDiffuseLighting`, `feDistantLight`, `fePointLight`,
`feMorphology`, `feTile`, `feImage`.

**Prohibido explícitamente (el escritor NO DEBE emitirlo):**

| Elemento/mecanismo | Motivo |
|---|---|
| `<script>`, `on*` | Seguridad: un documento no debe poder ejecutar código. Un lector **DEBE** eliminarlo al abrir y avisar. |
| `<foreignObject>` | No renderizable de forma predecible; no aporta nada al modelo. |
| `<animate>`, `<set>`, `<animateTransform>`, SMIL | Fuera del modelo v1.0. |
| `<font>`, `<glyph>` (fuentes SVG) | Obsoleto y sin soporte en navegadores. Usar WOFF2. |
| `<meshgradient>`, `<hatch>` (SVG 2) | Eliminados de SVG 2 y movidos a `svg-next`; sin implementación en navegadores. Los rellenos de 3/4 colores se hornean (§6.4). |
| Referencias externas (`http(s)://`, `file://`, rutas absolutas) | §3.3 |
| Entidades XML externas, DTD, `<!ENTITY>` | XXE. El lector **DEBE** rechazar cualquier DOCTYPE con entidades. |

**Usable con fallback obligatorio (SVG 2 / CSS Compositing):**

| Funcionalidad | Fallback exigido |
|---|---|
| `style="mix-blend-mode:<modo>"` | El elemento **DEBE** además tener un `opacity`/color plausible; el modo de mezcla **DEBE** repetirse en `xarast:blend`. Soportado por todos los navegadores actuales, no por todos los visores. |
| `style="isolation:isolate"` | Sin él el render degrada, pero no rompe. |
| `href` sin `xlink:` | Siempre acompañado de `xlink:href` (§5.2). |
| `var(--...)` para colores nombrados | Sólo si `xarast:colour-refs="var"`; por defecto se emiten literales (§6.12). |
| `paint-order` | Afecta al orden relleno/trazo del texto; degrada de forma aceptable. |

### 5.4 Marcado del contenido horneado

```xml
<g xarast:generated="blend" xarast:generated-by="x:blend-7" xarast:generated-rev="3">
  <!-- 40 pasos intermedios, puro SVG estándar -->
</g>
```

| Atributo | Obligatorio | Significado |
|---|---|---|
| `xarast:generated` | **DEBE** | Tipo de generador: `blend`, `contour`, `shadow`, `bevel`, `mould`, `fill-bake`, `stroke-outline`, `text-outline`, `clone-expand`, `effect` |
| `xarast:generated-by` | **DEBE** | ID del elemento paramétrico que lo produjo |
| `xarast:generated-rev` | **DEBERÍA** | Contador monótono; permite detectar horneado obsoleto |
| `xarast:generated-hash` | **PUEDE** | BLAKE3 de los parámetros de entrada; si no coincide al abrir, se regenera |

Reglas:

1. Al abrir, Xarast **DEBE** eliminar todo subárbol con `xarast:generated` cuyo
   `xarast:generated-by` resuelva a un elemento paramétrico presente, y regenerarlo.
2. Si `xarast:generated-by` **no** resuelve (el paramétrico fue borrado por un tercero),
   Xarast **DEBE** conservar el subárbol horneado como geometría normal editable y
   avisar de que el efecto vivo se ha perdido. **Nunca** borrar arte sin sustituto.
3. Un subárbol horneado **NO DEBE** contener elementos paramétricos anidados: el horneado
   es una hoja del árbol de edición.

#### 5.4.1 Tres estrategias de horneado, por orden de preferencia

1. **Geometría equivalente** (preferida): el efecto se expresa con paths/gradientes SVG.
   Aplica a: blend, contour, mould, trazo variable, clones expandidos.
   *Ventaja:* escala sin pérdida, es editable si todo lo demás falla.
2. **Filtro SVG** (segunda opción): `<filter>` con primitivas estándar.
   Aplica a: sombra, plumeado, bisel, desenfoques, ajustes fotográficos, fractales
   (`feTurbulence` es una aproximación notablemente buena de los rellenos de nubes y
   plasma de Xara).
   *Ventaja:* muy compacto. *Riesgo:* el resultado exacto depende del renderizador.
3. **Rasterización** (último recurso): PNG en `resources/baked/` referenciado con
   `<image>`.
   Aplica a: rellenos cónicos/diamante/3-4 colores de alta calidad, efectos cuyo horneado
   geométrico superaría 256 KiB (§4.6 Caso C), y cualquier cosa nueva que SVG no exprese.
   El escritor **DEBE** rasterizar a **2× la resolución nominal del objeto en el
   documento**, con un máximo de 4096 px de lado, y **DEBE** anotar `xarast:baked-dpi`.

### 5.5 Sistema de coordenadas y unidades

- **Unidad de usuario SVG = 1 punto PostScript = 1/72 pulgada.** Se elige el punto y no
  el píxel CSS porque la unidad interna de Xara es el **milipunto** (1/1000 pt,
  `Kernel/doccoord.h`), de modo que 3 decimales en el SVG representan **exactamente** la
  precisión interna sin error de redondeo. El documento **DEBE** declararlo:

```xml
<svg width="210mm" height="297mm" viewBox="0 0 595.276 841.89" ...>
```

  `viewBox` en puntos, `width`/`height` en la unidad física real, de modo que un navegador
  y una impresora acierten el tamaño.

- **Eje Y:** SVG crece hacia abajo; Xara crece hacia arriba. El documento se almacena en
  **convención SVG nativa (Y hacia abajo)**, con el origen en la **esquina superior
  izquierda del spread**. Se rechaza explícitamente la alternativa de aplicar un
  `transform="scale(1,-1)"` global: rompe el texto, los gradientes, los filtros y la
  legibilidad. La conversión Y se hace en el importador/exportador, una sola vez.
  Xarast **DEBE** guardar en `<xarast:document xarast:y-axis="down">` para hacerlo
  explícito y permitir un futuro `up`.

- **Unidades de trabajo del usuario** (mm, cm, in, pt, px, y unidades definidas por el
  usuario con prefijo/sufijo, `TAG_DEFINE_PREFIXUSERUNIT` 85 / `TAG_DEFINE_SUFFIXUSERUNIT`
  86) son **preferencia de presentación**, viven en `meta.xml` (§7) y **NO** afectan a los
  números del SVG.

- **Precisión.** El escritor **DEBE** emitir coordenadas con hasta 3 decimales y **NO
  DEBE** emitir más. El lector **DEBE** aceptar cualquier número válido de SVG,
  incluyendo notación exponencial.

### 5.6 Criterio medible de «render razonable» (O2)

Se define un test de conformidad automatizable:

1. Se renderiza el documento con Xarast a PNG a 96 dpi → *referencia*.
2. Se extrae `document.svg` y se renderiza con **resvg**, con **Chromium headless** y con
   **Inkscape** (`--export-type=png`) → *candidatos*.
3. Se calcula SSIM de cada candidato contra la referencia.

**Umbrales normativos** para el corpus de conformidad (§13.5):

| Clase de documento | SSIM mínimo | Nota |
|---|---|---|
| Sólo geometría, rellenos planos y degradados lineales/radiales | **0,99** | Debe ser casi exacto |
| Con transparencias, recortes, máscaras, texto | **0,95** | Diferencias de rasterizado y de fuentes |
| Con efectos vivos (sombra, bisel, plumeado, contorno, blend) | **0,90** | Los filtros SVG aproximan |
| Con rellenos fractales o cónicos | **0,85** | Horneado; se acepta más desviación |
| **Media del corpus completo** | **≥ 0,90** | Es el número que cierra O2 |

Un fallo de umbral **DEBE** romper la CI.

### 5.7 Identidad de los objetos

Cada objeto del modelo **DEBE** tener un identificador persistente emitido como atributo
`id` del elemento SVG correspondiente.

- El `id` **DEBE** ser un nombre XML válido, estable a lo largo de la vida del objeto:
  no cambia al mover, reordenar, editar ni reagrupar. Sólo un objeto nuevo recibe un `id`
  nuevo.
- Formato recomendado: `x` + 11 caracteres de un alfabeto base32 en minúsculas derivados
  de un ULID/UUIDv7 truncado (`xk3m9q2vr7t`). Es corto (12 bytes), ordenable por tiempo
  de creación y colisiona con probabilidad despreciable.
- Los IDs de elementos auxiliares generados (gradientes, filtros, clips, marcadores)
  **DEBERÍAN** derivarse del hash del contenido (§4.5.1 pasada 6), lo que los hace
  deduplicables y también estables.
- Un lector que encuentre `id` duplicados **DEBE** reasignar los duplicados y avisar; un
  documento con `id` duplicados **no es conforme**.
- Los IDs son la base sobre la que se construirán en el futuro la sincronización
  incremental y la edición colaborativa (§2.5, N3).

### 5.8 Estructura del árbol del documento

```xml
<svg …>                                        <!-- documento -->
  <title>…</title>
  <desc>…</desc>
  <metadata>  <rdf:RDF>…</rdf:RDF>  </metadata>
  <defs>
    <style type="text/css">…</style>           <!-- clases de pintura (§4.5.1) -->
    <xarast:document …/>                       <!-- propiedades globales -->
    <xarast:palette …>…</xarast:palette>       <!-- colores nombrados -->
    <xarast:units …/>                          <!-- unidades de usuario -->
    <!-- gradientes, patrones, filtros, marcadores, clipPaths, symbols -->
  </defs>
  <sodipodi:namedview …>                       <!-- guías/rejilla compatibles Inkscape -->
    <sodipodi:guide …/>
  </sodipodi:namedview>

  <g id="xSPREAD1" xarast:spread="1" xarast:kind="spread" …>
     <xarast:page …/>                          <!-- geometría de las páginas del spread -->
     <g id="xL1" inkscape:groupmode="layer" inkscape:label="Fondo"
        xarast:kind="layer" …>  …objetos…  </g>
     <g id="xL2" inkscape:groupmode="layer" inkscape:label="Texto"
        xarast:kind="layer" …>  …objetos…  </g>
     <g id="xLG" inkscape:groupmode="layer" inkscape:label="Guías"
        xarast:kind="layer" xarast:layer-kind="guide" style="display:none">…</g>
  </g>

  <svg id="xSPREAD2" x="0" y="900" width="595.276" height="841.89"
       viewBox="0 0 595.276 841.89" xarast:spread="2" xarast:kind="spread">
     …
  </svg>
</svg>
```

#### 5.8.1 Páginas y spreads

El modelo de Xara es `Document → Chapter → Spread → (Page*, Layer*)`
(`TAG_DOCUMENT` 40, `TAG_CHAPTER` 41, `TAG_SPREAD` 42, `TAG_PAGE` 44, `TAG_LAYER` 43).

**Decisión:** todos los spreads viven en **un único `document.svg`**.

- El **primer** spread se emite como un `<g>` directo del `<svg>` raíz, en el origen del
  `viewBox` raíz. Consecuencia: **un navegador que abra `document.svg` muestra la primera
  página, correctamente encuadrada**. Es la degradación deseada.
- Los spreads **segundo y siguientes** se emiten como `<svg>` **anidados** con `x`, `y`,
  `width`, `height` y su propio `viewBox`, dispuestos verticalmente por debajo del
  primero con una separación de 1 unidad de página. Quedan fuera del `viewBox` raíz, por
  lo que un navegador los recorta: ve la página 1, no un collage confuso.
- Se rechazan las alternativas: (i) un SVG por spread en `spreads/` rompería la apertura
  directa en navegador y multiplicaría los `<defs>` compartidos; (ii) todos los spreads
  dentro del mismo `viewBox` produciría un render inicial ilegible.
- Los **capítulos** (`TAG_CHAPTER`) se representan como agrupación lógica en
  `<xarast:document>`, no como nivel del árbol SVG, porque no tienen geometría.
- Cada spread **DEBE** declarar sus páginas con `<xarast:page>` (posición, tamaño,
  sangrado, si es página doble, escalado del spread `TAG_SPREADSCALING_*` 52/53).

Para documentos con **más de 32 spreads o más de 8 MiB de SVG**, el escritor **PUEDE**
usar el layout partido (`xarast:layout="split"`): `document.svg` contiene el primer
spread y una lista `<xarast:spread-ref xarast:href="spreads/spread-N.svgpart"/>`; los
demás spreads viven en `spreads/`. Un lector **DEBE** soportar ambos layouts. El escritor
**DEBE** anotar `mf:profile` en consecuencia y **DEBE** avisar de que la apertura directa
en navegador sólo mostrará el primer spread.

#### 5.8.2 Capas

Una capa es un `<g>` hijo directo del spread, **nunca** anidada en otra capa.

```xml
<g id="xL2"
   inkscape:groupmode="layer" inkscape:label="Texto"
   xarast:kind="layer"
   xarast:layer-kind="normal"
   xarast:visible="true"
   xarast:locked="false"
   xarast:printable="true"
   xarast:solid="false"
   xarast:active="true"
   xarast:frame-delay="0"
   style="display:inline;opacity:1">
```

| Atributo | Fuente en Xara | SVG equivalente | Notas |
|---|---|---|---|
| `inkscape:groupmode="layer"` | — | — | Compatibilidad Inkscape; coste ~28 B |
| `inkscape:label` | `TAG_LAYERDETAILS` (48) | — | Nombre visible; **DEBE** coincidir con `xarast:label` si ambos están |
| `xarast:visible` | `TAG_LAYERDETAILS` | `style="display:inline|none"` | **DEBEN** emitirse los dos; `display` es la representación base |
| `xarast:locked` | `TAG_LAYERDETAILS` | — | SVG no tiene bloqueo; sólo `xarast:` (+ `sodipodi:insensitive="true"` para Inkscape) |
| `xarast:printable` | `TAG_LAYERDETAILS` | — | Una capa no imprimible **DEBE** seguir siendo visible en pantalla |
| `xarast:layer-kind` | `TAG_GUIDELAYERDETAILS` (49) | — | `normal`, `guide`, `background`, `frame` |
| Opacidad de capa | — | `opacity` en el `<g>` | Xara no tiene opacidad de capa nativa; Xarast la añade y usa `opacity` estándar |
| `xarast:frame-*` | `TAG_LAYER_FRAMEPROPS` (4030) | — | Animación por frames; fuera del alcance de render v1.0 |

- La **capa de guías** **DEBE** emitirse con `style="display:none"` para que no aparezca
  en un visor externo, y con `xarast:layer-kind="guide"`.
- El **orden Z** es el orden del documento: el primer `<g>` de capa se pinta primero
  (queda al fondo). Esto invierte la convención habitual de los paneles de capas
  (donde la capa de arriba está al frente): la UI **DEBE** mostrar la lista invertida.
  Se documenta aquí porque es la fuente número uno de errores en implementaciones de
  formatos gráficos (§2.3).
- El atributo `xarast:solid` corresponde al concepto de Xara de capa con fondo sólido
  que oculta las inferiores.

### 5.9 Cómo se cumple en la práctica «abrirlo en un navegador»

Conviene ser exacto, porque hay una limitación real: **un navegador no abre un ZIP**.
El requisito O2 se satisface por cuatro vías, todas normativas:

1. **Extracción.** `unzip documento.xarast -d /tmp/doc && xdg-open /tmp/doc/document.svg`.
   Como las referencias a recursos son relativas al paquete (§3.3), todo resuelve.
   Esta es la vía canónica y **DEBE** funcionar siempre.
2. **Inkscape.** Igual: se abre `document.svg` extraído. Las capas, guías y rejilla
   aparecen correctamente gracias a `inkscape:groupmode` y `sodipodi:namedview` (§5.8.2).
   Inkscape **preserva** los atributos `xarast:` desconocidos al guardar, de modo que el
   round-trip Xarast → Inkscape → Xarast conserva las extensiones (§8).
3. **`xarast extract`** (CLI): el binario de Xarast **DEBE** ofrecer
   `xarast extract doc.xarast -o dir/` y `xarast cat doc.xarast document.svg`.
4. **Exportación a SVG plano autocontenido.** Xarast **DEBE** ofrecer
   «Exportar → SVG» con tres variantes:
   - `SVG (con extensiones)`: `document.svg` tal cual, recursos como ficheros hermanos.
   - `SVG (autocontenido)`: recursos incrustados como `data:` URIs; extensiones
     `xarast:` conservadas. Un único fichero que se abre en cualquier parte.
   - `SVG (plano / Inkscape / web)`: extensiones eliminadas, horneado consolidado,
     `<style>` expandido. **Con pérdida y así etiquetado en la UI.**
   La variante autocontenida **PUEDE** además comprimirse a `.svgz`.

Adicionalmente, el escritor **DEBERÍA** incluir en la raíz del paquete una entrada
`README.txt` (≈ 700 bytes, deflate a ~350) que explique al usuario humano qué es el
fichero y cómo extraerlo. Precedente: los `.epub` bien hechos lo hacen. Es la mejor
inversión de 350 bytes del formato.

---

## 6. Tabla de mapeo completa

Notación de las tablas: **Base** = lo que se emite en SVG estándar (representación (a));
**Extensión** = lo que se emite en el espacio `xarast:` (representación (b));
**Pérdida sin (b)** = qué se degrada en un visor externo.

### 6.1 Objetos geométricos

| Concepto Xara | Tag original | Base SVG | Extensión `xarast:` | Pérdida sin (b) |
|---|---|---|---|---|
| Path (relleno / trazo / ambos) | `TAG_PATH*` 100-103, 113-116 | `<path d="…"/>` con `fill`/`stroke` | — (exacto) | Ninguna |
| Flags de path (suave, rotacional, punto final) | `TAG_PATH_FLAGS` 111 | — | `xarast:node-flags="s r e …"` (una letra por nodo) | Ninguna visual; se pierde el comportamiento de edición de nodos |
| Grupo | `TAG_GROUP` 104 | `<g>` | `xarast:kind="group"` | Ninguna |
| Grupo con transparencia | `TAG_GROUPTRANSP` 4063 | `<g opacity="…" style="isolation:isolate">` | `xarast:group-transp="true"` | Composición ligeramente distinta |
| Grupo compuesto (render en caché) | `TAG_COMPOUNDRENDER` 4128, `TAG_CACHEBMP` 4064 | `<g>` normal | `xarast:compound="true"` | Ninguna |
| Elipse simple | `TAG_ELLIPSE_SIMPLE` 1000 | `<ellipse cx cy rx ry/>` | — | Ninguna |
| Elipse compleja (rotada/sesgada) | `TAG_ELLIPSE_COMPLEX` 1001 | `<ellipse …  transform="matrix(…)"/>` | — | Ninguna |
| Rectángulo simple | `TAG_RECTANGLE_SIMPLE` 1100 | `<rect x y width height/>` | — | Ninguna |
| Rectángulo redondeado | `TAG_RECTANGLE_SIMPLE_ROUNDED` 1104 | `<rect rx="…"/>` | `xarast:corner-ratio` si el radio es proporcional | Ninguna |
| Rectángulo estrellado / reformado y las 12 variantes | 1101-1115 | `<path d="…">` horneado | `<xarast:quickshape>` (§6.2) | Deja de ser paramétrico |
| QuickShape (polígono/estrella) | `TAG_REGULAR_SHAPE_PHASE_1/2` 1900/1901 | `<path d="…">` horneado | `<xarast:quickshape>` (§6.2) | Deja de ser paramétrico |
| Línea acotada / dimensión | `TAG_DIMENSION` 4130 | `<g>` con path + texto | `<xarast:dimension>` | Deja de actualizarse |
| Dirección web sobre objeto | `TAG_WEBADDRESS` 4020 | `<a xlink:href="…">` envolviendo el objeto | `xarast:web-bbox` | Ninguna |

### 6.2 Formas paramétricas (QuickShapes)

El nodo `NodeRegularShape` de Xara (`Kernel/nodershp.h`) parametriza rectángulos,
polígonos, estrellas y elipses con un único modelo. La extensión lo refleja campo a campo:

```xml
<path id="xq4" d="M 120,20 L 149.4,80.6 …Z"
      xarast:shape="quick"
      fill="#e8a33d">
  <xarast:quickshape
      xarast:sides="5"
      xarast:circular="false"
      xarast:stellated="true"
      xarast:primary-curvature="false"
      xarast:stellation-curvature="false"
      xarast:stell-radius-ratio="0.382"
      xarast:primary-curve-ratio="0"
      xarast:stell-curve-ratio="0"
      xarast:stell-offset-ratio="0"
      xarast:centre="120 90"
      xarast:major-axis="0 -70"
      xarast:minor-axis="70 0"
      xarast:matrix="1 0 0 1 0 0"
      xarast:reformed="false"/>
</path>
```

| Atributo | Campo en `NodeRegularShape` | Tipo | Notas |
|---|---|---|---|
| `xarast:sides` | `NumSides` | entero ≥ 3 | Ignorado si `circular` |
| `xarast:circular` | `Circular` | bool | La forma se basa en una circunferencia |
| `xarast:stellated` | `Stellated` | bool | Estrella |
| `xarast:primary-curvature` | `PrimaryCurvature` | bool | Lados curvos |
| `xarast:stellation-curvature` | `StellationCurvature` | bool | Puntas curvas |
| `xarast:stell-radius-ratio` | `StellRadiusToPrimary` | double | Radio interior / exterior |
| `xarast:primary-curve-ratio` | `PrimaryCurveToPrimary` | double | Curvatura del lado |
| `xarast:stell-curve-ratio` | `StellCurveToStell` | double | Curvatura de la punta |
| `xarast:stell-offset-ratio` | `StellOffsetRatio` | double | ±0,5 = 360/N grados de giro de las puntas |
| `xarast:centre` | `UTCentrePoint` | `x y` | En coordenadas sin transformar |
| `xarast:major-axis`, `xarast:minor-axis` | `UTMajorAxes`, `UTMinorAxes` | `x y` | Vectores desde el centro |
| `xarast:matrix` | `TransformMatrix` | 6 doubles | Transformación de la forma |
| `xarast:reformed` | `IsReformed()` | bool | Los lados han sido editados manualmente |
| `<xarast:edge-path>` (hijo, ×2) | `EdgePath1`, `EdgePath2` | `d` de path | **DEBE** emitirse sólo si `reformed` |

Regla: si `xarast:reformed="true"`, la forma tiene lados editados a mano y los dos
`<xarast:edge-path>` son obligatorios; en otro caso el path se regenera desde los
parámetros y **NO DEBEN** emitirse.

### 6.3 Rellenos (fills)

Todos los rellenos llevan además, cuando aplican, los atributos comunes:

| Atributo común | Tag | Valores | Base SVG |
|---|---|---|---|
| `xarast:fill-repeat` | `TAG_FILL_REPEATING` 163 / `NONREPEATING` 164 / `REPEATINGINVERTED` 165 / `_EXTRA` 206 | `simple`, `repeat`, `reflect`, `repeat-extra` | `spreadMethod="pad|repeat|reflect"` |
| `xarast:fill-profile` | `TAG_BLENDPROFILES` 4072 y perfiles de rampa | `bias gain` (2 doubles en [-1,1]) | Se hornea en stops (§6.5) |
| `xarast:fill-effect` | `TAG_FILLEFFECT_FADE` 160 / `RAINBOW` 161 / `ALTRAINBOW` 162 | `fade`, `rainbow`, `alt-rainbow` | Interpolación en HSV horneada en stops |

| Relleno Xara | Tag | Base SVG | Extensión | Pérdida sin (b) |
|---|---|---|---|---|
| Plano | `TAG_FLATFILL` 150 (+190-192) | `fill="#rrggbb"` | — | Ninguna |
| Sin relleno | `TAG_FLATFILL_NONE` 190 | `fill="none"` | — | Ninguna |
| Lineal (2 puntos) | `TAG_LINEARFILL` 153 | `<linearGradient x1 y1 x2 y2 gradientUnits="userSpaceOnUse">` | `xarast:fill="linear"` sólo si hay perfil | Perfil de rampa |
| Lineal multietapa | `TAG_LINEARFILLMULTISTAGE` 4075 | Mismo, con N `<stop>` | idem | Perfil |
| Lineal de 3 puntos | `TAG_LINEARFILL3POINT` 4121, `…MULTISTAGE3POINT` 4122 | `<linearGradient>` + `gradientTransform` que reproduce el sesgo | `xarast:fill="linear3" xarast:p0/p1/p2` | Ninguna visual si el sesgo es afín |
| Circular | `TAG_CIRCULARFILL` 154 | `<radialGradient cx cy r fx=cx fy=cy>` | — | Perfil |
| Elíptica | `TAG_ELLIPTICALFILL` 155 | `<radialGradient>` + `gradientTransform="matrix(…)"` | — | Perfil |
| Cónica | `TAG_CONICALFILL` 156, `…MULTISTAGE` 4078 | **Horneado**: `<g>` de ≤ 96 cuñas con relleno plano, **o** `<pattern>` con `<image>` rasterizado | `<xarast:fill xarast:type="conical" xarast:centre xarast:start-angle xarast:end-angle>` + stops | Aparece facetado o como bitmap |
| Diamante / cuadrada | `TAG_SQUAREFILL` 200, `…MULTISTAGE` 4088 | **Horneado** igual que cónica, o `<radialGradient>` con `gradientTransform` rotado 45° (aproximación aceptable) | `<xarast:fill xarast:type="diamond">` | Esquinas redondeadas en vez de rectas |
| 3 colores | `TAG_THREECOLFILL` 202 | **Horneado** a `<image>` (mesh gradients fueron retirados de SVG 2) | `<xarast:fill xarast:type="three-point">` con 3 `<xarast:point x y colour>` | Se vuelve raster |
| 4 colores | `TAG_FOURCOLFILL` 204 | idem | `xarast:type="four-point"` | idem |
| Bitmap | `TAG_BITMAPFILL` 157 | `<pattern patternUnits="userSpaceOnUse" patternTransform="matrix(…)"><image …/></pattern>` | `xarast:fill="bitmap"` + `xarast:tile-mode` | Ninguna si es mosaico simple |
| Bitmap contone (duotono) | `TAG_CONTONEBITMAPFILL` 158 | `<pattern>` + `<filter>` con `feColorMatrix` de luminancia + `feComponentTransfer` que interpola start→end | `xarast:contone-start`, `xarast:contone-end` | Ligera diferencia de curva tonal |
| Fractal — nubes | `TAG_FRACTALFILL` 159 (`FILLSHAPE_CLOUDS` 9) | `<filter>` con `feTurbulence type="fractalNoise"` + `feColorMatrix` + `feComponentTransfer` mapeando a los 2 colores, aplicado a un `<rect>` recortado por la forma | `<xarast:fill xarast:type="fractal-clouds" xarast:seed xarast:graininess xarast:octaves xarast:squash xarast:dpi>` | Textura **similar pero no idéntica**; muy aceptable |
| Fractal — plasma | `TAG_FRACTALFILL` 159 (`FILLSHAPE_PLASMA` 10) | idem con `type="turbulence"` | `xarast:type="fractal-plasma"` | idem |
| Ruido | `TAG_NOISEFILL` 4010 | `feTurbulence` + `feComposite` | `xarast:type="noise"` | idem |
| Relleno de bisel | `TAG_BEVEL`-relacionado, `bevfill.cpp` | Parte del filtro de bisel (§6.8) | — | — |

**Regla de rasterización de rellenos (normativa).** Cuando el escritor hornea un relleno
a bitmap, **DEBE**:
- rasterizar al **doble** de la resolución nominal del objeto (mín. 96 dpi, máx. 4096 px
  de lado);
- guardar el PNG en `resources/baked/` (dedupicado por hash);
- referenciarlo con `<pattern>` cuyo `patternTransform` reproduzca exactamente la
  geometría del relleno;
- anotar `xarast:baked-dpi` y `xarast:generated="fill-bake"`.

### 6.4 Perfiles de rampa no lineales

Xara permite una curva *bias/gain* sobre la interpolación de cualquier degradado y sobre
la distribución de un blend o contorno. SVG sólo interpola linealmente entre `<stop>`.

**Solución:** horneado por muestreo adaptativo.

```xml
<linearGradient id="g7a3" gradientUnits="userSpaceOnUse" x1="20" y1="20" x2="220" y2="20"
                xarast:profile="0.42 -0.15">
  <stop offset="0"     stop-color="#1c3f8f"/>
  <stop offset=".0625" stop-color="#23499a"/>
  …
  <stop offset="1"     stop-color="#ffd166"/>
</linearGradient>
```

- El escritor **DEBE** emitir `xarast:profile="<bias> <gain>"` (dos doubles en [-1, 1];
  `0 0` = lineal) y **DEBE** omitirlo si es lineal.
- El escritor **DEBE** hornear la curva emitiendo stops intermedios. Número de stops:
  muestreo adaptativo hasta que el error máximo de color entre la curva real y la
  interpolación lineal por tramos sea **≤ 2/255** en cada canal. Mínimo **9**, máximo
  **33** stops por tramo entre colores clave.
- El lector **DEBE** descartar los stops intermedios cuando `xarast:profile` está
  presente y recalcularlos, para no acumular error en sucesivos guardados.
- Los stops clave de origen se marcan con `xarast:key="true"` para poder distinguirlos
  de los horneados; alternativamente el escritor **PUEDE** listar los colores clave y sus
  posiciones en `xarast:stops="0:#1c3f8f 0.5:#7a2ea0 1:#ffd166"`, que es más compacto y
  es la forma **recomendada**.

### 6.5 Transparencias y modos de mezcla

Xara modela la transparencia como un «relleno de transparencia» con la misma variedad de
formas geométricas que los rellenos de color (`TAG_*TRANSPARENTFILL`, 166-172, 180-182,
201, 203, 205, 4011), más un **tipo** de transparencia que en la práctica es un modo de
mezcla (`enum TranspType` en `Kernel/fillval.h`).

#### 6.5.1 Geometría de la transparencia

| Transparencia Xara | Tag | Base SVG | Extensión |
|---|---|---|---|
| Plana | `TAG_FLATTRANSPARENTFILL` 166 | `fill-opacity` / `stroke-opacity` / `opacity` | — |
| Lineal | `TAG_LINEARTRANSPARENTFILL` 167 | `<mask maskUnits="userSpaceOnUse">` con un `<rect>` pintado por un `<linearGradient>` de grises | `<xarast:transparency xarast:type="linear" …>` |
| Circular / elíptica | 168 / 169 | `<mask>` con `<radialGradient>` de grises | `xarast:type="circular|elliptical"` |
| Cónica / cuadrada / 3-4 col. | 170 / 201 / 203 / 205 | `<mask>` con el mismo horneado que el relleno equivalente | idem |
| Bitmap | `TAG_BITMAPTRANSPARENTFILL` 171 | `<mask>` con `<pattern>` de la imagen en escala de grises | `xarast:type="bitmap"` |
| Fractal | `TAG_FRACTALTRANSPARENTFILL` 172 | `<mask>` con `feTurbulence` | `xarast:type="fractal-*"` |
| Transparencia de línea | `TAG_LINETRANSPARENCY` 173 | `stroke-opacity` | — |
| Transparencia de 3 puntos | `TAG_LINEARTRANSPARENTFILL3POINT` 4123 | `<mask>` + `gradientTransform` | `xarast:type="linear3"` |

**Nota importante sobre las máscaras.** El valor de luminancia del `<mask>` en SVG 1.1 se
calcula con la fórmula de luminancia lineal (`linearRGB` por defecto). El escritor
**DEBE** emitir `color-interpolation="sRGB"` en el `<mask>` y en el gradiente para que el
resultado coincida con el modelo de Xara (alfa directo, sin conversión de gamma). Omitir
esto produce transparencias visiblemente distintas en navegador. Es un error clásico.

#### 6.5.2 Tipos de transparencia → modos de mezcla

`TranspType` en Xara (`Kernel/fillval.h`) incluye: `Mix`, `StainGlass`, `Bleach`,
`Contrast`, `Saturation`, `Darken`, `Lighten`, `Brightness`, `Luminosity`, `Hue`, `Bevel`
y tres modos «especiales» (aditivo, sustractivo, tabla de consulta).

| `TranspType` | Valor | Base SVG/CSS | Precisión del fallback | Extensión |
|---|---|---|---|---|
| `TT_Mix` | 1 | alfa normal (`opacity`) | **Exacta** | — (por defecto) |
| `TT_StainGlass` | 2 | `style="mix-blend-mode:multiply"` | **Exacta** | `xarast:blend="stained-glass"` |
| `TT_Bleach` | 3 | `style="mix-blend-mode:screen"` | **Exacta** | `xarast:blend="bleach"` |
| `TT_DARKEN` | — | `mix-blend-mode:darken` | Exacta | `xarast:blend="darken"` |
| `TT_LIGHTEN` | — | `mix-blend-mode:lighten` | Exacta | `xarast:blend="lighten"` |
| `TT_SATURATION` | — | `mix-blend-mode:saturation` | Aproximada (modelo HSL de CSS vs HSV de Xara) | `xarast:blend="saturation"` |
| `TT_LUMINOSITY` | — | `mix-blend-mode:luminosity` | Aproximada | `xarast:blend="luminosity"` |
| `TT_HUE` | — | `mix-blend-mode:hue` | Aproximada | `xarast:blend="hue"` |
| `TT_CONTRAST` | — | `<filter>` con `feComponentTransfer type="linear"` alrededor del punto medio | Aproximada | `xarast:blend="contrast" xarast:amount="…"` |
| `TT_BRIGHTNESS` | — | `<filter>` con `feComponentTransfer type="linear" intercept` | Aproximada | `xarast:blend="brightness"` |
| `TT_BEVEL` | — | Se resuelve como parte del bisel (§6.8) | — | `xarast:blend="bevel"` |
| Especial aditivo | `T_SPECIAL_1` | `mix-blend-mode:plus-lighter` | Buena | `xarast:blend="additive"` |
| Especial sustractivo | `T_SPECIAL_2` | `mix-blend-mode:multiply` (aprox.) | Pobre | `xarast:blend="subtractive"` |
| Tabla de consulta | `T_SPECIAL_3` | `<filter>` con `feComponentTransfer type="table"` y la tabla incluida | Buena | `xarast:blend="lut"` + `<xarast:lut>` |

Reglas:

- El escritor **DEBE** emitir siempre `xarast:blend` cuando el modo no sea `Mix`, aunque
  exista un `mix-blend-mode` exacto: el nombre canónico del modo es el de Xarast y así el
  lector no depende de parsear CSS.
- El escritor **DEBE** emitir el `mix-blend-mode` dentro de `style=` y **NO** como atributo
  de presentación, porque sólo es propiedad CSS.
- Todo grupo que contenga hijos con `mix-blend-mode` **DEBERÍA** llevar
  `style="isolation:isolate"` para acotar la mezcla al grupo, replicando la semántica de
  grupo de transparencia de Xara (`TAG_GROUPTRANSP` 4063).

### 6.6 Trazo (stroke)

| Concepto | Tag | Base SVG | Extensión | Pérdida |
|---|---|---|---|---|
| Color de línea | `TAG_LINECOLOUR` 151 (+193-195) | `stroke="#rrggbb"` / `none` | — | — |
| Grosor | — | `stroke-width` | — | — |
| Guiones | — | `stroke-dasharray`, `stroke-dashoffset` | `xarast:dash-name="…"` (nombre del patrón de la galería) | Se pierde el nombre, no el aspecto |
| Extremos | — | `stroke-linecap="butt|round|square"` | — | — |
| Uniones | — | `stroke-linejoin="miter|round|bevel"`, `stroke-miterlimit` | — | — |
| Trazo escalable con el objeto | — | — | `xarast:stroke-scales="true"` | El grosor no se reescala al transformar |
| Flechas / terminadores | `arrows.cpp` | `<marker>` + `marker-start`/`marker-end` | `xarast:arrow-start="<nombre>"`, `xarast:arrow-end`, `xarast:arrow-scale-with-width` | Se pierde el nombre y el reescalado automático |
| Trazo de anchura variable | `TAG_VARIABLEWIDTHFUNC` 4000, `TAG_VARIABLEWIDTHTABLE` 4001 | **Horneado**: `<path>` cerrado con `fill` (el contorno del trazo), `stroke="none"`, `xarast:generated="stroke-outline"` | `<xarast:stroke xarast:type="variable">` + `<xarast:width-table>` (lista de `t:w` separados por espacio) | Deja de ser editable como trazo |
| Tipo de trazo / definición | `TAG_STROKETYPE` 4002, `TAG_STROKEDEFINITION` 4003 | — | `xarast:stroke-type="<id>"`, definición en `resources/brushes/` | — |
| Aerógrafo | `TAG_STROKEAIRBRUSH` 4004 | Horneado a `<image>` o a filtro | `xarast:stroke-type="airbrush"` + parámetros | Se vuelve raster |
| Pincel (brush) | `TAG_BRUSHATTR` 4079, `TAG_BRUSHDEFINITION` 4080, `TAG_BRUSHDATA` 4081 … 4113 | **Horneado**: `<g xarast:generated="stroke-outline">` con las instancias del pincel a lo largo del path | `<xarast:brush>` con referencia a `resources/brushes/b3-….xml`, más los datos de espaciado, rotación, escalado, aleatoriedad y color | Deja de ser un pincel vivo |
| Presión de tableta | `TAG_BRUSHPRESSUREINFO` 4105, `TAG_BRUSHPRESSUREDATA` 4106, `TAG_BRUSHPRESSURESAMPLEDATA` 4109 | (incluido en el horneado) | `<xarast:pressure>` con las muestras en base64 (u16 LE normalizadas) o lista de decimales | — |
| Sobreimpresión de línea/relleno | `TAG_OVERPRINTLINEON` 3500 … 3503 | — | `xarast:overprint-stroke="true"`, `xarast:overprint-fill="true"` | Sólo afecta a separación de color |
| Imprimir en todas las planchas | `TAG_PRINTONALLPLATESON` 3504 | — | `xarast:all-plates="true"` | idem |

Ejemplo de trazo variable:

```xml
<g xarast:kind="stroke-variable" id="xv9">
  <xarast:stroke xarast:type="variable" xarast:base-width="4.5"
                 xarast:profile="0.2 0">
    <xarast:width-table>0:0.1 0.15:0.7 0.5:1 0.85:0.7 1:0.1</xarast:width-table>
    <xarast:source-path d="M 10,50 C 60,10 140,90 190,50"/>
  </xarast:stroke>
  <path xarast:generated="stroke-outline" xarast:generated-by="xv9"
        d="M 10,49.8 C …Z" fill="#222"/>
</g>
```

### 6.7 Texto

El modelo de texto de Xara (`TAG_TEXT_STORY_*` 2100-2117, `TAG_TEXT_LINE` 2200,
`TAG_TEXT_CHAR` 2202, atributos 2900-2920, y las extensiones de XaraLX 0.6:
`TAG_TEXT_TAB` 4200, indentación 4201-4204, historias enlazadas 4205-4207).

| Concepto | Tag | Base SVG | Extensión | Pérdida |
|---|---|---|---|---|
| Texto simple (una línea) | `TAG_TEXT_STORY_SIMPLE` 2100 | `<text x y>…</text>` | `xarast:story="simple"` | Ninguna |
| Texto de párrafo / área | `TAG_TEXT_STORY_COMPLEX` 2101 | `<text>` con un `<tspan x y>` **por línea**, con posiciones horneadas | `<xarast:text-area>` con la forma contenedora, las indentaciones y el reflujo | El texto deja de refluir al editar |
| Texto en trazado | 2110-2117 | `<textPath xlink:href="#p">` (SVG 1.1 lo soporta) | `<xarast:text-path xarast:start-offset xarast:side xarast:reverse xarast:justify-along>` | Diferencias de colocación entre renderizadores |
| Historias enlazadas | `TAG_TEXT_STORY_LINK_INFO` 4206 | Cada historia es un `<text>` independiente | `xarast:story-next="#idSiguiente"`, `xarast:story-prev` | El reflujo entre marcos se pierde |
| Justificación | 2902-2905 | `text-anchor="start|middle|end"`; justificado completo **horneado** por palabra | `xarast:justify="left|centre|right|full"` | El justificado completo se congela |
| Tamaño de fuente | `TAG_TEXT_FONT_SIZE` 2906 | `font-size` (en pt) | — | — |
| Tipografía | `TAG_TEXT_FONT_TYPEFACE` 2907 | `font-family="'Nombre', <fallbacks>"` | `xarast:font-id`, `xarast:font-ref="resources/fonts/b3-….woff2"` | Sustitución de fuente |
| Negrita / cursiva | 2908-2911 | `font-weight`, `font-style` | — | — |
| Subrayado | 2912/2913 | `text-decoration="underline"` | — | — |
| Super/subíndice, script explícito | 2914-2917 | `<tspan baseline-shift="…" font-size="…">` | `xarast:script="super|sub|explicit"` + offset y tamaño | — |
| Tracking | `TAG_TEXT_TRACKING` 2918 | `letter-spacing` (convertido de milésimas de em a pt) | `xarast:tracking="<milésimas de em>"` | Redondeo |
| Relación de aspecto | `TAG_TEXT_ASPECT_RATIO` 2919 | Posiciones por glifo horneadas en `x="…"` del `<tspan>` | `xarast:aspect="1.2"` | — |
| Desplazamiento de línea base | `TAG_TEXT_BASELINE` 2920 | `baseline-shift` o `dy` | — | — |
| Interlineado | 2900 (ratio) / 2901 (absoluto) | `y`/`dy` explícitos por línea | `xarast:line-spacing="ratio:1.2"` o `"abs:14pt"` | Se congela |
| Kerning manual | `TAG_TEXT_KERN` 2204 | `dx` en el `<tspan>` | — | — |
| Tabuladores y regla | `TAG_TEXT_TAB` 4200, `TAG_TEXT_RULER` 4204 | Posiciones horneadas | `<xarast:tabs>` y `<xarast:ruler>` | Se congelan |
| Sangrías | 4201-4203 | Horneadas en `x` | `xarast:indent-left/first/right` | Se congelan |
| Texto convertido a curvas | — | `<path>` | `xarast:was-text="true"` + `<xarast:text-source>` con el texto original (accesibilidad y búsqueda) | — |

**Fuentes.** Reglas normativas:

1. El escritor **DEBE** registrar siempre familia, peso, estilo y un `xarast:font-id`
   estable.
2. El escritor **DEBERÍA** incrustar un **subconjunto WOFF2** de cada fuente usada en
   `resources/fonts/`, y **DEBE** emitir la regla `@font-face` correspondiente en el
   `<style>` de `<defs>`, de modo que un navegador que abra el SVG extraído renderice con
   la tipografía correcta.
3. El escritor **DEBE** respetar la licencia: si la fuente prohíbe la incrustación
   (bit `fsType` restrictivo en el `OS/2`), **NO DEBE** incrustarla y **DEBE** anotar
   `xarast:font-embed="denied"`.
4. El escritor **DEBE** emitir una cadena de fallback genérica
   (`font-family="'Xara Sans', 'DejaVu Sans', sans-serif"`).
5. En modo «archivo» (perfil de conformidad C, §14.2) el escritor **DEBE** además emitir
   una copia del texto convertido a curvas dentro de un
   `<g xarast:generated="text-outline" style="display:none">`, activable por el lector si
   la fuente no está disponible. Coste alto en tamaño; por eso es un perfil aparte.

### 6.8 Efectos vivos

Este es el grupo donde el principio de doble representación se gana el sueldo. En todos
los casos: el elemento paramétrico `<xarast:*>` vive junto al objeto fuente, y la
representación horneada vive en un hermano marcado con `xarast:generated`.

#### 6.8.1 Sombra (`TAG_SHADOWCONTROLLER` 4050, `TAG_SHADOW` 4051)

```xml
<g id="xs3" xarast:kind="shadow-group">
  <xarast:shadow xarast:type="wall" xarast:blur="6.2" xarast:offset="8 8"
                 xarast:angle="315" xarast:darkness="0.55" xarast:scale="1"
                 xarast:colour="#000000" xarast:penumbra="4"/>
  <g xarast:generated="shadow" xarast:generated-by="xs3"
     filter="url(#fsh3)" opacity="0.55">
    <use xlink:href="#xobj7" href="#xobj7"/>
  </g>
  <g id="xobj7"> <!-- el objeto real --> </g>
</g>
```

Con el filtro:

```xml
<filter id="fsh3" x="-30%" y="-30%" width="180%" height="180%"
        color-interpolation-filters="sRGB">
  <feGaussianBlur in="SourceAlpha" stdDeviation="3.1"/>
  <feOffset dx="8" dy="8"/>
  <feFlood flood-color="#000000"/>
  <feComposite in2="SourceAlpha" operator="in"/>
</filter>
```

| Tipo | `xarast:type` | Horneado |
|---|---|---|
| Sombra de pared | `wall` | Filtro (desplazamiento + desenfoque) |
| Sombra de suelo | `floor` | Copia transformada con perspectiva/sesgado + filtro. **DEBE** emitirse el `transform` completo |
| Resplandor | `glow` | Filtro sin desplazamiento, con `feMorphology` opcional |
| Sombra interior | `inner` | Filtro con `feComposite operator="out"` |

`color-interpolation-filters="sRGB"` es **obligatorio** en todos los filtros emitidos:
el valor por defecto de SVG es `linearRGB`, que produce desenfoques y sombras
visiblemente distintos a los de Xara.

#### 6.8.2 Bisel (`TAG_BEVEL` 4052, atributos `TAG_BEVATTR_*` 4053-4056, `TAG_BEVELINK` 4057)

```xml
<xarast:bevel xarast:type="round" xarast:indent="6" xarast:light-angle="135"
              xarast:light-elevation="45" xarast:contrast="0.5"
              xarast:direction="outer" xarast:join="round"
              xarast:light-colour="#ffffff" xarast:shadow-colour="#000000"/>
```

Horneado: `<filter>` con la cadena clásica
`feGaussianBlur` (sobre `SourceAlpha`, radio = indent) →
`feSpecularLighting` con `feDistantLight azimuth elevation` →
`feComposite operator="in"` →
`feComposite operator="arithmetic"` para componer luz y sombra sobre `SourceGraphic`.
Cuando el bisel lleva su propio relleno (`bevfill.cpp`), el escritor **DEBE** además
hornear el «inking node» (`TAG_BEVELINK`) como un `<path>` hermano.

| `xarast:type` | Correspondencia en Xara |
|---|---|
| `round` | Rounded |
| `flat` | Flat |
| `chisel` | Chisel / Angled |
| `ridge` | Ridge |
| `mesa` | Mesa / Plateau |

#### 6.8.3 Contorno (`TAG_CONTOURCONTROLLER` 4066, `TAG_CONTOUR` 4067)

```xml
<g id="xc5" xarast:kind="contour">
  <xarast:contour xarast:width="4" xarast:steps="6" xarast:direction="outer"
                  xarast:join="round" xarast:profile="0.3 0"
                  xarast:colour-profile="0 0" xarast:insets="false"/>
  <g xarast:generated="contour" xarast:generated-by="xc5">
    <path d="…"/> <!-- paso 6, el más externo, pintado primero -->
    …
  </g>
  <path id="xc5src" d="…"/>   <!-- objeto fuente -->
</g>
```

Horneado obligatorio como paths desplazados reales (estrategia 1 de §5.4.1). **NO
DEBERÍA** usarse `stroke-width` creciente como aproximación: falla en las uniones y en
los contornos internos.

#### 6.8.4 Blend (`TAG_BLEND` 105, `TAG_BLENDER` 106, `TAG_BLEND_PATH` 4061, `TAG_BLENDPROFILES` 4072, `TAG_BLENDERADDITIONAL` 4073, `TAG_BLENDER_CURVEPROP` 4060, `TAG_BLENDER_CURVEANGLES` 4062, `TAG_NODEBLENDPATH_FILLED` 4074)

```xml
<g id="xb2" xarast:kind="blend">
  <xarast:blend xarast:steps="24"
                xarast:from="#xb2a" xarast:to="#xb2b"
                xarast:position-profile="0.15 0"
                xarast:attribute-profile="0 0"
                xarast:one-to-one="false"
                xarast:antialias="true"
                xarast:path="#xb2path"
                xarast:rotate-along-path="true"
                xarast:start-angle="0" xarast:end-angle="90"
                xarast:tangential="true"/>
  <defs>
    <path id="xb2path" d="M 20,200 C 120,40 260,40 360,200"/>
  </defs>
  <g xarast:generated="blend" xarast:generated-by="xb2">
    <path d="…"/>   <!-- 24 pasos intermedios -->
  </g>
  <path id="xb2a" d="…" fill="#e33"/>
  <path id="xb2b" d="…" fill="#33e"/>
</g>
```

- Los objetos inicial y final **DEBEN** emitirse como elementos normales y visibles: son
  arte del usuario, no horneado.
- Los pasos intermedios **DEBEN** ir en un `<g xarast:generated="blend">`.
- Para blends de muchos pasos (> 64) el escritor **DEBERÍA** aplicar la regla de
  rasterización de §4.6 (Caso C).
- Un blend «de uno a uno» entre grupos (`one-to-one`) **DEBE** anotar la correspondencia
  de sub-objetos con `<xarast:blend-map>` para que el efecto se reconstruya exactamente.

#### 6.8.5 Moulds (`TAG_MOULD_ENVELOPE` 107, `TAG_MOULD_PERSPECTIVE` 108, `TAG_MOULD_GROUP` 109, `TAG_MOULD_PATH` 110, `TAG_MOULD_BOUNDS` 4012)

```xml
<g id="xm1" xarast:kind="mould">
  <xarast:mould xarast:type="envelope" xarast:bounds="0 0 200 120">
    <xarast:mould-shape d="M 0,0 C 60,-30 140,30 200,0 L 200,120 C 140,150 60,90 0,120 Z"/>
    <xarast:mould-source>
      <!-- el subárbol ORIGINAL, sin deformar, para poder seguir editándolo -->
      <g> <path d="…"/> <text …>…</text> </g>
    </xarast:mould-source>
  </xarast:mould>
  <g xarast:generated="mould" xarast:generated-by="xm1">
    <!-- geometría deformada: paths con los nodos ya transformados -->
  </g>
</g>
```

- El moldeado **DEBE** almacenar el **origen sin deformar** dentro de
  `<xarast:mould-source>`: es la única forma de que el efecto siga siendo reversible.
  Esto duplica esa geometría, y es un coste aceptado y acotado (el origen suele ser mucho
  más simple que el resultado).
- `<xarast:mould-source>` **NO DEBE** renderizarse: va dentro de un elemento de espacio de
  nombres ajeno, que los renderizadores SVG ignoran por completo. No requiere
  `display:none`.
- Para `xarast:type="perspective"` el horneado de un subárbol **rectilíneo** **PUEDE**
  reducirse a un único `transform="matrix(…)"` cuando la perspectiva degenere en afín.
- Para texto bajo un mould, el horneado **DEBE** convertir el texto a curvas (no hay
  forma de deformar `<text>` en SVG); el texto original queda en `<xarast:mould-source>`.

#### 6.8.6 Plumeado (`TAG_FEATHER` 4086, `TAG_FEATHER_EFFECT` 4127)

```xml
<xarast:feather xarast:width="5" xarast:profile="0 0"/>
```
Horneado: `filter` con `feGaussianBlur` sobre el canal alfa
(`feColorMatrix type="matrix"` que aísla alfa) + `feComposite`. El perfil se hornea con
`feComponentTransfer type="table"` sobre `feFuncA`.

#### 6.8.7 Efectos vivos genéricos (`TAG_LIVE_EFFECT` 4125, `TAG_LOCKED_EFFECT` 4126)

Xara delegaba estos efectos en plugins externos. En Xarast se modelan como una **cadena
de efectos** declarativa:

```xml
<xarast:effects>
  <xarast:effect xarast:id="blur1" xarast:kind="gaussian-blur" xarast:radius="3"/>
  <xarast:effect xarast:id="lv1" xarast:kind="levels"
                 xarast:black="0.05" xarast:white="0.92" xarast:gamma="1.1"/>
</xarast:effects>
```
- Cada `xarast:kind` conocido **DEBE** tener un horneado a filtro SVG definido en el
  registro de efectos (§14.3).
- Un `xarast:kind` **desconocido** para el lector **DEBE** preservarse (§8) y su horneado
  **DEBE** conservarse tal cual (el lector no puede regenerarlo, así que
  `xarast:generated` se trata como autoritativo: se emite además
  `xarast:base-authoritative="true"`).
- Un efecto «bloqueado» (`TAG_LOCKED_EFFECT`) se marca con `xarast:locked="true"`: su
  horneado no se regenera nunca.

### 6.9 Bitmaps y fotografía

| Concepto | Tag | Base SVG | Extensión |
|---|---|---|---|
| Nodo bitmap | `TAG_NODE_BITMAP` 198 | `<image xlink:href="resources/images/b3-….png" x y width height preserveAspectRatio="none" transform="matrix(…)"/>` | `xarast:bitmap-id="b3-…"` |
| Bitmap duotono | `TAG_NODE_CONTONEDBITMAP` 199 | `<image>` + `filter` de duotono | `xarast:contone-start/end` |
| Definición de bitmap | `TAG_DEFINEBITMAP_*` 65-71, 4138 | Entrada en `resources/images/` | `mf:digest` en el manifiesto |
| Propiedades del bitmap | `TAG_BITMAP_PROPERTIES` 4115 | — | `<xarast:bitmap-props xarast:dpi xarast:interpolate xarast:transparent-index>` |
| Suavizado del documento | `TAG_DOCUMENTBITMAPSMOOTHING` 4116 | `image-rendering="auto|pixelated"` | `xarast:bitmap-smoothing` |
| Proceso no destructivo (XPE) | `TAG_XPE_BITMAP_PROPERTIES` 4117, `TAG_DEFINEBITMAP_XPE` 4118 | `<image>` a la derivada **o** `<image>` al maestro + `filter` | `<xarast:photo-ops>` (cadena de operaciones) + `mf:derived-from` |
| Sonido embebido | `TAG_DEFINESOUND_WAV` 70 | — | `resources/blobs/` + `<xarast:media>` (no reproducido en v1.0) |

```xml
<image id="xi4" xlink:href="resources/derived/b3-8f2c….jpg" href="resources/derived/b3-8f2c….jpg"
       x="0" y="0" width="240" height="160" preserveAspectRatio="none"
       transform="matrix(1,0,0,1,60,40)"
       xarast:bitmap-id="b3-4a91c0de5f73b8a2">
  <xarast:photo-ops xarast:master="resources/images/b3-4a91c0de5f73b8a2.jpg">
    <xarast:op xarast:kind="crop" xarast:rect="120 80 1800 1200"/>
    <xarast:op xarast:kind="brightness-contrast" xarast:brightness="0.08" xarast:contrast="0.15"/>
    <xarast:op xarast:kind="unsharp" xarast:radius="1.2" xarast:amount="0.4"/>
  </xarast:photo-ops>
</image>
```

Este es el mecanismo que reproduce la eficiencia de Xara con bitmaps: **el maestro se
guarda una vez**, las variantes se describen con parámetros, y sólo se materializa una
derivada cuando el coste de regenerarla al abrir sería alto (§4.4).

### 6.10 Recorte y máscaras

| Concepto | Tag | Base SVG | Extensión |
|---|---|---|---|
| ClipView (recorte vivo) | `TAG_CLIPVIEWCONTROLLER` 4084, `TAG_CLIPVIEW` 4085, `TAG_CLIPVIEW_PATH` 4137 | `<g clip-path="url(#cpN)">` + `<clipPath id="cpN">` con el path | `xarast:clipview="true"`, `xarast:clip-shape="#…"`, `xarast:clip-keep-shape="true"` |
| Recorte por trazado (estático) | — | `clip-path` | — |
| Máscara por transparencia | ver §6.5 | `mask` | `<xarast:transparency>` |
| Regla de relleno | — | `fill-rule="nonzero|evenodd"`, `clip-rule` | — |

El ClipView de Xara conserva la forma recortadora como objeto editable. Por eso el
escritor **DEBE** emitir la forma **dos veces**: dentro del `<clipPath>` (donde no es
visible ni editable) y —si `xarast:clip-keep-shape="true"`— como un elemento con
`id` referenciado, para que Xarast lo restaure como objeto de primera clase. La segunda
copia **DEBERÍA** implementarse con `<use>` desde el `<clipPath>` para no duplicar el `d`.

### 6.11 Clones, símbolos y conjuntos

| Concepto | Tag | Base SVG | Extensión |
|---|---|---|---|
| Clon vivo (cambia con el original) | — | `<use xlink:href="#xorig" transform="…"/>` | `xarast:clone-of="#xorig"`, `xarast:clone-mode="live"` |
| Copia independiente | — | Subárbol duplicado | — (no es un clon) |
| Símbolo reutilizable | — | `<symbol id="…">` en `<defs>` + `<use>` | `xarast:symbol="true"` |
| Clon con atributos propios | — | `<use>` con `fill`/`stroke` que sobrescriben | `xarast:clone-override="fill stroke"` |
| Conjunto (Set) | `TAG_SETSENTINEL` 4070, `TAG_SETPROPERTY` 4071 | — | `<xarast:set xarast:id="…" xarast:members="#a #b #c">` en `<defs>` |
| Nombre de objeto (Name Gallery) | `TAG_NAMEGAL_DOCCOMP` 95 | `<title>` dentro del elemento | `xarast:names="boton fondo destacado"` (lista separada por espacios) |
| Desplazamiento de duplicado | `TAG_DUPLICATIONOFFSET` 4124 | — | `xarast:duplicate-offset="10 10"` en `<xarast:document>` |
| Estilo/plantilla (WizOp) | `TAG_WIZOP` 4040, `TAG_WIZOP_STYLE` 4041, `TAG_WIZOP_STYLEREF` 4042 | — | `<xarast:style-def>` / `xarast:style-ref` |
| Propiedad de barra | `TAG_BARPROPERTY` 4087 | — | `<xarast:bar>` |

`<title>` merece una nota: es la forma estándar y accesible de nombrar un objeto en SVG
(los lectores de pantalla lo anuncian, los navegadores lo muestran como tooltip). El
escritor **DEBERÍA** emitir `<title>` con el nombre principal del objeto **además** de
`xarast:names`, porque cuesta poco y mejora la accesibilidad del SVG exportado.

### 6.12 Color: paletas, colores nombrados, CMYK y tintas planas

Xara distingue color **RGB directo** (`TAG_DEFINERGBCOLOUR` 50) de color **complejo**
(`TAG_DEFINECOMPLEXCOLOUR` 51), que puede ser CMYK, HSV, una tinta plana, o un color
**derivado** de otro (tinte, sombra, o enlazado).

#### 6.12.1 La paleta del documento

```xml
<xarast:palette xarast:id="doc">
  <xarast:colour xarast:id="c-cielo" xarast:name="Cielo"
                 xarast:model="rgb" xarast:srgb="#3388cc"/>
  <xarast:colour xarast:id="c-cielo-50" xarast:name="Cielo 50%"
                 xarast:model="tint" xarast:parent="#c-cielo" xarast:amount="0.5"
                 xarast:srgb="#99c3e5"/>
  <xarast:colour xarast:id="c-rojo-corp" xarast:name="Rojo corporativo"
                 xarast:model="cmyk" xarast:cmyk="0 0.91 0.76 0.06"
                 xarast:srgb="#d21f35"
                 xarast:profile="resources/profiles/b3-1122….icc"/>
  <xarast:colour xarast:id="c-pantone" xarast:name="PANTONE 485 C"
                 xarast:model="spot" xarast:spot-name="PANTONE 485 C"
                 xarast:cmyk="0 0.95 1 0" xarast:srgb="#da291c"
                 xarast:screen-angle="45" xarast:solid-ink="true"/>
</xarast:palette>
```

| `xarast:model` | Significado | Atributos obligatorios |
|---|---|---|
| `rgb` | sRGB directo | `xarast:srgb` |
| `cmyk` | CMYK (con o sin perfil) | `xarast:cmyk`, `xarast:srgb` |
| `hsv` | HSV | `xarast:hsv`, `xarast:srgb` |
| `grey` | Escala de grises | `xarast:grey`, `xarast:srgb` |
| `spot` | Tinta plana | `xarast:spot-name`, `xarast:srgb` |
| `tint` | Tinte de un padre hacia blanco | `xarast:parent`, `xarast:amount`, `xarast:srgb` |
| `shade` | Sombra de un padre hacia negro | `xarast:parent`, `xarast:amount`, `xarast:srgb` |
| `linked` | Derivado por desplazamiento en HSV | `xarast:parent`, `xarast:hsv-delta`, `xarast:srgb` |

**`xarast:srgb` es obligatorio en todos los casos**: es el valor que hace que la
representación base funcione, y el que permite a cualquier lector pintar algo razonable
sin entender el modelo de color.

#### 6.12.2 Referencia desde los objetos

Dos modos, seleccionables con `xarast:colour-refs` en `<xarast:document>`:

- **`literal` (por defecto, recomendado):**
  ```xml
  <path d="…" fill="#3388cc" xarast:fill-ref="#c-cielo"/>
  ```
  Máxima compatibilidad: cualquier renderizador pinta el color correcto; Xarast
  reconstruye el enlace a la paleta desde `xarast:fill-ref`. Al cambiar el color de la
  paleta hay que reescribir todos los literales, lo que es barato y determinista.

- **`var` (opcional):**
  ```xml
  <style>:root{--c-cielo:#3388cc;--c-cielo-50:#99c3e5}</style>
  …
  <path d="…" fill="var(--c-cielo, #3388cc)"/>
  ```
  Los navegadores modernos lo resuelven; algunos visores e Inkscape sólo parcialmente,
  pero el valor de reserva (`, #3388cc`) los cubre. Se ofrece porque hace el fichero
  mucho más editable a mano, pero **no** es el modo por defecto.

#### 6.12.3 CMYK y perfiles ICC

SVG 1.1 define la sintaxis `icc-color()`, que los renderizadores que no la soportan
**ignoran**, quedándose con el color RGB que la precede. Es exactamente el mecanismo de
degradación que se necesita:

```xml
<path d="…" fill="#d21f35 icc-color(coated, 0 0.91 0.76 0.06)"
      xarast:fill-ref="#c-rojo-corp"/>
```
con `<color-profile name="coated" xlink:href="resources/profiles/b3-1122….icc"/>` en
`<defs>`.

- El escritor **DEBERÍA** emitir la sintaxis `icc-color()` cuando el color tenga
  componentes CMYK y exista un perfil.
- El escritor **DEBE**, en todo caso, emitir `xarast:fill-ref` o los componentes CMYK en
  `xarast:cmyk` sobre el propio elemento si el color no está en la paleta: la forma
  `icc-color()` es frágil frente a sanitizadores.
- Planchas de color e imagesetting (`TAG_COLOURPLATE` 3508, `TAG_IMAGESETTING` 3507,
  `TAG_PRINTERSETTINGS` 3506/4135, marcas de registro 3509/3510) se almacenan en
  `meta.xml` bajo `<xarast:print>`, no en el SVG: no afectan al render en pantalla.

### 6.13 Resumen: qué se pierde en un visor externo

| Se ve **igual** | Se ve **aproximado** | Se ve **distinto pero razonable** |
|---|---|---|
| Paths, formas, grupos, capas, orden Z | Sombras, biselados, plumeado (filtros SVG) | Rellenos cónicos y de 3/4 colores (facetado o raster) |
| Rellenos planos, lineales, radiales, elípticos | Perfiles de rampa (horneados, error ≤ 2/255) | Rellenos fractales (`feTurbulence` ≠ el fractal de Xara) |
| Bitmaps, patrones, recortes, máscaras | Modos de mezcla HSL (saturación, matiz, luminosidad) | Texto sin la fuente incrustada |
| Transparencias planas y graduadas | Contornos y blends (horneados exactos, pero no vivos) | Sobreimpresión, tintas planas, CMYK (se ve el sRGB) |
| Trazos, guiones, extremos, uniones, flechas | Trazo variable y pinceles (horneados a relleno) | — |
| Texto con fuente incrustada, texto en trazado | Moulds (horneados exactos, no vivos) | — |

---

## 7. Metadatos del documento

### 7.1 Ubicación y autoridad

Los metadatos viven en **`meta.xml`**, que es la **fuente autoritativa**. Un subconjunto
se **duplica** en `document.svg` dentro de `<metadata><rdf:RDF>` en formato Dublin Core,
para que el SVG extraído siga siendo autodescriptivo (es lo que hace Inkscape, y permite
que herramientas como `exiftool` lo lean).

En caso de conflicto entre `meta.xml` y el `<metadata>` del SVG, **manda `meta.xml`**.
El escritor **DEBE** mantenerlos sincronizados al guardar.

### 7.2 Contenido de `meta.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<xarast:meta xmlns:xarast="https://xarast.org/ns/document/1.0"
             xmlns:dc="http://purl.org/dc/elements/1.1/"
             xarast:version="1.0" xarast:min-reader="1.0">

  <xarast:identity>
    <xarast:doc-id>01J9Q7ZB2K4M8N6P3R5T7V9W1X</xarast:doc-id>
    <xarast:revision>47</xarast:revision>
  </xarast:identity>

  <dc:title>Cartel de la exposición</dc:title>
  <dc:creator>Ada Lovelace</dc:creator>
  <dc:contributor>Grace Hopper</dc:contributor>
  <dc:description>Cartel A3 para la exposición de primavera.</dc:description>
  <dc:subject>cartel, exposición, primavera</dc:subject>
  <dc:language>es-ES</dc:language>
  <dc:rights>CC BY-SA 4.0</dc:rights>

  <xarast:dates>
    <xarast:created>2026-03-11T09:14:02Z</xarast:created>
    <xarast:modified>2026-09-19T17:41:55Z</xarast:modified>
    <xarast:printed>2026-05-02T11:00:00Z</xarast:printed>
    <xarast:editing-duration>PT18H42M</xarast:editing-duration>
    <xarast:editing-cycles>93</xarast:editing-cycles>
  </xarast:dates>

  <xarast:generator xarast:name="Xarast" xarast:version="0.1.0"
                    xarast:platform="linux-x86_64"/>
  <xarast:origin xarast:imported-from="xar" xarast:source-file="cartel.xar"
                 xarast:source-format-version="2.1"/>

  <xarast:statistics xarast:spreads="1" xarast:pages="1" xarast:layers="7"
                     xarast:objects="1842" xarast:bitmaps="3"
                     xarast:fonts="2" xarast:colours="18"/>

  <xarast:units xarast:default="mm" xarast:precision="2">
    <xarast:user-unit xarast:id="u-pica" xarast:name="Pica" xarast:abbrev="pc"
                      xarast:points="12" xarast:prefix="" xarast:suffix=" pc"/>
  </xarast:units>

  <xarast:page-setup xarast:width="297mm" xarast:height="420mm"
                     xarast:orientation="portrait"
                     xarast:bleed="3mm"
                     xarast:double-page="false"
                     xarast:facing="false"
                     xarast:margin-top="10mm" xarast:margin-right="10mm"
                     xarast:margin-bottom="10mm" xarast:margin-left="10mm"/>

  <xarast:grid xarast:kind="rectangular" xarast:origin="0 0"
               xarast:spacing="10mm" xarast:subdivisions="10"
               xarast:visible="true" xarast:snap="true"
               xarast:colour="#c8d8e8"/>

  <xarast:guides>
    <xarast:guide xarast:orientation="vertical"   xarast:position="30mm"  xarast:colour="#00a0ff"/>
    <xarast:guide xarast:orientation="horizontal" xarast:position="45mm"/>
    <xarast:guide xarast:orientation="angled" xarast:position="100mm 100mm"
                  xarast:angle="30"/>
  </xarast:guides>

  <xarast:view xarast:zoom="0.75" xarast:scroll="0 0" xarast:quality="antialiased"
               xarast:active-layer="xL2"/>

  <xarast:nudge xarast:distance="1mm"/>

  <xarast:print>
    <xarast:imagesetting xarast:screen-ruling="150" xarast:screen-angle="45"
                         xarast:dot-shape="round" xarast:negative="false"
                         xarast:emulsion-down="false"/>
    <xarast:plate xarast:name="Cyan"    xarast:enabled="true" xarast:angle="15"/>
    <xarast:plate xarast:name="Magenta" xarast:enabled="true" xarast:angle="75"/>
    <xarast:plate xarast:name="Yellow"  xarast:enabled="true" xarast:angle="0"/>
    <xarast:plate xarast:name="Black"   xarast:enabled="true" xarast:angle="45"/>
    <xarast:printmarks xarast:kind="default"/>
  </xarast:print>

  <xarast:colour-management
      xarast:working-rgb="sRGB IEC61966-2.1"
      xarast:working-cmyk="resources/profiles/b3-1122….icc"
      xarast:rendering-intent="relative-colorimetric"
      xarast:black-point-compensation="true"/>

  <xarast:comment>Revisión aprobada por dirección el 2026-05-02.</xarast:comment>
</xarast:meta>
```

Correspondencia con los tags de Xara: `TAG_DOCUMENTCOMMENT` (90),
`TAG_DOCUMENTDATES` (91), `TAG_DOCUMENTFLAGS` (93), `TAG_DOCUMENTINFORMATION` (4136),
`TAG_GRIDRULERSETTINGS` (46), `TAG_GRIDRULERORIGIN` (47), `TAG_DEFINE_DEFAULTUNITS` (87),
`TAG_DEFINE_PREFIXUSERUNIT` (85), `TAG_DEFINE_SUFFIXUSERUNIT` (86),
`TAG_DOCUMENTNUDGE` (4114), `TAG_VIEWPORT` (80), `TAG_VIEWQUALITY` (81),
`TAG_DOCUMENTVIEW` (82), `TAG_SPREADINFORMATION` (45), `TAG_PRINTERSETTINGS` (3506),
`TAG_IMAGESETTING` (3507), `TAG_COLOURPLATE` (3508), `TAG_PRINTMARK*` (3509/3510).

### 7.3 Guías y rejilla: doble emisión

Para que Inkscape muestre las guías del documento, el escritor **DEBERÍA** emitir además
en `document.svg`:

```xml
<sodipodi:namedview id="base" units="mm" inkscape:document-units="mm"
                    showgrid="true" inkscape:snap-global="true">
  <inkscape:grid type="xygrid" spacingx="10mm" spacingy="10mm" originx="0" originy="0"/>
  <sodipodi:guide position="85,0" orientation="1,0"/>
  <sodipodi:guide position="0,127" orientation="0,1"/>
</sodipodi:namedview>
```

**Cuidado con el eje Y:** las guías de Inkscape ≥ 1.0 usan el eje Y hacia abajo por
defecto, igual que Xarast (§5.5), así que la conversión es la identidad. El escritor
**DEBE** emitir `inkscape:document-units` coherente con `xarast:units/@default`.

### 7.4 Versión del formato y capacidades

Tres números distintos, con propósitos distintos:

| Campo | Dónde | Semántica |
|---|---|---|
| `xarast:version` | manifiesto y `meta.xml` | Versión del **formato** con la que se escribió (`MAYOR.MENOR`) |
| `xarast:min-reader` | manifiesto y `meta.xml` | Versión **mínima** de lector requerida para abrir el fichero **sin perder nada** |
| `<mf:requires>` | manifiesto | Lista de **capacidades** discretas requeridas |
| `xarast:generator` | `meta.xml` | Aplicación y versión que lo escribió (diagnóstico) |

Reglas de compatibilidad:

1. **Hacia atrás (leer ficheros viejos).** Un lector de versión *V* **DEBE** abrir sin
   pérdida cualquier fichero con `xarast:version ≤ V` dentro de la misma versión mayor.
2. **Hacia delante (leer ficheros nuevos).** Si `xarast:min-reader > V`, el lector
   **DEBE** avisar claramente y **DEBERÍA** ofrecer abrir en **modo sólo lectura**. Si el
   usuario fuerza la edición, se aplican las reglas de §8 y el lector **DEBE** marcar el
   documento como degradado.
3. Si `xarast:min-reader ≤ V` pero `xarast:version > V`, el lector **DEBE** abrir y
   editar normalmente: el escritor ha garantizado que todo lo nuevo es ignorable de forma
   segura. **Esta es la vía normal para la evolución del formato.**
4. Un escritor **DEBE** elevar `xarast:min-reader` **sólo** cuando introduzca algo cuya
   ignorancia produciría un documento **visualmente incorrecto o peligroso**, nunca por
   añadir un parámetro nuevo a un efecto existente.
5. La versión **mayor** sólo se incrementa con un cambio incompatible del contenedor
   (nuevo URI de espacio de nombres, §5.2).

```xml
<mf:requires>
  <mf:capability mf:name="zstd"/>
  <mf:capability mf:name="mesh-fill-v2" mf:optional="true"/>
</mf:requires>
```
Una capacidad con `mf:optional="true"` **NO** impide abrir: sólo avisa de que algo se
verá peor.

---

## 8. Preservación de datos desconocidos

Éste es el requisito **O4** y es, deliberadamente, la parte más estricta de la
especificación. La razón es empírica: es la propiedad que fallan casi todos los formatos
comparables (§2.4), y su fallo convierte «el documento se ve raro» en «el trabajo del
usuario se ha destruido y no hay vuelta atrás».

### 8.1 Principio general

> **Un lector DEBE preservar, íntegramente y en su sitio, todo dato que no comprenda, y
> DEBE volver a escribirlo al guardar.** No comprender no autoriza a borrar.

### 8.2 Preservación dentro de `document.svg` y `meta.xml`

El modelo de documento en memoria **DEBE** tener, en cada nodo, un contenedor de
«equipaje» (*foreign baggage*) que retiene:

1. **Atributos de espacio de nombres ajeno desconocidos** (incluidos los `xarast:` de una
   versión futura): se almacenan como `(uri, local-name, valor)` y se reemiten
   literalmente sobre el mismo elemento.
2. **Elementos hijo de espacio de nombres ajeno desconocidos**: se almacenan como el
   subárbol XML **serializado tal cual** (texto exacto, incluidos prefijos y
   declaraciones de espacio de nombres necesarias) junto con su **posición** relativa
   entre los hermanos conocidos, y se reemiten en esa posición.
3. **Elementos SVG desconocidos** (p. ej. de SVG 2 no soportado): igual que el punto 2,
   pero además el lector **DEBE** avisar de que el documento contiene SVG que no
   entiende, porque afecta al render.
4. **Comentarios XML y instrucciones de procesamiento**: se preservan y se reemiten en su
   posición. (Coste bajo, valor alto: la gente pone notas ahí.)
5. **Atributos SVG estándar desconocidos**: se preservan.
6. El orden de los atributos **NO** es significativo y **NO** necesita preservarse; el
   escritor **DEBE** emitir los atributos en orden determinista (SVG estándar primero en
   orden canónico, luego los de espacios ajenos ordenados por URI y nombre local) para
   cumplir O8.

**Ejemplo.** Xarast 0.1 abre un documento escrito por Xarast 2.0:

```xml
<path id="xa7" d="M 0,0 L 100,0"
      fill="#c33"
      xarast:shape="quick"
      xarast:mesh-warp="3 0.5 0.2 …"          <!-- desconocido en 0.1 -->
      acme:review-state="approved">            <!-- desconocido, de un plugin -->
  <xarast:quickshape xarast:sides="5" …/>      <!-- conocido -->
  <xarast:neural-fill xarast:model="…"/>       <!-- desconocido en 0.1 -->
</path>
```

Tras editar la geometría en 0.1 y guardar, el fichero **DEBE** contener todavía
`xarast:mesh-warp`, `acme:review-state` y `<xarast:neural-fill>`, intactos.

### 8.3 Preservación de entradas del ZIP

1. Toda entrada del ZIP cuyo nombre no encaje en el layout de §3.2 **DEBE** copiarse
   inalterada al guardar, junto con su entrada del manifiesto.
2. El escritor **NO DEBE** recomprimirlas con otro método (se copia el flujo comprimido
   tal cual cuando es posible; si no, se recomprime con el mismo método).
3. Los recursos bajo `resources/` referenciados **sólo** desde equipaje desconocido
   **NO DEBEN** recolectarse como basura. Para esto, el lector **DEBE** escanear el
   equipaje en busca de cadenas que coincidan con rutas de entrada del manifiesto y
   marcarlas como referenciadas. Regla conservadora, deliberada: preferimos arrastrar un
   recurso de sobra a romper un documento.
4. `extensions/` y `META-INF/` (salvo `manifest.xml`) se preservan siempre.

### 8.4 Detección de pérdida: el digest de preservación

Los puntos 1-3 protegen frente a **Xarast**. No protegen frente a un tercero (un
limpiador de SVG, un script, Inkscape en una versión que rompa algo) que destruya el
equipaje. Para eso:

- El escritor **DEBERÍA** emitir en `<xarast:document>`:
  ```xml
  xarast:foreign-digest="blake3:9f2c…"
  xarast:foreign-count="14"
  ```
  donde el digest se calcula sobre la concatenación canónica (C14N sobre cada fragmento,
  ordenados por la ruta del elemento propietario) de todo el equipaje que el escritor
  **no** comprendía.
- Al abrir, el lector **DEBE** recalcular el digest. Si no coincide o el contador ha
  disminuido, **DEBE** avisar: *«Este documento ha sido modificado por otra aplicación y
  se han perdido N datos de una versión más reciente de Xarast. Guardar sobrescribirá esa
  pérdida de forma permanente.»* y **DEBERÍA** ofrecer «Guardar como copia».
- El digest **NO** se recalcula sobre el equipaje que el lector actual **sí** entiende:
  sólo cubre lo genuinamente desconocido.

### 8.5 Cuando el usuario edita un objeto con equipaje desconocido

Este es el caso difícil y hay que decidirlo explícitamente:

1. **Edición que no toca al objeto** (mover otra cosa, cambiar una capa distinta):
   el equipaje se preserva sin más.
2. **Edición de atributos ortogonales** (mover el objeto, cambiar su color): el equipaje
   se preserva y el objeto se marca con `xarast:foreign-dirty="true"`.
3. **Edición que invalida el equipaje** (editar los nodos de un path que lleva
   `xarast:neural-fill`, cuando no sabemos si ese relleno depende de la geometría): el
   lector **DEBE** preservar el equipaje **y** marcar `xarast:foreign-stale="true"`.
   Un lector futuro que entienda ese equipaje **DEBE** comprobar la marca y revalidar o
   regenerar en vez de confiar ciegamente.
4. **Borrado del objeto**: el equipaje se va con él. No se intenta salvarlo. Se registra
   en el aviso de guardado (`N objetos con datos de versiones futuras eliminados`).
5. `xarast:base-authoritative="true"` (§5.1 regla 4) es la marca que un escritor pone
   cuando la representación **base** SVG debe ganar sobre la paramétrica desconocida;
   la usa por ejemplo un efecto vivo desconocido cuyo horneado no podemos regenerar
   (§6.8.7).

### 8.6 Lo que un lector NUNCA debe hacer

- **NO DEBE** «normalizar» el SVG reescribiendo elementos que no ha tocado.
- **NO DEBE** eliminar declaraciones de espacios de nombres, aunque parezcan sin usar:
  pueden estarlo desde dentro de un fragmento de equipaje serializado.
- **NO DEBE** reordenar hijos.
- **NO DEBE** convertir `<rect>`/`<circle>` a `<path>` al guardar si no los ha editado.
- **NO DEBE** reindentar ni reescribir el documento entero si el usuario no ha cambiado
  nada (un «abrir y cerrar» no DEBERÍA producir escritura alguna).

### 8.7 Test de conformidad de preservación

Obligatorio en CI (§13.5):

1. Se toma un documento de referencia y se le inyectan mecánicamente, en cada nodo,
   atributos y elementos de un espacio de nombres ficticio (`urn:test:future`).
2. Se abre con Xarast, se aplica una batería de ediciones (mover, cambiar color,
   agrupar, cambiar de capa, deshacer, rehacer) y se guarda.
3. Se extrae el SVG y se comprueba que **el 100 %** del contenido inyectado sigue
   presente, en el mismo elemento y en la misma posición relativa.
4. Se repite con entradas ZIP desconocidas y con un `meta.xml` con secciones
   desconocidas.

---

## 9. Identificación: magic bytes, extensión, MIME

### 9.1 Extensión

- Extensión primaria: **`.xarast`**.
- Extensión alternativa aceptada en lectura: `.xrst` (para sistemas de ficheros que
  limitan la extensión a 4 caracteres). El escritor **NO DEBERÍA** usarla por defecto.
- El escritor **NO DEBE** usar `.xar` (ya ocupada por Xara y por el archivador xar de
  macOS: colisión doble, documentada en §2).

### 9.2 Firma (magic bytes)

```
offset  0  : 50 4B 03 04                      "PK\x03\x04"  (cabecera local ZIP)
offset 30  : 6D 69 6D 65 74 79 70 65          "mimetype"    (nombre de la 1ª entrada)
offset 38  : 61 70 70 6C 69 63 61 74 69 6F 6E 2F 76 6E 64 2E
             78 61 72 61 73 74 2B 7A 69 70    "application/vnd.xarast+zip"  (26 bytes)
```

La firma completa a comprobar es, por tanto:

```
"PK\x03\x04" en 0  AND  "mimetype" en 30  AND  "application/vnd.xarast+zip" en 38
```

Esto funciona **sin descomprimir nada** y con sólo 64 bytes leídos. Es el mismo diseño
que usan EPUB y ODF, y por eso §3.2.1 prohíbe el campo extra en la cabecera local de
`mimetype`.

### 9.3 Tipo MIME

| Tipo | Uso |
|---|---|
| `application/vnd.xarast+zip` | **Canónico.** El sufijo `+zip` (RFC 6839) informa a las herramientas genéricas de que el contenedor es un ZIP |
| `application/x-xarast` | Alias tolerado en lectura; **NO DEBE** escribirse |
| `image/svg+xml` | El tipo de `document.svg` dentro del paquete |

Se elige el árbol `vnd.` (proveedor) y no `prs.` (personal) ni `x-` (experimental, hoy
desaconsejado por el RFC 6648). El registro ante IANA **DEBERÍA** solicitarse antes de la
v1.0 estable.

### 9.4 Integración en Linux: `shared-mime-info`

Fichero `packaging/linux/xarast.xml` (se instala en
`/usr/share/mime/packages/xarast.xml`):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/vnd.xarast+zip">
    <comment>Xarast document</comment>
    <comment xml:lang="es">Documento de Xarast</comment>
    <comment xml:lang="de">Xarast-Dokument</comment>
    <comment xml:lang="fr">Document Xarast</comment>
    <comment xml:lang="pt">Documento do Xarast</comment>
    <acronym>XARAST</acronym>
    <expanded-acronym>Xarast Vector Document</expanded-acronym>
    <sub-class-of type="application/zip"/>
    <generic-icon name="image-x-generic"/>
    <magic priority="80">
      <match type="string" value="PK\003\004" offset="0">
        <match type="string" value="mimetype" offset="30">
          <match type="string" value="application/vnd.xarast+zip" offset="38"/>
        </match>
      </match>
    </magic>
    <glob pattern="*.xarast"/>
    <glob pattern="*.xrst"/>
    <alias type="application/x-xarast"/>
  </mime-type>
</mime-info>
```

- `priority="80"` es mayor que el de `application/zip` (que es bajo por ser genérico),
  siguiendo la recomendación de la especificación de usar valores altos para subtipos
  específicos.
- `<sub-class-of type="application/zip"/>` permite a los gestores de archivos ofrecer
  «Extraer aquí» además de «Abrir con Xarast».
- Instalación: `update-mime-database /usr/share/mime`.
- Para el AppImage, el fichero **DEBE** incluirse también en la raíz del AppDir para que
  las herramientas de integración de escritorio lo registren.

### 9.5 Fichero `.desktop`

`packaging/linux/org.xarast.Xarast.desktop`:

```ini
[Desktop Entry]
Type=Application
Version=1.5
Name=Xarast
GenericName=Vector Graphics Editor
GenericName[es]=Editor de gráficos vectoriales
Comment=Create and edit vector graphics and photo compositions
Comment[es]=Crea y edita gráficos vectoriales y composiciones fotográficas
Exec=xarast %F
TryExec=xarast
Icon=org.xarast.Xarast
Terminal=false
StartupNotify=true
StartupWMClass=xarast
Categories=Graphics;VectorGraphics;2DGraphics;
Keywords=vector;svg;draw;illustration;xara;
Keywords[es]=vectorial;svg;dibujo;ilustración;xara;
MimeType=application/vnd.xarast+zip;image/svg+xml;image/svg+xml-compressed;application/vnd.xara;application/pdf;image/png;image/jpeg;image/webp;
Actions=NewDocument;

[Desktop Action NewDocument]
Name=New Document
Name[es]=Documento nuevo
Exec=xarast --new
```

Se incluye `application/vnd.xara` porque Xarast importa `.xar` (requisito de producto).

### 9.6 Miniaturas en el escritorio

`packaging/linux/xarast.thumbnailer` (en `/usr/share/thumbnailers/`):

```ini
[Thumbnailer Entry]
TryExec=xarast-thumbnailer
Exec=xarast-thumbnailer -s %s %u %o
MimeType=application/vnd.xarast+zip;
```

`xarast-thumbnailer` **DEBE** limitarse a extraer `thumbnail.png` del ZIP y reescalarlo:
no debe abrir ni renderizar el documento. Es la justificación operativa de O6 y de que
la miniatura esté en el paquete.

### 9.7 Otras plataformas (previsto)

- **Windows:** clave de registro `.xarast` → `Xarast.Document`, `PerceivedType=image`,
  extractor de miniaturas `IThumbnailProvider` que lee `thumbnail.png`.
- **macOS:** `UTExportedTypeDeclarations` con identificador `org.xarast.document`,
  `UTTypeConformsTo = ["public.zip-archive", "public.composite-content"]`, y una
  extensión Quick Look.

---

## 10. Recuperación ante fallos

### 10.1 Escritura atómica (obligatoria)

El escritor **DEBE** guardar siguiendo exactamente esta secuencia:

1. Crear `<nombre>.xarast.tmp-<pid>-<rand>` **en el mismo directorio** que el destino
   (para que el `rename` sea atómico: mismo sistema de ficheros).
2. Escribir el ZIP completo.
3. `flush` + **`fsync`** del descriptor del fichero temporal.
4. `rename(tmp, destino)` — atómico en POSIX y en Windows (`MoveFileEx` con
   `MOVEFILE_REPLACE_EXISTING`).
5. `fsync` del **directorio** contenedor (POSIX), para que el rename sobreviva a un corte.
6. Borrar el autoguardado y el journal asociados.

El escritor **NO DEBE** truncar ni escribir sobre el fichero original en ningún momento.
Si el paso 2 o 3 falla, se borra el temporal y el original queda intacto.

Si el destino existe y el usuario tiene activada la preferencia de copia de seguridad, el
escritor **DEBERÍA** conservar el anterior como `<nombre>.xarast.bak` (rotando una sola
copia).

### 10.2 Autoguardado

- Ubicación: **fuera** del documento, en
  `$XDG_STATE_HOME/xarast/autosave/<doc-id>/` (por defecto
  `~/.local/state/xarast/autosave/<doc-id>/`), donde `<doc-id>` es el identificador
  persistente del documento (`meta.xml` → `<xarast:doc-id>`), o un ULID nuevo si el
  documento nunca se ha guardado.
- Contenido: un `.xarast` **completo y válido** (no un formato aparte), llamado
  `snapshot.xarast`, más un `state.json` con la ruta del documento original, el
  `doc-id`, la marca de tiempo, el PID y el nombre del host.
- Cadencia: **cada 5 minutos** de reloj **y** cada **50 operaciones de deshacer**,
  lo que ocurra antes. Configurable; `0` desactiva.
- El autoguardado **DEBE** usar la misma escritura atómica de §10.1.
- El autoguardado **NO DEBE** bloquear la interfaz: se serializa en un hilo aparte sobre
  una instantánea inmutable del modelo (esto es un requisito para el diseño del modelo de
  documento: las estructuras persistentes o el copy-on-write lo hacen barato).
- Al arrancar, Xarast **DEBE** escanear el directorio de autoguardado y, si encuentra
  instantáneas cuyo `state.json` apunta a un PID que ya no existe (o a otro host),
  ofrecer la recuperación con una comparación de fechas frente al fichero en disco.

### 10.3 Journal de operaciones

Para reducir la ventana de pérdida por debajo de la cadencia del autoguardado:

- Ubicación: `$XDG_STATE_HOME/xarast/autosave/<doc-id>/journal.ndjson`.
- Formato: **NDJSON append-only**, una línea por operación de deshacer aplicada, con
  `{"seq":N,"ts":"…","op":"…","payload":{…}}`.
- El escritor **DEBE** hacer `write` + `fsync` de cada línea (o agrupar con un
  `fsync` cada 250 ms como máximo).
- Al recuperar, Xarast carga `snapshot.xarast` y **reproduce** las entradas del journal
  con `seq` posterior al de la instantánea.
- El journal **DEBE** truncarse en cada autoguardado y borrarse en cada guardado real.
- Una operación que no sea representable en el journal (p. ej. una importación de
  100 MB) **DEBE** forzar un autoguardado inmediato en lugar de escribir al journal.
- Se descarta explícitamente meter el journal **dentro** del `.xarast`: obligaría a
  reescribir el ZIP en cada operación.

### 10.4 Fichero de bloqueo

Para evitar que dos instancias (o dos usuarios en una unidad de red) editen el mismo
documento a la vez:

- Nombre: `.<nombre>.xarast.lock`, en el **mismo directorio** que el documento (patrón de
  LibreOffice, reconocible y que funciona en sistemas de ficheros de red).
- Contenido (texto UTF-8, una clave por línea):
  ```
  xarast-lock/1
  pid=48213
  host=nombre-del-equipo
  user=jose
  boot-id=8f1c3d2e-…
  doc-id=01J9Q7ZB2K4M8N6P3R5T7V9W1X
  since=2026-09-19T17:02:11Z
  ```
- Creación: con `O_CREAT|O_EXCL` (fallo si existe), más un bloqueo consultivo
  `flock`/`LOCK_EX|LOCK_NB` sobre el propio fichero de bloqueo, que aporta detección
  fiable en local aunque el proceso muera.
- Si el bloqueo existe:
  - Si `host` y `boot-id` coinciden con los actuales y el PID **no** está vivo → bloqueo
    obsoleto: se reclama automáticamente y se avisa.
  - En cualquier otro caso → se ofrece «Abrir sólo lectura» / «Abrir una copia» /
    «Forzar (arriesgado)».
- El bloqueo **DEBE** liberarse al cerrar, y el proceso **DEBE** instalar manejadores
  para `SIGINT`/`SIGTERM` que lo eliminen. **NO DEBE** dejarse en `atexit` únicamente.
- Si el directorio es de sólo lectura, la ausencia de bloqueo **NO DEBE** impedir abrir.

### 10.5 Robustez de lectura

- Un `.xarast` con el directorio central dañado **DEBERÍA** poder recuperarse escaneando
  las cabeceras locales (`PK\x03\x04`). Xarast **DEBERÍA** ofrecer
  `xarast repair doc.xarast` que haga ese escaneo, valide digests contra el manifiesto
  cuando esté legible, y reconstruya un paquete sano.
- Un `document.svg` con XML mal formado **DEBERÍA** intentar recuperarse parseando hasta
  el punto de error y conservando lo leído, con aviso claro y apertura en sólo lectura.
- Las entradas cuyo digest BLAKE3 no coincida con el manifiesto **DEBEN** marcarse; el
  documento se abre pero se avisa, y los recursos corruptos se sustituyen por un
  marcador visible.
- Límites duros obligatorios contra *zip bombs* y XML malicioso:
  - ratio de descompresión máxima por entrada: **200:1** (configurable al alza);
  - tamaño descomprimido total máximo: **4 GiB** por defecto;
  - profundidad máxima de anidamiento XML: **256**;
  - número máximo de entidades XML: **0** (las DTD se rechazan, §5.3);
  - número máximo de entradas: **65 535** sin ZIP64, **1 000 000** con ZIP64.

---

## 11. Esquemas formales

Se proporcionan en **RELAX NG Compact** (`.rnc`), por ser el más legible y el que mejor
expresa contenido mixto y extensibilidad abierta (`anyAttribute`/`anyElement`), que es
justo lo que exige §8. Se generan además las versiones XML (`.rng`) para `xmllint`.

Ubicación en el repositorio: `crates/xarast-format/schemas/`.

### 11.1 `manifest.rnc` — `META-INF/manifest.xml`

```rnc
default namespace = ""
namespace mf = "https://xarast.org/ns/manifest/1.0"

start = Manifest

Manifest =
  element mf:manifest {
    attribute mf:version     { xsd:string { pattern = "[0-9]+\.[0-9]+" } },
    attribute mf:min-reader  { xsd:string { pattern = "[0-9]+\.[0-9]+" } },
    attribute mf:generator   { text }?,
    attribute mf:profile     { "portable" | "compact" },
    Requires?,
    FileEntry+,
    AnyForeign*
  }

Requires =
  element mf:requires {
    element mf:capability {
      attribute mf:name     { text },
      attribute mf:optional { xsd:boolean }?
    }+
  }

FileEntry =
  element mf:file-entry {
    attribute mf:full-path   { text },
    attribute mf:media-type  { text },
    attribute mf:role        { Role }?,
    attribute mf:size        { xsd:nonNegativeInteger }?,
    attribute mf:method      { "stored" | "deflate" | "zstd" }?,
    attribute mf:digest      { "blake3-256" }?,
    attribute mf:digest-value{ xsd:string { pattern = "[0-9a-f]{64}" } }?,
    attribute mf:refcount    { xsd:nonNegativeInteger }?,
    attribute mf:derived-from{ xsd:string { pattern = "[0-9a-f]{64}" } }?,
    attribute mf:derivation  { text }?,
    AnyForeignAttr*,
    AnyForeign*
  }

Role = "mimetype" | "manifest" | "meta" | "document" | "thumbnail"
     | "preview" | "resource" | "history" | "extension" | "unknown"

# --- Extensibilidad: TODO lo desconocido es válido y DEBE preservarse (§8) ---
AnyForeignAttr = attribute * - mf:* { text }
AnyForeign     = element   * - mf:* { (AnyForeignAttr | AnyForeign | text)* }
```

### 11.2 `xarast-doc.rnc` — extensiones en `document.svg` (esquemático)

Se valida **sólo** el vocabulario `xarast:`; el SVG base se valida aparte contra el
esquema oficial de SVG 1.1.

```rnc
namespace x   = "https://xarast.org/ns/document/1.0"
namespace svg = "http://www.w3.org/2000/svg"

Num      = xsd:double
Point    = xsd:string          # "x y"
Matrix6  = xsd:string          # "a b c d e f"
IdRef    = xsd:string          # "#id"
Bool     = xsd:boolean
Colour   = xsd:string          # "#rrggbb" | "#rrggbbaa"
Profile  = xsd:string          # "<bias> <gain>"

# ---------------- Documento ----------------
Document =
  element x:document {
    attribute x:version           { text },
    attribute x:min-reader        { text },
    attribute x:y-axis            { "down" | "up" }?,
    attribute x:layout            { "single" | "split" }?,
    attribute x:colour-refs       { "literal" | "var" }?,
    attribute x:duplicate-offset  { Point }?,
    attribute x:foreign-digest    { text }?,
    attribute x:foreign-count     { xsd:nonNegativeInteger }?,
    Chapter*, AnyForeign*
  }

Chapter = element x:chapter { attribute x:name { text }, attribute x:spreads { text } }

Page =
  element x:page {
    attribute x:index  { xsd:positiveInteger },
    attribute x:rect   { xsd:string },      # "x y w h"
    attribute x:bleed  { Num }?,
    attribute x:double { Bool }?,
    attribute x:scale  { Num }?
  }

# ---------------- Formas paramétricas ----------------
QuickShape =
  element x:quickshape {
    attribute x:sides                 { xsd:positiveInteger },
    attribute x:circular              { Bool },
    attribute x:stellated             { Bool },
    attribute x:primary-curvature     { Bool },
    attribute x:stellation-curvature  { Bool },
    attribute x:stell-radius-ratio    { Num }?,
    attribute x:primary-curve-ratio   { Num }?,
    attribute x:stell-curve-ratio     { Num }?,
    attribute x:stell-offset-ratio    { Num }?,
    attribute x:centre                { Point },
    attribute x:major-axis            { Point },
    attribute x:minor-axis            { Point },
    attribute x:matrix                { Matrix6 }?,
    attribute x:reformed              { Bool }?,
    element x:edge-path { attribute d { text } }*,
    AnyForeign*
  }

# ---------------- Rellenos y transparencias ----------------
FillKind = "flat" | "linear" | "linear3" | "circular" | "elliptical"
         | "conical" | "diamond" | "three-point" | "four-point"
         | "bitmap" | "contone" | "fractal-clouds" | "fractal-plasma" | "noise"

Fill =
  element x:fill {
    attribute x:type       { FillKind },
    attribute x:repeat     { "simple" | "repeat" | "reflect" | "repeat-extra" }?,
    attribute x:profile    { Profile }?,
    attribute x:effect     { "fade" | "rainbow" | "alt-rainbow" }?,
    attribute x:stops      { text }?,          # "0:#rrggbb 0.5:#… 1:#…"
    attribute x:centre     { Point }?,
    attribute x:seed       { xsd:integer }?,
    attribute x:graininess { Num }?,
    attribute x:octaves    { xsd:positiveInteger }?,
    attribute x:dpi        { Num }?,
    element x:point { attribute x { Num }, attribute y { Num },
                      attribute x:colour { Colour } }*,
    AnyForeignAttr*, AnyForeign*
  }

BlendMode = "mix" | "stained-glass" | "bleach" | "darken" | "lighten"
          | "saturation" | "luminosity" | "hue" | "contrast" | "brightness"
          | "bevel" | "additive" | "subtractive" | "lut"

Transparency =
  element x:transparency {
    attribute x:type   { FillKind },
    attribute x:blend  { BlendMode }?,
    attribute x:amount { Num }?,
    attribute x:profile{ Profile }?,
    attribute x:stops  { text }?,
    AnyForeignAttr*, AnyForeign*
  }

# ---------------- Trazo ----------------
Stroke =
  element x:stroke {
    attribute x:type       { "plain" | "variable" | "brush" | "airbrush" },
    attribute x:base-width { Num }?,
    attribute x:profile    { Profile }?,
    element x:width-table  { text }?,          # "t:w t:w …"
    element x:pressure     { attribute x:encoding { "text" | "base64-u16le" }, text }?,
    element x:source-path  { attribute d { text } }?,
    AnyForeign*
  }

# ---------------- Efectos vivos ----------------
Shadow =
  element x:shadow {
    attribute x:type     { "wall" | "floor" | "glow" | "inner" },
    attribute x:blur     { Num }, attribute x:offset { Point }?,
    attribute x:angle    { Num }?, attribute x:darkness { Num },
    attribute x:scale    { Num }?, attribute x:colour { Colour }?,
    attribute x:penumbra { Num }?
  }

Bevel =
  element x:bevel {
    attribute x:type            { "round" | "flat" | "chisel" | "ridge" | "mesa" },
    attribute x:indent          { Num },
    attribute x:light-angle     { Num },
    attribute x:light-elevation { Num }?,
    attribute x:contrast        { Num },
    attribute x:direction       { "inner" | "outer" },
    attribute x:join            { "round" | "miter" | "bevel" }?,
    attribute x:light-colour    { Colour }?,
    attribute x:shadow-colour   { Colour }?
  }

Contour =
  element x:contour {
    attribute x:width          { Num },
    attribute x:steps          { xsd:positiveInteger },
    attribute x:direction      { "inner" | "outer" | "both" },
    attribute x:join           { "round" | "miter" | "bevel" }?,
    attribute x:profile        { Profile }?,
    attribute x:colour-profile { Profile }?,
    attribute x:insets         { Bool }?
  }

Blend =
  element x:blend {
    attribute x:steps             { xsd:positiveInteger },
    attribute x:from              { IdRef },
    attribute x:to                { IdRef },
    attribute x:position-profile  { Profile }?,
    attribute x:attribute-profile { Profile }?,
    attribute x:one-to-one        { Bool }?,
    attribute x:antialias         { Bool }?,
    attribute x:path              { IdRef }?,
    attribute x:rotate-along-path { Bool }?,
    attribute x:start-angle       { Num }?,
    attribute x:end-angle         { Num }?,
    attribute x:tangential        { Bool }?,
    element x:blend-map { attribute x:pairs { text } }?,
    AnyForeign*
  }

Mould =
  element x:mould {
    attribute x:type   { "envelope" | "perspective" },
    attribute x:bounds { xsd:string },
    element x:mould-shape  { attribute d { text } },
    element x:mould-source { AnySvg* },
    AnyForeign*
  }

Feather = element x:feather { attribute x:width { Num },
                              attribute x:profile { Profile }? }

Effects =
  element x:effects {
    element x:effect {
      attribute x:id     { xsd:ID },
      attribute x:kind   { text },
      attribute x:locked { Bool }?,
      AnyForeignAttr*, AnyForeign*
    }+
  }

# ---------------- Color ----------------
Palette =
  element x:palette {
    attribute x:id { text },
    element x:colour {
      attribute x:id     { xsd:ID },
      attribute x:name   { text }?,
      attribute x:model  { "rgb"|"cmyk"|"hsv"|"grey"|"spot"|"tint"|"shade"|"linked" },
      attribute x:srgb   { Colour },
      attribute x:cmyk   { text }?,
      attribute x:hsv    { text }?,
      attribute x:grey   { Num }?,
      attribute x:spot-name  { text }?,
      attribute x:parent     { IdRef }?,
      attribute x:amount     { Num }?,
      attribute x:hsv-delta  { text }?,
      attribute x:profile    { text }?,
      attribute x:screen-angle { Num }?,
      attribute x:solid-ink  { Bool }?,
      AnyForeignAttr*
    }+,
    AnyForeign*
  }

# ---------------- Atributos sueltos admitidos en elementos SVG ----------------
CommonXarastAttrs =
  attribute x:kind                { text }?,
  attribute x:generated           { text }?,
  attribute x:generated-by        { text }?,
  attribute x:generated-rev       { xsd:nonNegativeInteger }?,
  attribute x:generated-hash      { text }?,
  attribute x:base-authoritative  { Bool }?,
  attribute x:foreign-dirty       { Bool }?,
  attribute x:foreign-stale       { Bool }?,
  attribute x:names               { text }?,
  attribute x:fill-ref            { IdRef }?,
  attribute x:stroke-ref          { IdRef }?,
  attribute x:clone-of            { IdRef }?,
  attribute x:clone-mode          { "live" | "copy" }?,
  attribute x:locked              { Bool }?,
  attribute x:visible             { Bool }?,
  attribute x:printable           { Bool }?

AnySvg     = element svg:* { (attribute * { text } | AnySvg | text)* }
AnyForeignAttr = attribute * - x:* { text }
AnyForeign     = element   * - x:* { (attribute * { text } | AnyForeign | text)* }
```

### 11.3 `meta.rnc` — `meta.xml`

Se omite por extensión; su estructura es la de §7.2, con las mismas reglas de
extensibilidad abierta (`AnyForeign*` en cada nivel) y con `dc:*` tomado de Dublin Core.
El esquema completo vive en `crates/xarast-format/schemas/meta.rnc`.

### 11.4 Validación en CI

```bash
# Validación del manifiesto y de los metadatos
trang schemas/manifest.rnc schemas/manifest.rng
xmllint --noout --relaxng schemas/manifest.rng  extracted/META-INF/manifest.xml
xmllint --noout --relaxng schemas/meta.rng      extracted/meta.xml

# Validación del SVG base contra el esquema oficial de SVG 1.1
xmllint --noout --relaxng vendor/svg11.rng      extracted/document.svg

# Validación del vocabulario xarast: (extrayendo sólo ese subárbol)
xarast-validate extracted/document.svg --schema schemas/xarast-doc.rng
```

La CI **DEBE** ejecutar los cuatro pasos sobre cada fichero del corpus de conformidad.

---

## 12. Ejemplo completo

Un `.xarast` mínimo: **un rectángulo con degradado lineal (con perfil de rampa no
lineal) sobre una capa de fondo**, en dos capas, página A4.

> Todo lo que sigue es **real y verificado**: los ficheros se han construido, se ha
> comprobado que el XML está bien formado (`xmllint --noout`), se han calculado los
> digests BLAKE3 con la implementación de referencia, y el listado del ZIP y el volcado
> hexadecimal son la salida literal de `unzip -lv` y `od`. Los tamaños y los CRC cuadran.

### 12.1 Listado del ZIP

```console
$ unzip -lv ejemplo.xarast
Archive:  ejemplo.xarast
 Length   Method    Size  Cmpr    Date    Time   CRC-32   Name
--------  ------  ------- ---- ---------- ----- --------  ----
      26  Stored       26   0% 2026-09-19 17:41 8f97fcdc  mimetype
    1458  Defl:N      534  63% 2026-09-19 17:41 e509c5f1  META-INF/manifest.xml
    1841  Defl:N      771  58% 2026-09-19 17:41 7fed299a  meta.xml
    3375  Defl:N     1289  62% 2026-09-19 17:41 257a57e2  document.svg
     571  Stored      571   0% 2026-09-19 17:41 5cb1b71d  thumbnail.png
--------          -------  ---                            -------
    7271             3191  56%                            5 files
```

Tamaño total del fichero: **3 717 bytes**.

### 12.2 Verificación de la firma (magic bytes)

```console
$ od -A d -t x1z -v ejemplo.xarast | head -4
0000000 50 4b 03 04 14 00 00 00 00 00 3b 8d 33 5d dc fc  >PK........;.3]..<
0000016 97 8f 1a 00 00 00 1a 00 00 00 08 00 00 00 6d 69  >..............mi<
0000032 6d 65 74 79 70 65 61 70 70 6c 69 63 61 74 69 6f  >metypeapplicatio<
0000048 6e 2f 76 6e 64 2e 78 61 72 61 73 74 2b 7a 69 70  >n/vnd.xarast+zip<
```

Desglose de la cabecera local de la primera entrada:

| Offset | Bytes | Campo | Valor |
|---|---|---|---|
| 0 | `50 4b 03 04` | firma | `PK\x03\x04` |
| 4 | `14 00` | versión necesaria | 2.0 |
| 6 | `00 00` | flags | 0 (sin cifrado, sin descriptor) |
| 8 | `00 00` | método | **0 = STORED** |
| 18 | `1a 00 00 00` | tamaño comprimido | 26 |
| 22 | `1a 00 00 00` | tamaño sin comprimir | 26 |
| 26 | `08 00` | longitud del nombre | 8 |
| **28** | `00 00` | **longitud del campo extra** | **0** ← requisito de §3.2.1 |
| **30** | `6d 69 6d 65 74 79 70 65` | nombre | **`mimetype`** |
| **38** | `61 70 70 …` | contenido | **`application/vnd.xarast+zip`** |

### 12.3 `mimetype` (26 bytes, STORED, sin salto de línea final)

```
application/vnd.xarast+zip
```

### 12.4 `META-INF/manifest.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0"
             mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"
             mf:generator="Xarast/0.1.0 (linux; x86_64)">
  <mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/>
  <mf:file-entry mf:full-path="mimetype" mf:media-type="text/plain"
                 mf:role="mimetype" mf:size="26" mf:method="stored"/>
  <mf:file-entry mf:full-path="META-INF/manifest.xml" mf:media-type="application/xml"
                 mf:role="manifest" mf:method="deflate"/>
  <mf:file-entry mf:full-path="meta.xml" mf:media-type="application/xml"
                 mf:role="meta" mf:size="1841" mf:method="deflate"
                 mf:digest="blake3-256"
                 mf:digest-value="0292ad76bf30d13dce31f284842a8b01f772b36a7974775ae9f2c0eb46a5f956"/>
  <mf:file-entry mf:full-path="document.svg" mf:media-type="image/svg+xml"
                 mf:role="document" mf:size="3375" mf:method="deflate"
                 mf:digest="blake3-256"
                 mf:digest-value="e07d64cd960dc94e50d9030c9d1c3333d8b91c877bc113e535ade8d3ba5adc47"/>
  <mf:file-entry mf:full-path="thumbnail.png" mf:media-type="image/png"
                 mf:role="thumbnail" mf:size="571" mf:method="stored"
                 mf:digest="blake3-256"
                 mf:digest-value="4ab11d84d1b953e36c278f709e317dd045c2242fdddcda28e5f1b35d02910607"/>
</mf:manifest>
```

Obsérvese que la entrada del propio manifiesto **no** lleva digest (no puede contener su
propio hash) ni `mf:size` (lo aporta el ZIP).

### 12.5 `meta.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<xarast:meta xmlns:xarast="https://xarast.org/ns/document/1.0"
             xmlns:dc="http://purl.org/dc/elements/1.1/"
             xarast:version="1.0" xarast:min-reader="1.0">
  <xarast:identity>
    <xarast:doc-id>01J9Q7ZB2K4M8N6P3R5T7V9W1X</xarast:doc-id>
    <xarast:revision>3</xarast:revision>
  </xarast:identity>
  <dc:title>Ejemplo mínimo</dc:title>
  <dc:creator>Ada Lovelace</dc:creator>
  <dc:language>es-ES</dc:language>
  <xarast:dates>
    <xarast:created>2026-09-19T17:02:11Z</xarast:created>
    <xarast:modified>2026-09-19T17:41:55Z</xarast:modified>
    <xarast:editing-duration>PT39M44S</xarast:editing-duration>
    <xarast:editing-cycles>3</xarast:editing-cycles>
  </xarast:dates>
  <xarast:generator xarast:name="Xarast" xarast:version="0.1.0" xarast:platform="linux-x86_64"/>
  <xarast:statistics xarast:spreads="1" xarast:pages="1" xarast:layers="2"
                     xarast:objects="2" xarast:bitmaps="0" xarast:fonts="0" xarast:colours="3"/>
  <xarast:units xarast:default="mm" xarast:precision="2"/>
  <xarast:page-setup xarast:width="210mm" xarast:height="297mm"
                     xarast:orientation="portrait" xarast:bleed="0mm" xarast:double-page="false"/>
  <xarast:grid xarast:kind="rectangular" xarast:origin="0 0" xarast:spacing="10mm"
               xarast:subdivisions="10" xarast:visible="false" xarast:snap="true"/>
  <xarast:guides>
    <xarast:guide xarast:orientation="vertical" xarast:position="105mm"/>
  </xarast:guides>
  <xarast:view xarast:zoom="0.75" xarast:scroll="0 0" xarast:quality="antialiased"
               xarast:active-layer="xL2"/>
  <xarast:nudge xarast:distance="1mm"/>
  <xarast:colour-management xarast:working-rgb="sRGB IEC61966-2.1"
                            xarast:rendering-intent="relative-colorimetric"/>
</xarast:meta>
```

### 12.6 `document.svg` (contenido literal, 3 375 bytes)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     xmlns:xlink="http://www.w3.org/1999/xlink"
     xmlns:xarast="https://xarast.org/ns/document/1.0"
     xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
     width="210mm" height="297mm" viewBox="0 0 595.276 841.89"
     version="1.1" id="xdoc">
<title>Ejemplo mínimo</title>
<desc>Un rectángulo con degradado, en dos capas.</desc>
<metadata><rdf:RDF><rdf:Description>
<dc:title>Ejemplo mínimo</dc:title><dc:creator>Ada Lovelace</dc:creator>
<dc:date>2026-09-19T17:41:55Z</dc:date></rdf:Description></rdf:RDF></metadata>
<defs>
<xarast:document xarast:version="1.0" xarast:min-reader="1.0" xarast:y-axis="down"
                 xarast:layout="single" xarast:colour-refs="literal"
                 xarast:duplicate-offset="10 10"
                 xarast:foreign-digest="blake3:0000000000000000" xarast:foreign-count="0"/>
<xarast:units xarast:default="mm" xarast:precision="2"/>
<xarast:palette xarast:id="doc">
  <xarast:colour xarast:id="c-azul"   xarast:name="Azul"   xarast:model="rgb" xarast:srgb="#1c3f8f"/>
  <xarast:colour xarast:id="c-ambar"  xarast:name="Ámbar"  xarast:model="rgb" xarast:srgb="#ffd166"/>
  <xarast:colour xarast:id="c-grafito" xarast:name="Grafito" xarast:model="rgb" xarast:srgb="#2b2b2b"/>
</xarast:palette>
<linearGradient id="g7a3" gradientUnits="userSpaceOnUse"
                x1="141.732" y1="255.118" x2="453.543" y2="255.118"
                spreadMethod="pad" xarast:profile="0.35 0"
                xarast:stops="0:#1c3f8f 1:#ffd166">
  <stop offset="0" stop-color="#1c3f8f"/>
  <stop offset=".125" stop-color="#2a4f9c"/>
  <stop offset=".25" stop-color="#3b60a8"/>
  <stop offset=".375" stop-color="#5173b3"/>
  <stop offset=".5" stop-color="#6c88bd"/>
  <stop offset=".625" stop-color="#8d9fc4"/>
  <stop offset=".75" stop-color="#b3b8c6"/>
  <stop offset=".875" stop-color="#dcc8bf"/>
  <stop offset="1" stop-color="#ffd166"/>
</linearGradient>
</defs>
<sodipodi:namedview id="base" units="mm" inkscape:document-units="mm"
                    showgrid="false" inkscape:current-layer="xL2">
  <sodipodi:guide position="297.638,0" orientation="1,0"/>
</sodipodi:namedview>
<g id="xSPREAD1" xarast:kind="spread" xarast:spread="1">
<xarast:page xarast:index="1" xarast:rect="0 0 595.276 841.89" xarast:bleed="0"/>
<g id="xL1" inkscape:groupmode="layer" inkscape:label="Fondo"
   xarast:kind="layer" xarast:layer-kind="normal" xarast:visible="true"
   xarast:locked="true" xarast:printable="true" xarast:solid="false"
   sodipodi:insensitive="true" style="display:inline">
<rect id="xk3m9q2vr7t" x="0" y="0" width="595.276" height="841.89" fill="#f4f1ea"/>
</g>
<g id="xL2" inkscape:groupmode="layer" inkscape:label="Ilustración"
   xarast:kind="layer" xarast:layer-kind="normal" xarast:visible="true"
   xarast:locked="false" xarast:printable="true" xarast:solid="false"
   xarast:active="true" style="display:inline">
<rect id="xp8w4n1zc6h" x="141.732" y="141.732" width="311.811" height="226.772"
      rx="11.339" fill="url(#g7a3)" stroke="#2b2b2b" stroke-width="2.835"
      xarast:fill-ref="#c-azul" xarast:stroke-ref="#c-grafito"/>
</g>
</g>
</svg>
```

### 12.7 Comentario del ejemplo

Puntos que ilustra, uno a uno:

1. **El SVG es válido y autónomo.** Ningún elemento `xarast:` afecta al render: un
   navegador pinta el fondo crema, el rectángulo redondeado con el degradado y su borde
   grafito, con el tamaño físico A4 correcto.
2. **Las nueve paradas del degradado están horneadas** desde el perfil
   `xarast:profile="0.35 0"`. Xarast, al abrir, **descarta** esas nueve y regenera desde
   el perfil y `xarast:stops="0:#1c3f8f 1:#ffd166"` (§6.4). Un visor externo ve la curva
   aproximada con error ≤ 2/255.
3. **Las dos capas llevan doble marcado**: `inkscape:groupmode`/`inkscape:label` para
   Inkscape y `xarast:*` para Xarast. El bloqueo de la capa de fondo se expresa con
   `xarast:locked="true"` **y** con `sodipodi:insensitive="true"`, de modo que Inkscape
   también la respeta.
4. **`<xarast:page>` no se renderiza** porque está en un espacio de nombres ajeno: la
   geometría de la página no ensucia el dibujo.
5. **Referencias a la paleta** con `xarast:fill-ref`/`xarast:stroke-ref` junto al valor
   literal. Se ve el color correcto en cualquier parte y Xarast conserva el enlace.
6. **Coordenadas en puntos con 3 decimales**: `595.276` pt = 210 mm exactos,
   `141.732` pt = 50 mm, `2.835` pt = 1 mm. Precisión de milipunto sin ambigüedad.
7. **`xarast:foreign-count="0"`**: no hay equipaje desconocido. Un escritor **PUEDE**
   omitir los dos atributos cuando el contador es cero; aquí se emiten para ilustrarlos.
8. **Ratio de compresión 56 %** con sólo cinco entradas y 7 KiB de contenido: sobre
   documentos reales la ratio del SVG sube a 6:1-12:1 (§4.6).

### 12.8 Reproducir la verificación

```console
$ unzip -o ejemplo.xarast -d /tmp/ej && xdg-open /tmp/ej/document.svg   # render en navegador
$ xmllint --noout /tmp/ej/document.svg /tmp/ej/meta.xml /tmp/ej/META-INF/manifest.xml
$ b3sum /tmp/ej/document.svg
e07d64cd960dc94e50d9030c9d1c3333d8b91c877bc113e535ade8d3ba5adc47  /tmp/ej/document.svg
```

---

## 13. Plan de implementación en Rust

### 13.1 Ubicación en el árbol de crates

```
crates/
  xarast-model/      # modelo de documento (nodos, atributos, capas) — sin E/S
  xarast-format/     # ESTE formato: contenedor ZIP, manifiesto, meta
    schemas/         # *.rnc y *.rng
  xarast-svg/        # perfil SVG: serialización/deserialización modelo <-> SVG
  xarast-render/     # render (para miniaturas, previews y horneado)
  xarast-xar/        # importador del .xar binario
  xarast-cli/        # binario `xarast` (extract, cat, repair, convert, validate)
```

`xarast-format` **no** depende de `xarast-render`: la generación de miniaturas y el
horneado se inyectan mediante *traits* (`ThumbnailProvider`, `BakeProvider`) para que el
crate del formato siga siendo comprobable sin motor gráfico y sin dependencias pesadas.

### 13.2 Crates de terceros

| Crate | Uso | Notas |
|---|---|---|
| `zip` | Lectura y escritura del contenedor | Soporta `Stored`, `Deflated` y `Zstd` (método 93). Desactivar features innecesarias: `default-features = false, features = ["deflate", "zstd", "time"]` |
| `flate2` (backend `zlib-rs` o `miniz_oxide`) | Deflate | Implicado por `zip`; fijar el backend para reproducibilidad |
| `zstd` | Perfil `compact` | Sólo tras feature `compact` |
| `blake3` | Digests y deduplicación | Muy rápido; hashing en streaming durante la importación |
| `quick-xml` | Parseo y serialización de SVG/XML | **Clave**: es de los pocos que permiten preservar prefijos, comentarios e instrucciones de procesamiento — requisito de §8 |
| `memchr` | Aceleración del escaneo en `quick-xml` | Transitiva |
| `serde` + `serde_json` | `state.json` del autoguardado, journal NDJSON | No para el manifiesto (es XML) |
| `time` o `jiff` | Fechas RFC 3339 y fechas DOS del ZIP | |
| `ulid` o `uuid` (v7) | IDs persistentes de objeto y `doc-id` | |
| `fs4` | `flock`/bloqueo consultivo multiplataforma | Para §10.4 |
| `tempfile` | Escritura atómica | `NamedTempFile::persist` |
| `thiserror` | Errores tipados de la API pública | |
| `tracing` | Diagnóstico | |
| `resvg` + `usvg` + `tiny-skia` | **Sólo en `xarast-render`**: validación externa y miniaturas de referencia en tests | |
| `image` | Codificación de miniaturas PNG | |
| `insta` | Snapshot tests del SVG serializado | |
| `proptest` | Round-trip basado en propiedades | |
| `cargo-fuzz` + `arbitrary` | Fuzzing del lector | Obligatorio (principio 5 de la visión) |
| `criterion` | Benchmarks de guardar/abrir | |

Se rechaza explícitamente: cualquier binding a `libxml2` (superficie C, XXE por defecto),
y `roxmltree` como parser principal (es de sólo lectura y descarta información necesaria
para el round-trip; sí es útil en tests).

### 13.3 API pública propuesta

```rust
// ============================ crates/xarast-format/src/lib.rs ============================

/// Versión del formato que este código escribe.
pub const FORMAT_VERSION: Version = Version { major: 1, minor: 0 };
pub const MIME_TYPE: &str = "application/vnd.xarast+zip";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version { pub major: u16, pub minor: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile { Portable, Compact }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method { Stored, Deflate, Zstd }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(pub [u8; 32]);          // BLAKE3-256

// --------------------------------- LECTURA ---------------------------------

pub struct XarastReader<R: Read + Seek> { /* … */ }

impl<R: Read + Seek> XarastReader<R> {
    /// Abre el contenedor: valida la firma, lee el manifiesto y `meta.xml`.
    /// NO parsea `document.svg` (requisito O6: apertura barata).
    pub fn open(reader: R) -> Result<Self, ReadError>;

    /// Comprueba sólo los 64 primeros bytes. Útil para detección de tipo.
    pub fn sniff(reader: &mut R) -> Result<bool, ReadError>;

    pub fn format_version(&self) -> Version;
    pub fn min_reader(&self) -> Version;
    pub fn profile(&self) -> Profile;
    pub fn manifest(&self) -> &Manifest;
    pub fn meta(&self) -> &DocumentMeta;

    /// Miniatura sin tocar el documento.
    pub fn thumbnail(&mut self) -> Result<Option<Vec<u8>>, ReadError>;
    pub fn preview(&mut self, spread: u32) -> Result<Option<Vec<u8>>, ReadError>;

    /// Bytes crudos de una entrada; verifica el digest del manifiesto.
    pub fn entry(&mut self, path: &str) -> Result<Vec<u8>, ReadError>;
    pub fn entry_stream(&mut self, path: &str) -> Result<impl Read + '_, ReadError>;
    pub fn resource(&mut self, id: ResourceId) -> Result<Vec<u8>, ReadError>;
    pub fn entries(&self) -> impl Iterator<Item = &FileEntry>;

    /// Parsea el documento completo al modelo. Aquí es donde se paga el coste.
    pub fn document(&mut self, opts: &LoadOptions) -> Result<LoadedDocument, ReadError>;

    /// Consume el lector y devuelve el contexto de preservación (§8) para
    /// poder reescribir el fichero sin perder nada.
    pub fn into_preservation(self) -> PreservationContext;
}

#[derive(Debug, Default, Clone)]
pub struct LoadOptions {
    /// Regenerar los subárboles `xarast:generated` (por defecto: sí).
    pub rebake: bool,
    /// Límites anti zip-bomb / XML malicioso (§10.5).
    pub limits: Limits,
    /// Rechazar en vez de avisar cuando el digest no cuadre.
    pub strict: bool,
}

pub struct LoadedDocument {
    pub document: xarast_model::Document,
    pub preservation: PreservationContext,
    pub diagnostics: Vec<Diagnostic>,     // avisos, no errores
}

// --------------------------------- ESCRITURA ---------------------------------

pub struct XarastWriter<W: Write + Seek> { /* … */ }

#[derive(Debug, Clone)]
pub struct WriteOptions {
    pub profile: Profile,
    pub deflate_level: u8,        // 0..=9
    pub zstd_level: i32,          // -7..=22
    pub deterministic: bool,      // O8: sin timestamps variables, orden fijo
    pub fixed_mtime: Option<OffsetDateTime>,
    pub pretty: bool,             // indentar el SVG (para git)
    pub thumbnail: bool,
    pub previews: bool,
    pub embed_fonts: bool,
    pub materialize_derived: bool, // false = regenerar derivadas al abrir (§4.4)
    pub history: HistoryPolicy,
}

impl<W: Write + Seek> XarastWriter<W> {
    pub fn new(writer: W, opts: WriteOptions) -> Self;

    /// Reinyecta el equipaje desconocido leído previamente (§8). Si se omite,
    /// el resultado NO es un round-trip sin pérdida y `finish()` lo señala.
    pub fn with_preservation(self, ctx: PreservationContext) -> Self;
    pub fn with_thumbnail_provider(self, p: Arc<dyn ThumbnailProvider>) -> Self;
    pub fn with_bake_provider(self, p: Arc<dyn BakeProvider>) -> Self;

    pub fn write_document(&mut self, doc: &xarast_model::Document) -> Result<(), WriteError>;
    pub fn add_resource(&mut self, kind: ResourceKind, bytes: &[u8]) -> Result<ResourceId, WriteError>;
    pub fn finish(self) -> Result<WriteReport, WriteError>;
}

pub struct WriteReport {
    pub bytes_written: u64,
    pub entries: usize,
    pub resources_deduplicated: usize,
    pub bytes_saved_by_dedup: u64,
    pub preservation_complete: bool,
    pub warnings: Vec<Diagnostic>,
}

/// Guardado atómico completo (§10.1): temporal + fsync + rename + fsync del dir.
pub fn save_atomic(
    path: &Path,
    doc: &xarast_model::Document,
    ctx: Option<&PreservationContext>,
    opts: &WriteOptions,
) -> Result<WriteReport, WriteError>;

// --------------------------------- PRESERVACIÓN ---------------------------------

/// Todo lo que el lector no entendió y el escritor DEBE devolver a su sitio.
#[derive(Debug, Default, Clone)]
pub struct PreservationContext {
    /// Atributos y subárboles ajenos, indexados por id de nodo del modelo.
    pub node_baggage: HashMap<NodeId, ForeignBaggage>,
    /// Entradas del ZIP desconocidas, con su flujo comprimido intacto.
    pub unknown_entries: Vec<RawZipEntry>,
    /// Secciones desconocidas de `meta.xml`.
    pub meta_baggage: Vec<ForeignFragment>,
    /// Comentarios e instrucciones de procesamiento con su posición.
    pub comments: Vec<PositionedNode>,
    pub foreign_digest: Option<[u8; 32]>,
    pub foreign_count: usize,
}

impl PreservationContext {
    /// Recalcula el digest canónico (§8.4).
    pub fn digest(&self) -> [u8; 32];
    /// Compara con el digest almacenado; `Err` si se ha perdido algo.
    pub fn verify(&self, stored: &[u8; 32]) -> Result<(), PreservationLoss>;
}

// --------------------------------- BLOQUEO Y RECUPERACIÓN ---------------------------------

pub struct DocumentLock { /* … */ }
impl DocumentLock {
    pub fn acquire(doc_path: &Path) -> Result<Self, LockError>;   // §10.4
    pub fn holder(&self) -> Option<&LockHolder>;
    pub fn steal_if_stale(doc_path: &Path) -> Result<Self, LockError>;
}

pub struct AutosaveSession { /* … */ }
impl AutosaveSession {
    pub fn begin(doc_id: &DocId) -> Result<Self, IoError>;
    pub fn snapshot(&mut self, doc: &xarast_model::Document) -> Result<(), IoError>;
    pub fn journal(&mut self, op: &JournalEntry) -> Result<(), IoError>;
    pub fn discard(self) -> Result<(), IoError>;
    pub fn recoverable() -> Result<Vec<RecoveryCandidate>, IoError>;
}
```

En `xarast-svg`:

```rust
pub struct SvgProfile { /* opciones de serialización */ }

pub fn write_svg(
    doc: &Document, ctx: Option<&PreservationContext>,
    bake: &dyn BakeProvider, opts: &SvgProfile, out: &mut dyn Write,
) -> Result<SvgWriteStats, SvgError>;

pub fn read_svg(
    input: &[u8], limits: &Limits,
) -> Result<(Document, PreservationContext, Vec<Diagnostic>), SvgError>;

/// Horneado: toda la lógica de §5.4 y §6 vive detrás de este trait.
pub trait BakeProvider: Send + Sync {
    fn bake_gradient_profile(&self, g: &GradientSpec) -> Vec<GradientStop>;
    fn bake_effect(&self, e: &LiveEffect, target: &Node) -> BakedSubtree;
    fn bake_fill(&self, f: &Fill, bounds: Rect) -> BakedFill;   // puede rasterizar
    fn bake_stroke(&self, s: &Stroke, path: &Path) -> BakedSubtree;
}
```

### 13.4 Detalles de implementación que hay que acertar

1. **`quick-xml` en modo preservador.** Usar `Reader` con `check_end_names(true)`,
   `trim_text(false)` y **no** expandir entidades. Al escribir, `Writer` con las mismas
   comillas y escapes que el original para los fragmentos preservados: los fragmentos de
   equipaje se reemiten como **bytes crudos** (`Event::Text` con `BytesText::from_escaped`)
   para garantizar identidad byte a byte.
2. **Entrada `mimetype` sin campo extra.** El crate `zip` puede añadir campos extra
   (timestamps extendidos, alineación). Hay que construir esa entrada con
   `SimpleFileOptions::default().compression_method(Stored).last_modified_time(fixed)` y
   **verificar en un test** que el byte 28 es `00 00` y que los bytes 38..64 son el MIME.
   Es un test de una línea que protege la detección de tipo del formato entero.
3. **Copia de flujos comprimidos sin recomprimir.** Para las entradas preservadas y para
   los recursos que no cambian, usar `ZipWriter::raw_copy_file` (evita descomprimir y
   recomprimir megabytes en cada guardado). Es la diferencia entre guardar un documento
   fotográfico en 0,2 s y en 8 s.
4. **Hashing en streaming.** `blake3::Hasher` sobre el lector, sin materializar el
   recurso completo en memoria cuando supere unos pocos MiB.
5. **Escritura determinista.** `deterministic: true` ⇒ fecha DOS fija
   (`1980-01-01 00:00:00`, el mínimo representable), atributos externos fijos (`0o644`),
   sin campos extra, orden de entradas y de atributos canónico. Test: guardar dos veces
   el mismo modelo produce ficheros byte-idénticos.
6. **Límites antes de asignar.** Todos los límites de §10.5 se comprueban **antes** de
   reservar memoria, no después.
7. **Sin `unsafe`.** `#![forbid(unsafe_code)]` en `xarast-format` y `xarast-svg`.

### 13.5 Tests

#### a) Round-trip estructural (obligatorio)

```rust
#[test] fn roundtrip_modelo_identico() {
    for caso in corpus() {
        let doc0 = load(&caso);
        let bytes = save_to_vec(&doc0);
        let doc1 = load_from_bytes(&bytes);
        assert_eq!(doc0.canonical(), doc1.canonical());   // igualdad estructural
    }
}
```

#### b) Round-trip byte-estable (O8)

```rust
#[test] fn guardar_dos_veces_es_identico() {
    let doc = load(caso);
    let a = save_to_vec_deterministic(&doc);
    let b = save_to_vec_deterministic(&load_from_bytes(&a));
    assert_eq!(a, b);            // punto fijo en el primer reguardado
}
```

#### c) Preservación de lo desconocido (§8.7) — **el test más importante**

```rust
#[test] fn preserva_el_100_por_cien_del_equipaje_ajeno() {
    let inyectado = inject_foreign_ns(load_raw(caso), "urn:test:future");
    let doc = load_from_bytes(&inyectado);
    let doc = aplicar_ediciones(doc);        // mover, recolorear, agrupar, deshacer
    let salida = save_to_vec(&doc);
    assert_foreign_intact(&inyectado, &salida, "urn:test:future");
}
```

#### d) Firma y detección

```rust
#[test] fn magic_bytes_en_offsets_fijos() {
    let z = save_to_vec(&Document::empty());
    assert_eq!(&z[0..4],   b"PK\x03\x04");
    assert_eq!(&z[28..30], &[0, 0]);                 // sin campo extra
    assert_eq!(&z[30..38], b"mimetype");
    assert_eq!(&z[38..64], MIME_TYPE.as_bytes());
}
```

#### e) Conformidad de render (§5.6)

Golden tests: para cada fichero del corpus, renderizar con Xarast y con `resvg`,
Chromium y (si está instalado) Inkscape, y comprobar los umbrales SSIM de la tabla de
§5.6. Se almacenan los PNG de referencia en Git LFS.

#### f) Validación de esquema

Ejecutar `xmllint --relaxng` sobre manifiesto, `meta.xml` y SVG de cada fichero generado
(§11.4).

#### g) Deduplicación

```rust
#[test] fn ocho_usos_una_entrada() {
    let doc = documento_con_la_misma_imagen_n_veces(8);
    let rep = save_report(&doc);
    assert_eq!(rep.entries_under("resources/images/"), 1);
    assert!(rep.bytes_saved_by_dedup > 7 * IMG_LEN - 4096);
}
```

#### h) Compresión

```rust
#[test] fn los_jpeg_no_se_recomprimen() {
    let rep = save_report(&documento_con_jpeg());
    assert_eq!(rep.method_of("resources/images/b3-*.jpg"), Method::Stored);
}
```

#### i) Fuzzing (obligatorio desde el día 1)

```
fuzz/fuzz_targets/
  fuzz_open.rs       # XarastReader::open sobre bytes arbitrarios
  fuzz_document.rs   # parseo de document.svg sobre XML arbitrario
  fuzz_roundtrip.rs  # load → save → load, comprobando que no hay pánico ni divergencia
```
Criterio de aceptación: 24 h de `cargo fuzz` sin hallazgos antes de cada release.

#### j) Propiedades (`proptest`)

Generar documentos aleatorios del modelo y comprobar:
`load(save(d)) == d`, `save(load(save(d))) == save(d)`, y que ninguna combinación de
opciones de escritura cambia el modelo recuperado.

#### k) Interoperabilidad con Inkscape (manual, en cada release)

Abrir en Inkscape, mover un objeto, guardar, reabrir en Xarast: las capas, las guías y
**todas** las extensiones `xarast:` deben seguir ahí (es la comprobación real de §8.4).

### 13.6 Orden de implementación sugerido

| Paso | Entrega | Fase de proyecto |
|---|---|---|
| 1 | Contenedor: leer/escribir ZIP, `mimetype`, manifiesto, `meta.xml`, tests (d) y (b) | v0.1 |
| 2 | Perfil SVG **núcleo**: paths, formas, grupos, capas, rellenos planos y degradados lineales/radiales, trazo, bitmaps; tests (a), (f) | v0.1 |
| 3 | Preservación y round-trip de lo desconocido; tests (c) | v0.1 (**no posponer**: es mucho más caro añadirlo después) |
| 4 | Deduplicación, política de compresión, escritura atómica, bloqueo, autoguardado; tests (g), (h) | v0.1 |
| 5 | Texto, recortes, máscaras, paleta, CMYK | v0.2 |
| 6 | Efectos vivos y horneado (`BakeProvider` completo), rellenos exóticos | v0.3 |
| 7 | `history/`, perfil `compact` (zstd), layout partido | v1.0 |

> **Nota de diseño crítica:** el paso 3 **DEBE** ir en la v0.1, aunque no haya nada que
> preservar todavía. Si el modelo de documento no nace con el contenedor de equipaje en
> cada nodo, añadirlo después obliga a tocar todo el modelo, todo el parser y todo el
> serializador. Es la lección directa de §2.4.

---

## 14. Apéndices

### 14.1 Resumen de decisiones y alternativas descartadas

| Decisión | Alternativa descartada | Motivo |
|---|---|---|
| Contenedor ZIP | Tar+zstd, SQLite, formato propio | Interoperabilidad, herramientas ubicuas, acceso aleatorio |
| `mimetype` STORED primero | Sólo extensión de fichero | Detección por magic sin descomprimir |
| Manifiesto XML | `manifest.json` | Homogeneidad de la cadena XML, validación con `xmllint`, espacios de nombres (§3.5) |
| Un `document.svg` | Un SVG por spread; `content.xml`+`styles.xml` | Apertura directa en navegador; sin referencias cruzadas |
| Extensiones por espacio de nombres | Atributos `data-*`; blob binario adjunto | Estándar, ignorables por construcción, legibles |
| Doble representación con horneado | Sólo paramétrico; sólo horneado | Cumple O1 y O2 simultáneamente |
| Unidad = punto PostScript | Píxel CSS; milipunto directo | Precisión exacta de milipunto con 3 decimales |
| Eje Y hacia abajo | `scale(1,-1)` global | No rompe texto, gradientes ni filtros |
| BLAKE3-256 | SHA-256, xxHash, CRC | Rápido y criptográficamente sólido |
| Deflate por defecto, zstd opcional | zstd siempre | Interoperabilidad de lectura con herramientas comunes |
| Binarios STORED | Recomprimir todo | Coste de CPU sin ahorro |
| Journal fuera del paquete | Journal dentro del ZIP | Reescribir el ZIP por operación es inviable |

### 14.2 Niveles de conformidad

| Nivel | Nombre | Un **lector** de este nivel… | Un **escritor** de este nivel… |
|---|---|---|---|
| **A** | Núcleo | Abre el contenedor, lee manifiesto y metadatos, y renderiza el SVG base. No entiende `xarast:` pero **DEBE** preservarlo (§8) | Produce contenedor y SVG base válidos, con `xarast:` mínimo (documento, páginas, capas) |
| **B** | Completo | Entiende todo el vocabulario `xarast:` de v1.0, regenera el horneado, edita con fidelidad total | Emite el vocabulario completo y el horneado de §5.4 |
| **C** | Archivo | Nivel B + verifica todos los digests, exige fuentes incrustadas y texto duplicado en curvas, rechaza referencias a recursos ausentes | Nivel B + incrusta fuentes y perfiles ICC, materializa todas las derivadas, emite `history/` vacío y `README.txt` |

Xarast v0.1 apunta al nivel **B** para el subconjunto de v0.1 y al nivel **A** completo.
Herramientas de terceros (importadores, visores, indexadores) tienen en el nivel **A** un
objetivo alcanzable en pocas horas de trabajo.

### 14.3 Registro de `xarast:kind` de efectos (v1.0)

Efectos con horneado a filtro SVG definido. Ampliar esta tabla **NO** eleva
`min-reader` (§7.4 regla 4).

| `xarast:kind` | Parámetros | Horneado |
|---|---|---|
| `gaussian-blur` | `radius` | `feGaussianBlur` |
| `sharpen` / `unsharp` | `radius`, `amount` | `feConvolveMatrix` o `feGaussianBlur`+`feComposite arithmetic` |
| `levels` | `black`, `white`, `gamma` | `feComponentTransfer type="gamma"` + `linear` |
| `brightness-contrast` | `brightness`, `contrast` | `feComponentTransfer type="linear"` |
| `hue-saturation` | `hue`, `saturation`, `lightness` | `feColorMatrix type="hueRotate"` + `saturate` |
| `greyscale` | `method` | `feColorMatrix type="saturate" values="0"` |
| `sepia` | `amount` | `feColorMatrix` |
| `invert` | — | `feComponentTransfer type="table" tableValues="1 0"` |
| `posterize` | `levels` | `feComponentTransfer type="discrete"` |
| `noise` | `amount`, `seed` | `feTurbulence` + `feComposite` |
| `emboss` | `angle`, `depth` | `feConvolveMatrix` |
| `displace` | `map`, `scale` | `feDisplacementMap` |

### 14.4 Lista de comprobación de conformidad de un `.xarast`

- [ ] Primera entrada = `mimetype`, STORED, sin campo extra, 26 bytes exactos
- [ ] Bytes 0..4 = `PK\x03\x04`; 30..38 = `mimetype`; 38..64 = el tipo MIME
- [ ] `META-INF/manifest.xml` presente, valida contra `manifest.rnc`
- [ ] Entrada `/` del manifiesto con `media-type` = contenido de `mimetype`
- [ ] Una entrada de manifiesto por cada entrada del ZIP; sin duplicados
- [ ] Digests BLAKE3 correctos para `document`, `meta` y todo `resource`
- [ ] `document.svg` bien formado, valida contra SVG 1.1, sin `<script>`, sin DTD
- [ ] Todas las referencias a recursos son relativas y resuelven dentro del paquete
- [ ] `id` únicos en todo el documento
- [ ] Cada subárbol `xarast:generated` tiene un `xarast:generated-by` que resuelve
- [ ] Ningún nombre de entrada con `..`, `\`, `/` inicial o caracteres de control
- [ ] Ratio de descompresión de cada entrada < 200:1
- [ ] SSIM medio del corpus ≥ 0,90 contra el render nativo

### 14.5 Trabajo futuro (v1.1+)

1. **Deltas en `history/`** (`*.vcdiff`) en lugar de instantáneas completas.
2. **Firma digital** (`META-INF/signatures.xml`, XMLDSig sobre los digests del
   manifiesto) y **cifrado** (`META-INF/encryption.xml`, AES-GCM por entrada).
3. **Modelo plano con `nodeChanges`** para sincronización incremental y colaboración,
   aprovechando los IDs estables de §5.7 (lección de Figma, §2.5).
4. **Registro IANA** del tipo `application/vnd.xarast+zip`.
5. **Perfil `web`**: variante del escritor que produce un SVG optimizado para servir
   directamente (sin extensiones, con `<style>` consolidado y `viewBox` recortado).
6. **Mallas de color reales** si `svg-next` estabiliza `<meshgradient>`; hasta entonces,
   horneado (§6.3).

### 14.6 Referencias

- OASIS, *Open Document Format for Office Applications v1.2/1.3, Part 3: Packages* —
  https://docs.oasis-open.org/office/v1.2/cs01/OpenDocument-v1.2-cs01-part3.html
- Krita, *file_kra* — https://docs.krita.org/en/general_concepts/file_formats/file_kra.html
  y https://github.com/2shady4u/godot-kra-psd-importer/blob/master/docs/KRA_FORMAT.md
- Scribus, estructura `.sla` — http://justsolve.archiveteam.org/wiki/Scribus
- Inkscape, *Inkscape-specific XML attributes* —
  https://wiki.inkscape.org/wiki/Inkscape-specific_XML_attributes
  y *Inkscape SVG vs. plain SVG* — https://wiki.inkscape.org/wiki/Inkscape_SVG_vs._plain_SVG
- W3C, *SVG 1.1 (Second Edition), 23 Extensibility* —
  https://www.w3.org/TR/2011/REC-SVG11-20110816/extend.html
- Libre Arts, *Gradient meshes and hatching to be removed from SVG 2.0* —
  https://librearts.org/2018/05/gradient-meshes-and-hatching-to-be-removed-from-svg-2-0/
- Figma `.fig` / Kiwi — https://github.com/OpenFig-org/openfig-core/blob/main/docs/research.md
- Xara, *Xar Format Specification* — http://site.xara.com/support/docs/webformat/spec/XARFormatDocument.pdf
- freedesktop.org, *Shared MIME-info Database* —
  https://specifications.freedesktop.org/shared-mime-info-spec/latest-single/
- Wikipedia, *ZIP (file format)* — métodos de compresión y el cambio del ID 20 al 93 para
  Zstandard — https://en.wikipedia.org/wiki/ZIP_(file_format)
- crate `zip` — https://docs.rs/zip/latest/zip/enum.CompressionMethod.html
- crate `blake3` — https://docs.rs/blake3
- Código fuente original: `/home/user/xara-xtreme/Kernel/cxftags.h` (209 tags),
  `Kernel/fillval.h` (`FILLSHAPE_*`, `RepeatType`, `TranspType`),
  `Kernel/nodershp.h` (`NodeRegularShape`), `Kernel/doccoord.h` (milipuntos),
  `Mime/xaralx.xml`, `xaralx.desktop`.
