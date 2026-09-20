# Formato de fichero `.xar` (CXF / Camelot eXchange Format) — especificación técnica

> **Nota de sala limpia.** Este documento describe el *comportamiento* y los
> *formatos de datos* de Xara Xtreme (GPL-2.0-only) con fines de
> interoperabilidad: layout binario del contenedor `.xar`, numeración de tags y
> semántica de cada record. No reproduce código fuente del original; las
> referencias `fichero:línea` apuntan al árbol de referencia en `xara-xtreme/` y
> sirven solo para localizar la lógica descrita. Xarast se implementa desde esta
> especificación, no traduciendo el original.

> **Estado:** documento normativo de referencia para implementar el importador `.xar` en Rust.
> **Origen:** ingeniería inversa del código fuente original de **Xara Xtreme / Xara LX**
> (`/home/user/xara-xtreme`, C++, GPL v2), validada contra 59 ficheros `.xar` reales del
> propio repositorio (`testfiles/`, `Designs/`, `Templates/`, `TextDesigns/`).
> Todas las referencias cruzadas tienen la forma `fichero:línea` relativa a la raíz del
> árbol original (por ejemplo `Kernel/cxfile.cpp:1699`).
>
> **Nomenclatura:** el formato se llama internamente *v2 file format* o *CXF*. La extensión
> `.xar` corresponde al formato nativo (`CXN`) y `.web` al formato web (`CXW`/`CXM`);
> ambos comparten **exactamente** la misma estructura binaria (ver §1.4).

---

## Índice

1. [Estructura física del fichero](#1-estructura-física-del-fichero)
2. [Estructura de record](#2-estructura-de-record)
3. [Compresión](#3-compresión)
4. [Tabla completa de tags y layouts](#4-tabla-completa-de-tags-y-layouts)
5. [Sistema de coordenadas y unidades](#5-sistema-de-coordenadas-y-unidades)
6. [Modelo de árbol, definiciones y referencias](#6-modelo-de-árbol-definiciones-y-referencias)
7. [Representación de caminos (paths)](#7-representación-de-caminos-paths)
8. [Atributos y su modelo de herencia](#8-atributos-y-su-modelo-de-herencia)
9. [Colores](#9-colores)
10. [Prioridades de implementación](#10-prioridades-de-implementación)
11. [Riesgos y ambigüedades conocidas](#11-riesgos-y-ambigüedades-conocidas)
12. [Apéndices](#12-apéndices)

---

## 1. Estructura física del fichero

### 1.1. Visión general

Un fichero `.xar` es:

```
+-------------------------------------------------------------+
| 8 bytes de firma (magic)                                     |
+-------------------------------------------------------------+
| Secuencia de records, cada uno: tag u32, size u32, datos     |
|   ...                                                        |
|   TAG_STARTCOMPRESSION  -> a partir de aquí flujo deflate    |
|   ...records comprimidos...                                  |
|   TAG_ENDCOMPRESSION    -> seguido de 8 bytes CRC+tamaño     |
|   ...                                                        |
|   TAG_ENDOFFILE                                              |
+-------------------------------------------------------------+
```

No hay tabla de contenidos, ni índice, ni offsets absolutos: **el fichero es un flujo
secuencial de records**. El único "puntero" del formato es el *número de record*
(posición ordinal dentro del fichero), usado para referenciar definiciones (§6.3).

### 1.2. Firma / magic bytes

Definición: `Kernel/cxfdefs.h:109-110`

| Constante | Valor (`u32`) | Definida en |
|---|---|---|
| `CXF_IDWORD1` | `0x41524158` | `Kernel/cxfdefs.h:109` |
| `CXF_IDWORD2` | `0x0A0DA3A3` | `Kernel/cxfdefs.h:110` |

Se escriben como dos `u32` **little-endian** (`Kernel/cxfile.cpp:241-242`), de modo que
los 8 primeros bytes del fichero son exactamente:

```
offset 0: 58 41 52 41 A3 A3 0D 0A        ("XARA" + A3 A3 CR LF)
```

Validado en los 59 ficheros del corpus. La detección de formato (`HowCompatible`,
`Kernel/camfiltr.cpp:1639-1690`) exige estos 8 bytes; opcionalmente comprueba que el
record siguiente sea `TAG_FILEHEADER` (2) con `size > 3` y lee los 3 primeros bytes de su
payload como identificador de tipo (`CXN`/`CXW`/`CXM`).

La secuencia `A3 A3 0D 0A` es la típica trampa anti-transferencia-ASCII (si un FTP en modo
texto convierte CRLF↔LF, la firma deja de validar).

### 1.3. Orden de bytes (endianness)

**Todo el formato es little-endian**, sin excepción, en todos los tipos multibyte:

* `Kernel/cxfile.cpp:875-905` — `CXaraFile::Read(UINT32*)` aplica `LEtoNative()`.
* `Kernel/cxfrec.cpp:597-676` — `WriteUINT32/INT32/UINT16/INT16/FLOAT/DOUBLE/FIXED16/ANGLE`
  aplican `NativetoLE()`.
* `Kernel/cxfrec.cpp:678-686` — `WriteWCHAR` escribe UTF-16 **LE**.

Única excepción de "orden raro": las coordenadas *intercaladas* (interleaved) de los
caminos relativos, que escriben los bytes de X e Y alternados de más significativo a menos
significativo (§7.3). No es big-endian del fichero: es una permutación deliberada dentro de
un campo de 8 bytes, hecha para mejorar la compresión zlib
(`Kernel/cxfrec.cpp:752-790`).

### 1.4. Cabecera lógica: `TAG_FILEHEADER` (tag 2)

Escritura: `Kernel/camfiltr.cpp:3749-3768` · Lectura: `Kernel/cxfile.cpp:2520-2547`

Es el **primer record** del fichero, siempre **sin comprimir**.

| Offset | Tipo | Campo | Notas |
|---|---|---|---|
| 0 | `[u8;3]` | `file_type` | ASCII, **sin terminador**: `"CXN"`, `"CXW"` o `"CXM"` |
| 3 | `u32` | `file_size` | Tamaño **descomprimido** total del fichero, en bytes (se usa para la barra de progreso). Se escribe como 0 y se parchea al final de la exportación: `Kernel/camfiltr.cpp:4115-4125` |
| 7 | `u32` | `native_web_link_id` | Identificador de enlace entre el fichero nativo y su versión web. Siempre 0 en el corpus |
| 11 | `u32` | `precompression_flags` | Flags de "precompresión". **Debe ser 0**; cualquier otro valor hace que el importador aborte con `IDS_UNKNOWN_COMPRESSION` (`Kernel/camfiltr.cpp:832-845`) |
| 15 | ASCII-Z | `producer` | p. ej. `"Xara X"` |
| … | ASCII-Z | `producer_version` | p. ej. `"3.0"` |
| … | ASCII-Z | `producer_build` | p. ej. `"0.2704 (MarkG)"` |

Las tres cadenas son ASCII terminadas en `NUL` (§2.4). El tamaño del record es variable
(36–47 bytes en el corpus).

Ejemplo real (`testfiles/OneLine.xar`, record #1, size 41):

```
43 58 4E | 9C 0F 00 00 | 00 00 00 00 | 00 00 00 00 |
"Xara X\0" "3.0\0" "0.2704 (MarkG)\0"
 -> type="CXN", file_size=3996, link=0, precomp=0
```

### 1.5. Versiones

El formato **no tiene un número de versión global**. La "versión" se deduce de tres cosas:

1. **Rango del tag.** `Kernel/cxftags.h:109-114` documenta los rangos asignados:
   `0–3999` = Camelot v1.5 (primera versión del formato, **congelada**),
   `4000–4999` = Camelot v2.0 y posteriores (Xara X, Xara X1/X2, XaraLX).
2. **Cadenas del `TAG_FILEHEADER`** (`producer`, `producer_version`, `producer_build`):
   es la única "versión de programa" almacenada, y es puramente informativa.
3. **Tamaño real de cada record.** Varios records crecieron entre versiones (p. ej.
   `TAG_LINEARFILL` pasó de 24 a 40 bytes al añadir el perfil bias/gain, ver
   `Kernel/cxfdefs.h:253`; `TAG_TEXT_STORY_SIMPLE` de 8 a 12 al añadir el flag de autokern,
   `Kernel/cxfdefs.h:470`). **El lector debe tolerar records más cortos de lo esperado y
   rellenar con valores por defecto**: el código original hace exactamente eso mediante
   lecturas "noError" (`ReadINT32noError`, `ReadDOUBLEnoError`,
   `Kernel/cxfrec.cpp:1290,1342`; uso en `Kernel/rechtext.cpp:756-770`).

Además existe una versión **del subsistema de compresión** dentro de
`TAG_STARTCOMPRESSION` (§3.2).

### 1.6. Diferencias entre `.xar` (nativo) y `.web`

Identificadores (`Kernel/camfiltr.h:143-145`):

| Constante del original | Valor (3 caracteres ASCII) | Formato que identifica |
|---|---|---|
| `EXPORT_FILETYPE_WEB` | `CXW` | formato web (`.web`) |
| `EXPORT_FILETYPE_MIN` | `CXM` | formato web «mínimo» |
| `EXPORT_FILETYPE_NATIVE` | `CXN` | formato nativo (`.xar`) |

Este identificador es el campo `type` del `TAG_FILEHEADER` (§1.4).

* **La estructura binaria es idéntica.** El mismo lector sirve para los tres.
* La diferencia es *qué* records se emiten. El exportador llama a
  `WritePreChildrenWeb()` o `WritePreChildrenNative()` según el filtro
  (`Kernel/camfiltr.cpp:4532-4545`). En la inmensa mayoría de las clases,
  `WritePreChildrenNative()` simplemente delega en la versión Web
  (p. ej. `Kernel/fillattr.cpp:5981-5987`), así que el contenido coincide.
* Lo que **solo** aparece en nativo: información de documento que no afecta al render
  (p. ej. `TAG_DOCUMENTFLAGS`, `Kernel/infocomp.cpp:736-738` comprueba `IsWebFilter()` y
  no lo escribe en web), tamaño de undo, comentarios, vistas, etc.
* El formato `CXM` ("minimal web") indica que el documento necesita una plantilla por
  defecto al cargarse (`Kernel/webfiltr.cpp:383-390`).
* Existe además un **formato de texto** (`CXaraTemplateFile`, `Kernel/cxftfile.h:106-140`)
  usado para plantillas Flare: traduce cada record a texto y los datos binarios a BinHex.
  **Fuera del alcance de este documento.**

**Recomendación para Rust:** aceptar `CXN`, `CXW` y `CXM` con el mismo parser y exponer el
tipo detectado en la API.

---

## 2. Estructura de record

### 2.1. Layout binario

Escritura: `Kernel/cxfile.cpp:1699-1717` y `Kernel/cxfile.cpp:1636-1657`
Lectura: `Kernel/cxfile.cpp:1855-1866`

```
+--------+--------+-------------------------------+
| tag:u32| size:u32|  payload: size bytes          |
+--------+--------+-------------------------------+
```

* `tag`: `u32` little-endian. Identifica el tipo de record.
* `size`: `u32` little-endian. **Tamaño del payload en bytes, sin contar los 8 bytes de
  cabecera.** Puede ser 0.
* `payload`: exactamente `size` bytes.

**No hay alineación ni padding de ningún tipo.** Los records se suceden sin huecos y los
campos dentro del payload tampoco están alineados (p. ej. `TAG_SPREADINFORMATION` tiene un
`u8` al final de cuatro `i32`; `TAG_DEFINECOMPLEXCOLOUR` empieza con 5 bytes sueltos
seguidos de `u32`). **Un `#[repr(C)]` de Rust NO sirve para mapear payloads**: hay que leer
campo a campo (o usar `#[repr(C, packed)]` con mucho cuidado y conversión explícita de
endianness).

El valor `CXF_UNKNOWN_SIZE = -1` (`Kernel/cxfdefs.h:111`) **nunca aparece en el fichero**:
es un marcador interno del escritor que significa "el tamaño se calcula al cerrar el
record". En el fichero el tamaño siempre es el real.

### 2.2. Records de tamaño variable

Tres mecanismos producen tamaño variable:

1. **Cadenas embebidas** (nombres de capa, de color, de fuente, URLs…): terminadas en NUL,
   el tamaño del record las incluye.
2. **Arrays con contador explícito**: p. ej. la rampa de colores de los degradados
   multietapa (`u32 n` seguido de `n` pares), `Kernel/fillattr.cpp:6837-6843`.
3. **Arrays *sin* contador, deducidos del tamaño del record**: el caso más importante es
   el de los caminos relativos, donde `num_coords = size / 9`
   (`Kernel/cxfrec.cpp:1837`). También `TAG_ATOMICTAGS` (`size/4` tags,
   `Kernel/cxfile.cpp:2574`) y `TAG_PATH_FLAGS` (`size` = número de puntos).

### 2.3. Records desconocidos: cómo se ignoran

`Kernel/cxfile.cpp:1997-2065` (`CXaraFile::ReadNextRecord`):

```text
si no existe un manejador registrado para el tag leído:
    consumir los `size` bytes del payload sin interpretarlos
    continuar con el record siguiente
```

Es decir: **saltar `size` bytes y continuar**. Pero hay dos matices críticos
(`Kernel/camfiltr.cpp:5293-5312`, `BaseCamelotFilter::UnrecognisedTag`):

* Si el tag está en la lista **esencial** (`TAG_ESSENTIALTAGS`, tag 11) → **abortar la
  importación**: el fichero no se puede representar fielmente.
* Si el tag está en la lista **atómica** (`TAG_ATOMICTAGS`, tag 10) → hay que **descartar
  también todo su subárbol** (`StripNextSubTree()`, `Kernel/cxfile.cpp:620-632`),
  es decir, ignorar todos los records hasta el `TAG_UP` que cierra el `TAG_DOWN` que sigue
  al record desconocido. Motivo: si no entiendes un nodo compuesto (bisel, contorno,
  sombra, ClipView, efecto vivo), sus hijos son datos *derivados* que no deben insertarse
  sueltos en el árbol.
* En cualquier otro caso → ignorar solo ese record y seguir, acumulando un aviso al usuario.

La lista atómica que escribe Xara LX (`Kernel/camfiltr.cpp:6996-7013`) es:

```
TAG_BEVEL(4052) TAG_BEVELINK(4057) TAG_CONTOURCONTROLLER(4066) TAG_CONTOUR(4067)
TAG_SHADOWCONTROLLER(4050) TAG_SHADOW(4051) TAG_CLIPVIEWCONTROLLER(4084) TAG_CLIPVIEW(4085)
TAG_CURRENTATTRIBUTES(4119) TAG_LIVE_EFFECT(4125) TAG_LOCKED_EFFECT(4126)
TAG_FEATHER_EFFECT(4127)
```

En el corpus aparecen **739 records `TAG_ATOMICTAGS` de 4 bytes cada uno** (uno por tag),
no un único record con la lista completa: el lector debe acumularlos todos
(`Kernel/cxfile.cpp:2562-2585` recorre `size/4` entradas por record).

### 2.4. Tipos de datos primitivos del payload

Todos definidos en `Kernel/cxfrec.cpp` (escritura ~líneas 597-1100, lectura ~1148-1660).

| Nombre CXF | Bytes | Codificación | Rust |
|---|---|---|---|
| `BYTE` | 1 | entero sin signo | `u8` |
| `UINT16` / `INT16` | 2 | LE | `u16` / `i16` |
| `UINT32` / `INT32` | 4 | LE | `u32` / `i32` |
| `REFERENCE` | 4 | LE, **con signo**: >0 = número de record; <0 = referencia predefinida; 0 = error/nulo | `i32` |
| `FLOAT` | 4 | IEEE-754 binary32 LE | `f32` |
| `DOUBLE` | 8 | IEEE-754 binary64 LE | `f64` |
| `FIXED16` | 4 | `i32` con punto binario entre los bits 15 y 16 → valor real = `raw / 65536.0` (`Kernel/ccmaths.h:116`, `Kernel/fixed16.h`) | `i32` + helper |
| `ANGLE` | 4 | alias de `FIXED16`, en radianes (`Kernel/ccmaths.h:120`) | `i32` + helper |
| `FIXED24` | 4 | `i32` con 24 bits fraccionarios → `raw / 16777216.0` (`Kernel/fixed24.h:157,264`). Solo aparece en componentes de color | `i32` + helper |
| `DocCoord` | 8 | dos `INT32` (x, y) en millipoints (§5) | `(i32,i32)` |
| `DocCoord` intercalada | 8 | bytes de x/y alternados (§7.3) | idem |
| `Matrix` | 24 | `FIXED16 a, b, c, d` + `INT32 e, f` (`Kernel/cxfrec.cpp:1913-1959`) | ver §5.4 |
| `ASCII-Z` | var | bytes ASCII terminados en `0x00` (`Kernel/cxfrec.cpp:1546-1562`) | `CString`-like |
| `UNICODE-Z` | var | UTF-16 **LE**, terminado en `0x0000` (2 bytes). Constante `SIZEOF_XAR_UTF16 = 2` (`Kernel/cxfile.h:127`) | `Vec<u16>` → `String` |
| `UTF16STR` | var | idéntico a `UNICODE-Z` pero sin límite de longitud (`Kernel/cxfrec.cpp:1034-1058`) | idem |
| `CCPanose` | 10 | 10 bytes PANOSE (family, serif, weight, proportion, contrast, strokeVar, armStyle, letterform, midline, xHeight) (`Kernel/cxfrec.cpp:1416-1447`) | `[u8;10]` |
| `RGBTRIPLE` | 3 | R, G, B (solo en paletas de `TAG_DEFINEBITMAP_JPEG8BPP`) | `[u8;3]` |

> **Atención:** `TAG_TEXT_STRING` (2201) es la única cadena que **no** lleva terminador:
> su longitud es `size / 2` caracteres UTF-16. Verificado en `TextDesigns/SimpleText.xar`
> (size 38 = 19 caracteres, "Single line of text", sin NUL).

### 2.5. Pseudocódigo Rust del lector de records

```rust
pub const XAR_MAGIC: [u8; 8] = [0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];

#[derive(Debug, Clone)]
pub struct Record {
    pub number: u32,   // 1..N, orden en el fichero; es la clave de las referencias
    pub tag: u32,
    pub data: Vec<u8>, // longitud == size
}

/// Cursor sobre el payload de un record: TODO es little-endian.
pub struct Cur<'a> { b: &'a [u8], p: usize }

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Self { Cur { b, p: 0 } }
    pub fn remaining(&self) -> usize { self.b.len() - self.p }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Err> {
        if self.remaining() < n { return Err(Err::Eof); }
        let s = &self.b[self.p..self.p + n]; self.p += n; Ok(s)
    }
    pub fn u8(&mut self)  -> Result<u8, Err>  { Ok(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Result<u16, Err> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    pub fn i16(&mut self) -> Result<i16, Err> { Ok(self.u16()? as i16) }
    pub fn u32(&mut self) -> Result<u32, Err> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn i32(&mut self) -> Result<i32, Err> { Ok(self.u32()? as i32) }
    pub fn f32(&mut self) -> Result<f32, Err> { Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn f64(&mut self) -> Result<f64, Err> { Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap())) }

    pub fn fixed16(&mut self) -> Result<f64, Err> { Ok(self.i32()? as f64 / 65536.0) }
    pub fn fixed24(&mut self) -> Result<f64, Err> { Ok(self.i32()? as f64 / 16_777_216.0) }
    pub fn angle(&mut self)   -> Result<f64, Err> { self.fixed16() }          // radianes
    pub fn reference(&mut self) -> Result<i32, Err> { self.i32() }

    pub fn coord(&mut self) -> Result<Coord, Err> { Ok(Coord { x: self.i32()?, y: self.i32()? }) }

    /// 8 bytes con los bytes de X e Y intercalados, de MSB a LSB.
    pub fn coord_interleaved(&mut self) -> Result<Coord, Err> {
        let b = self.take(8)?;
        let x = i32::from_be_bytes([b[0], b[2], b[4], b[6]]);
        let y = i32::from_be_bytes([b[1], b[3], b[5], b[7]]);
        Ok(Coord { x, y })
    }

    pub fn matrix(&mut self) -> Result<Matrix, Err> {
        Ok(Matrix { a: self.fixed16()?, b: self.fixed16()?, c: self.fixed16()?,
                    d: self.fixed16()?, e: self.i32()?,     f: self.i32()? })
    }

    pub fn ascii_z(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        loop { let c = self.u8()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf8_lossy(&v).into_owned())
    }

    pub fn utf16_z(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        loop { let c = self.u16()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf16_lossy(&v))
    }

    /// Resto del record como UTF-16 sin terminador (TAG_TEXT_STRING).
    pub fn utf16_rest(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        while self.remaining() >= 2 { let c = self.u16()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf16_lossy(&v))
    }
}
```

---

## 3. Compresión

### 3.1. Qué está comprimido y qué no

* Los 8 bytes de firma: **nunca** comprimidos.
* `TAG_FILEHEADER` y, típicamente, el bitmap de previsualización: **sin comprimir**
  (para que un visor pueda leerlos sin inflar nada).
* Desde el final del record `TAG_STARTCOMPRESSION` (tag 30) hasta el final del record
  `TAG_ENDCOMPRESSION` (tag 31): **flujo deflate crudo**.
* Los **records "streamed"** (definiciones de bitmap y sonido) **fuerzan el cierre del
  bloque comprimido antes de escribirse y lo reabren después**
  (`Kernel/cxfile.cpp:1437-1465` y `Kernel/cxfile.cpp:1550-1570`). Por eso en el corpus
  hay 103 pares START/END en 59 ficheros: los documentos con bitmaps tienen varios
  bloques comprimidos.

Un parser genérico **no necesita conocer los records streamed**: basta con implementar
correctamente START/END y todo encaja de forma natural. (Confirmado: el parser de
validación del §12.1 procesa sin excepciones los 59 ficheros, incluidos los que llevan
JPEG/PNG embebidos.)

### 3.2. `TAG_STARTCOMPRESSION` (30), payload de 4 bytes

`Kernel/cxfile.cpp:705-742`:

El escritor original compone el valor como `major * 100 + minor` a partir de las dos
constantes de versión de zlib que lleva el propio árbol, y lo emite como un único `u32`.

| Offset | Tipo | Campo |
|---|---|---|
| 0 | `u32` | `compression_version`: byte alto = **tipo de compresión** (0 = zlib/deflate, único valor conocido); resto = versión `major*100 + minor` de zlib |

`Kernel/zutil.h:144-145` define `ZLIB_MAJOR_VERSIONNO = 0`, `ZLIB_MINOR_VERSIONNO = 99`,
por lo que el valor escrito siempre es **99** (`0x00000063`). En los 59 ficheros del corpus
el valor es 99 sin excepción. El lector original solo hace *trace* de este valor y no lo
valida (`Kernel/cxfile.cpp:660-670`); se recomienda comprobar que el byte alto sea 0 y
avisar si no.

Inmediatamente **después** de los 4 bytes de payload comienza el flujo deflate.

### 3.3. Algoritmo exacto

`Kernel/zstream.cpp:490-545` (inicialización) y `Kernel/zstream.cpp:760-770`:

El original inicializa zlib con los parámetros siguientes (los nombres son los de la
API pública de zlib, no del original):

| Sentido | Parámetro | Valor usado | Efecto |
|---|---|---|---|
| escritura | `level` | `Z_DEFAULT_COMPRESSION` (6) | irrelevante para el lector |
| escritura | `method` | `Z_DEFLATED` | DEFLATE |
| ambos | `windowBits` | `-MAX_WBITS` (`-15`) | **flujo crudo**: sin cabecera zlib ni gzip |
| escritura | `memLevel` | `DEF_MEM_LEVEL` (8) | uso de memoria del compresor |
| escritura | `strategy` | `Z_DEFAULT_STRATEGY` (0) | estrategia por defecto |

* Algoritmo: **DEFLATE (RFC 1951) crudo, sin cabecera zlib ni gzip** (`windowBits`
  negativo = `-15`).
* Nivel: `Z_DEFAULT_COMPRESSION` (6). Irrelevante para el lector.
* Estrategia: por defecto; `memLevel` = `DEF_MEM_LEVEL` (8).
* En Rust: `flate2::Decompress::new(false)` / `DeflateDecoder`, o
  `miniz_oxide::inflate::stream` con `wrap = false`.

> Hay una variante con cabecera zlib (`MAX_WBITS` positivo) en `gz_init` cuando el
> parámetro `Header` es cierto (`Kernel/zstream.cpp:493-500`), pero **no se usa para `.xar`**:
> `CCStreamFile::InitCompression(Header=FALSE)` es el camino que toma el filtro nativo
> (`Kernel/ccfile.h:636`, `Kernel/cxfile.cpp:738`).

### 3.4. `TAG_ENDCOMPRESSION` (31), payload de 8 bytes = CRC + tamaño

Esta es la parte más sutil del formato, y hay que implementarla con exactitud
(`Kernel/cxfile.cpp:789-816` + `Kernel/zstream.cpp:1256-1275`):

1. La **cabecera** del record `TAG_ENDCOMPRESSION` (tag=31, size=8) se escribe **dentro del
   flujo comprimido** (mediante `WriteRecordHeader`, que escribe a través del stream
   activo).
2. Acto seguido se cierra el deflate (`Z_FINISH` + flush) y se escriben, **ya sin
   comprimir, directamente en el fichero**, 8 bytes:

| Offset | Tipo | Campo |
|---|---|---|
| 0 | `u32` LE | `crc32` de **todos los bytes descomprimidos** del bloque |
| 4 | `u32` LE | número de bytes **descomprimidos** del bloque (`stream.total_in`) |

`putLong` escribe en orden LSB-first (`Kernel/zstream.cpp:1205-1217`), `getLong` lo lee
igual (`Kernel/zstream.cpp:1228-1240`).

El CRC es el **CRC-32 estándar de zlib** (polinomio 0xEDB88320, init 0, xorout final;
`Kernel/zstream.cpp:1550+`, tabla clásica de zlib), calculado sobre el contenido *plano*
(lo que el lector obtiene al inflar), incluyendo la cabecera de 8 bytes del propio
`TAG_ENDCOMPRESSION`.

La comprobación del lector original (`Kernel/zstream.cpp:1298-1310`) es:
`crc_leído == crc_calculado && tamaño_leído == total_out`.

**Validación empírica:** el parser del §12.1 recalcula CRC y longitud en los 59 ficheros
del corpus y coincide en el 100 % de los 103 bloques comprimidos.

### 3.5. Máquina de estados de descompresión (Rust)

```rust
enum Stream { Plain, Deflate(flate2::Decompress) }

pub struct XarReader<'a> {
    raw: &'a [u8],
    pos: usize,          // posición en el fichero físico
    inflated: Vec<u8>,   // buffer de salida del inflate
    ipos: usize,         // posición dentro de `inflated`
    st: Stream,
    crc: u32,            // CRC acumulado del bloque actual
    total_in: u32,       // bytes inflados del bloque actual
}

impl<'a> XarReader<'a> {
    fn read(&mut self, n: usize) -> Result<Vec<u8>, Err> { /* lee de raw o de inflated */ }

    fn start_compression(&mut self) {
        self.st = Stream::Deflate(flate2::Decompress::new(/*zlib_header=*/false));
        self.inflated.clear(); self.ipos = 0; self.crc = 0; self.total_in = 0;
    }

    /// Se llama TRAS haber leído la cabecera (tag=31,size=8) desde el flujo inflado.
    fn end_compression(&mut self) -> Result<(), Err> {
        let (crc_calc, len_calc) = (self.crc, self.total_in);
        // El trailer de 8 bytes está SIN COMPRIMIR en el fichero, justo detrás
        // del último byte consumido por el inflater.
        self.pos = self.deflate_end_offset();      // total_in del inflater sobre `raw`
        self.st = Stream::Plain;
        let t = self.read(8)?;
        let crc_file = u32::from_le_bytes(t[0..4].try_into().unwrap());
        let len_file = u32::from_le_bytes(t[4..8].try_into().unwrap());
        if crc_file != crc_calc || len_file != len_calc { warn!("bloque comprimido corrupto"); }
        Ok(())
    }
}
```

**Nota de implementación importante:** hay que saber *cuántos bytes del fichero físico ha
consumido realmente el inflater* para posicionarse en el trailer. Con `flate2` se usa
`Decompress::total_in()`; con la API de alto nivel se puede leer el
`unused_data`/`into_inner()`. El código original hace exactamente el mismo ajuste
(`Kernel/zstream.cpp:1312-1318`: retrocede el puntero `n - 8` bytes).

### 3.6. Records "streamed" (bitmaps)

`Kernel/cxfile.cpp:1437-1465` (`StartStreamedRecord`) y `Kernel/cxfile.cpp:1593-1612`
(`FixStreamedRecordHeader`):

* Se apaga la compresión (emitiendo el `TAG_ENDCOMPRESSION` correspondiente).
* Se escribe la cabecera `tag/size` **sin comprimir**, con `size` provisional.
* Se vuelca el contenido (fichero PNG/JPEG/GIF/BMP/WAV completo) **sin comprimir**.
* Se reposiciona el puntero de fichero y se parchea el campo `size`
  (`Size = Pos - StartOfStreamedRecord - 8`).
* Se reactiva la compresión (nuevo `TAG_STARTCOMPRESSION`).

Para el lector no hay nada especial: leer `size` bytes.

---

## 4. Tabla completa de tags y layouts

### 4.0. Cómo leer la tabla

La columna **“tam.”** indica el tamaño del payload según `Kernel/cxfdefs.h`
(`var` = variable, `—` = no declarado en `cxfdefs.h`). La columna **“corpus”** indica
cuántas veces aparece el tag en los 59 ficheros `.xar` analizados (0 = **no observado**;
útil para priorizar). Todos los tamaños fijos declarados han sido **verificados contra los
tamaños reales observados** y coinciden (§12.2).

En total hay **300 tags definidos** en `Kernel/cxftags.h`; **157 distintos** aparecen en el
corpus.

### 4.1. Tabla maestra (300 tags)

| tag | nombre simbólico | tam. | corpus | descripción |
|---:|---|---|---:|---|
| 0 | `TAG_UP` | 0 | 201121 | Sube un nivel en el árbol (cierra el grupo de hijos abierto por TAG_DOWN) |
| 1 | `TAG_DOWN` | 0 | 201121 | Baja un nivel: los records siguientes son hijos del último nodo insertado |
| 2 | `TAG_FILEHEADER` | var | 59 | Cabecera lógica: tipo de fichero, tamaño sin comprimir, productor y versión |
| 3 | `TAG_ENDOFFILE` | 0 | 59 | Fin del fichero; el parser debe detenerse aquí |
| 10 | `TAG_ATOMICTAGS` | var | 739 | Lista de tags «atómicos»: si no se reconocen, se descarta todo su subárbol |
| 11 | `TAG_ESSENTIALTAGS` | var | 0 | Lista de tags «esenciales»: si no se reconocen, hay que abortar la importación |
| 12 | `TAG_TAGDESCRIPTION` | var | 0 | Descripciones textuales de tags, para mensajes de aviso al usuario |
| 20 | `TAG_NONRENDERSECTION_START` | 0 | 0 | Inicio de sección no renderizable (reservado, sin implementación) |
| 21 | `TAG_NONRENDERSECTION_END` | 0 | 0 | Fin de sección no renderizable (reservado, sin implementación) |
| 22 | `TAG_RENDERING_PAUSE` | 0 | 0 | Pausa de render (reservado, sin implementación) |
| 23 | `TAG_RENDERING_RESUME` | 0 | 0 | Reanudar render (reservado, sin implementación) |
| 30 | `TAG_STARTCOMPRESSION` | 4 | 103 | A partir del byte siguiente el flujo está comprimido (deflate crudo) |
| 31 | `TAG_ENDCOMPRESSION` | 8 | 103 | Fin del bloque comprimido; sus 8 bytes de datos son CRC32+tamaño sin comprimir |
| 40 | `TAG_DOCUMENT` | 0 | 59 | Nodo raíz del documento |
| 41 | `TAG_CHAPTER` | 0 | 59 | Nodo capítulo (contenedor de spreads) |
| 42 | `TAG_SPREAD` | 0 | 59 | Nodo spread (hoja/pliego) |
| 43 | `TAG_LAYER` | 0 | 115 | Nodo capa |
| 44 | `TAG_PAGE` | 16 | 0 | Nodo página (legacy; la geometría va en TAG_SPREADINFORMATION) |
| 45 | `TAG_SPREADINFORMATION` | 17 | 59 | Tamaño de página, márgenes, sangrado y flags del spread |
| 46 | `TAG_GRIDRULERSETTINGS` | 17 | 59 | Configuración de rejilla y reglas del spread |
| 47 | `TAG_GRIDRULERORIGIN` | 8 | 59 | Origen de la rejilla (DocCoord) |
| 48 | `TAG_LAYERDETAILS` | var | 115 | Flags y nombre de la capa |
| 49 | `TAG_GUIDELAYERDETAILS` | var | 0 | Como LAYERDETAILS más la referencia de color de las guías |
| 50 | `TAG_DEFINERGBCOLOUR` | 3 | 0 | Definición de color RGB simple (3 bytes); referenciable por número de record |
| 51 | `TAG_DEFINECOMPLEXCOLOUR` | 29+nombre | 5766 | Definición de color indexado/nombrado completo; referenciable |
| 52 | `TAG_SPREADSCALING_ACTIVE` | 24 | 1 | Escala de dibujo del spread (activa) |
| 53 | `TAG_SPREADSCALING_INACTIVE` | 24 | 58 | Escala de dibujo del spread (inactiva) |
| 60 | `TAG_PREVIEWBITMAP_BMP` | var | 0 | Bitmap de previsualización del documento en BMP (datos crudos, streamed) |
| 61 | `TAG_PREVIEWBITMAP_GIF` | var | 58 | Bitmap de previsualización en GIF (datos crudos, streamed) |
| 62 | `TAG_PREVIEWBITMAP_JPEG` | var | 0 | Bitmap de previsualización en JPEG (streamed) |
| 63 | `TAG_PREVIEWBITMAP_PNG` | var | 0 | Bitmap de previsualización en PNG (streamed) |
| 64 | `TAG_PREVIEWBITMAP_TIFFLZW` | var | 0 | Bitmap de previsualización en TIFF-LZW (streamed) |
| 65 | `TAG_DEFINEBITMAP_BMP` | var | 0 | Definición de bitmap: nombre + fichero BMP embebido (streamed, referenciable) |
| 66 | `TAG_DEFINEBITMAP_GIF` | var | 0 | Definición de bitmap: nombre + fichero GIF embebido (streamed, referenciable) |
| 67 | `TAG_DEFINEBITMAP_JPEG` | var | 4 | Definición de bitmap: nombre + JPEG embebido (streamed, referenciable) |
| 68 | `TAG_DEFINEBITMAP_PNG` | var | 32 | Definición de bitmap: nombre + PNG embebido (streamed, referenciable) |
| 69 | `TAG_DEFINEBITMAP_BMPZIP` | var | 0 | Definición de bitmap BMP comprimido (streamed) |
| 70 | `TAG_DEFINESOUND_WAV` | var | 0 | Definición de sonido WAV embebido (streamed) |
| 71 | `TAG_DEFINEBITMAP_JPEG8BPP` | var | 8 | JPEG de 24bpp + paleta para reconstruir un bitmap de 8bpp (streamed) |
| 80 | `TAG_VIEWPORT` | 16 | 59 | Rectángulo de la vista del documento |
| 81 | `TAG_VIEWQUALITY` | 1 | 0 | Calidad de render de la vista |
| 82 | `TAG_DOCUMENTVIEW` | 24 | 59 | Escala, área visible y flags de la vista guardada |
| 85 | `TAG_DEFINE_PREFIXUSERUNIT` | 28+strings | 0 | Definición de unidad de usuario con prefijo; referenciable |
| 86 | `TAG_DEFINE_SUFFIXUSERUNIT` | 28+strings | 0 | Definición de unidad de usuario con sufijo; referenciable |
| 87 | `TAG_DEFINE_DEFAULTUNITS` | 8 | 59 | Unidades por defecto de página y de fuente (referencias a unidades) |
| 90 | `TAG_DOCUMENTCOMMENT` | var | 0 | Comentario del documento (string Unicode) |
| 91 | `TAG_DOCUMENTDATES` | 8 | 59 | Fechas de creación y último guardado |
| 92 | `TAG_DOCUMENTUNDOSIZE` | 4 | 59 | Tamaño del búfer de deshacer |
| 93 | `TAG_DOCUMENTFLAGS` | 4 | 59 | Flags del documento (bit0 = todas las capas visibles, bit1 = multicapa) |
| 95 | `TAG_NAMEGAL_DOCCOMP` | — | 0 | Componente de documento de la galería de nombres (sin implementación) |
| 100 | `TAG_PATH` | var | 0 | Camino absoluto, ni relleno ni trazado |
| 101 | `TAG_PATH_FILLED` | var | 0 | Camino absoluto, relleno |
| 102 | `TAG_PATH_STROKED` | var | 0 | Camino absoluto, trazado |
| 103 | `TAG_PATH_FILLED_STROKED` | var | 0 | Camino absoluto, relleno y trazado |
| 104 | `TAG_GROUP` | — | 19075 | Nodo grupo (sin payload); los hijos van entre DOWN/UP |
| 105 | `TAG_BLEND` | 3 | 480 | Nodo mezcla (blend): número de pasos y flags |
| 106 | `TAG_BLENDER` | 8 | 508 | Nodo «blender» que empareja caminos de origen/destino |
| 107 | `TAG_MOULD_ENVELOPE` | 4 | 68 | Molde de tipo envolvente (umbral) |
| 108 | `TAG_MOULD_PERSPECTIVE` | 4 | 129 | Molde de tipo perspectiva (umbral) |
| 109 | `TAG_MOULD_GROUP` | 0 | 197 | Grupo moldeado (contenedor) |
| 110 | `TAG_MOULD_PATH` | var | 197 | Camino que define la malla del molde |
| 112 | `TAG_GUIDELINE` | 5 | 0 | Línea guía (tipo + ordenada) |
| 113 | `TAG_PATH_RELATIVE` | var | 0 | Camino en formato relativo/intercalado, ni relleno ni trazado |
| 114 | `TAG_PATH_RELATIVE_FILLED` | var | 0 | Camino relativo, relleno |
| 115 | `TAG_PATH_RELATIVE_STROKED` | var | 16888 | Camino relativo, trazado |
| 116 | `TAG_PATH_RELATIVE_FILLED_STROKED` | var | 135227 | Camino relativo, relleno y trazado |
| 118 | `TAG_PATHREF_TRANSFORM` | 28 | 0 | Camino idéntico a otro record de camino, aplicando una matriz |
| 150 | `TAG_FLATFILL` | 4 | 69066 | Relleno plano con referencia a color |
| 151 | `TAG_LINECOLOUR` | 4 | 63297 | Color de línea con referencia a color |
| 152 | `TAG_LINEWIDTH` | 4 | 138164 | Grosor de línea en millipoints |
| 153 | `TAG_LINEARFILL` | 40 | 55864 | Relleno degradado lineal (2 colores + perfil) |
| 154 | `TAG_CIRCULARFILL` | 40 | 108 | Relleno degradado circular |
| 155 | `TAG_ELLIPTICALFILL` | 48 | 29161 | Relleno degradado elíptico |
| 156 | `TAG_CONICALFILL` | 40 | 409 | Relleno degradado cónico |
| 157 | `TAG_BITMAPFILL` | 44 | 29 | Relleno de bitmap |
| 158 | `TAG_CONTONEBITMAPFILL` | 52 | 7 | Relleno de bitmap contoneado (2 colores) |
| 159 | `TAG_FRACTALFILL` | 69 | 78 | Relleno fractal (plasma) |
| 160 | `TAG_FILLEFFECT_FADE` | 0 | 107 | Efecto de interpolación de color del relleno: fundido |
| 161 | `TAG_FILLEFFECT_RAINBOW` | 0 | 10 | Efecto de interpolación: arcoíris (HSV sentido corto) |
| 162 | `TAG_FILLEFFECT_ALTRAINBOW` | 0 | 8 | Efecto de interpolación: arcoíris alternativo |
| 163 | `TAG_FILL_REPEATING` | 0 | 107 | Mapeo del relleno: repetir |
| 164 | `TAG_FILL_NONREPEATING` | 0 | 155 | Mapeo del relleno: no repetir |
| 165 | `TAG_FILL_REPEATINGINVERTED` | 0 | 172 | Mapeo del relleno: repetir invertido |
| 166 | `TAG_FLATTRANSPARENTFILL` | 2 | 22949 | Transparencia plana (nivel + tipo) |
| 167 | `TAG_LINEARTRANSPARENTFILL` | 35 | 11797 | Transparencia degradada lineal |
| 168 | `TAG_CIRCULARTRANSPARENTFILL` | 35 | 45 | Transparencia degradada circular |
| 169 | `TAG_ELLIPTICALTRANSPARENTFILL` | 43 | 5279 | Transparencia degradada elíptica |
| 170 | `TAG_CONICALTRANSPARENTFILL` | 35 | 5 | Transparencia degradada cónica |
| 171 | `TAG_BITMAPTRANSPARENTFILL` | 47 | 3 | Transparencia por bitmap |
| 172 | `TAG_FRACTALTRANSPARENTFILL` | 64 | 4 | Transparencia fractal |
| 173 | `TAG_LINETRANSPARENCY` | 2 | 24245 | Transparencia de la línea (nivel + tipo) |
| 174 | `TAG_STARTCAP` | 1 | 26258 | Extremo inicial de línea (butt/round/square) |
| 175 | `TAG_ENDCAP` | 1 | 26258 | Extremo final de línea |
| 176 | `TAG_JOINSTYLE` | 1 | 65951 | Tipo de unión de segmentos (mitre/round/bevel) |
| 177 | `TAG_MITRELIMIT` | 4 | 0 | Límite de mitra |
| 178 | `TAG_WINDINGRULE` | 1 | 3 | Regla de relleno (nonzero/negative/evenodd/positive) |
| 179 | `TAG_QUALITY` | 4 | 0 | Calidad de render del objeto |
| 180 | `TAG_TRANSPARENTFILL_REPEATING` | 0 | 107 | Mapeo de la transparencia: repetir |
| 181 | `TAG_TRANSPARENTFILL_NONREPEATING` | 0 | 5 | Mapeo de la transparencia: no repetir |
| 182 | `TAG_TRANSPARENTFILL_REPEATINGINVERTED` | 0 | 19 | Mapeo de la transparencia: repetir invertido |
| 183 | `TAG_DASHSTYLE` | 4 | 15 | Patrón de guiones por referencia (negativa = patrón predefinido) |
| 184 | `TAG_DEFINEDASH` | var | 0 | Patrón de guiones explícito (inicio, ancho, nº elementos, array) |
| 185 | `TAG_ARROWHEAD` | 12 | 12 | Punta de flecha inicial: referencia + escala ancho/alto |
| 186 | `TAG_ARROWTAIL` | 12 | 50 | Punta de flecha final: referencia + escala ancho/alto |
| 187 | `TAG_DEFINEARROW` | — | 0 | Definición de flecha personalizada (declarado, sin implementación) |
| 188 | `TAG_DEFINEDASH_SCALED` | var | 0 | Patrón de guiones explícito que escala con el grosor de línea |
| 189 | `TAG_USERVALUE` | var | 0 | Atributo de usuario clave/valor (Unicode) |
| 190 | `TAG_FLATFILL_NONE` | 0 | 0 | Relleno plano «ninguno» (sin payload) |
| 191 | `TAG_FLATFILL_BLACK` | 0 | 0 | Relleno plano negro (sin payload) |
| 192 | `TAG_FLATFILL_WHITE` | 0 | 0 | Relleno plano blanco (sin payload) |
| 193 | `TAG_LINECOLOUR_NONE` | 0 | 74003 | Color de línea «ninguno» (sin payload) |
| 194 | `TAG_LINECOLOUR_BLACK` | 0 | 0 | Color de línea negro (sin payload) |
| 195 | `TAG_LINECOLOUR_WHITE` | 0 | 0 | Color de línea blanco (sin payload) |
| 198 | `TAG_NODE_BITMAP` | 36 | 25 | Objeto bitmap: paralelogramo de 4 puntos + referencia a bitmap |
| 199 | `TAG_NODE_CONTONEDBITMAP` | 44 | 0 | Objeto bitmap contoneado: 4 puntos + bitmap + 2 colores |
| 200 | `TAG_SQUAREFILL` | 48 | 19 | Relleno degradado cuadrado |
| 201 | `TAG_SQUARETRANSPARENTFILL` | 43 | 32 | Transparencia degradada cuadrada |
| 202 | `TAG_THREECOLFILL` | 36 | 8 | Relleno degradado de 3 colores |
| 203 | `TAG_THREECOLTRANSPARENTFILL` | 28 | 2 | Transparencia de 3 niveles |
| 204 | `TAG_FOURCOLFILL` | 40 | 13 | Relleno degradado de 4 colores |
| 205 | `TAG_FOURCOLTRANSPARENTFILL` | 29 | 2 | Transparencia de 4 niveles |
| 206 | `TAG_FILL_REPEATING_EXTRA` | 0 | 10 | Mapeo del relleno: repetición «extra» |
| 207 | `TAG_TRANSPARENTFILL_REPEATING_EXTRA` | 0 | 0 | Mapeo de la transparencia: repetición «extra» |
| 1000 | `TAG_ELLIPSE_SIMPLE` | 16 | 0 | Elipse simple (centro, ancho, alto) |
| 1001 | `TAG_ELLIPSE_COMPLEX` | 24 | 0 | Elipse compleja (centro, eje mayor, eje menor) |
| 1100 | `TAG_RECTANGLE_SIMPLE` | 16 | 0 | Rectángulo simple (centro/ancho/alto) |
| 1101 | `TAG_RECTANGLE_SIMPLE_REFORMED` | var | 0 | Rectángulo simple (centro/ancho/alto), con caminos de arista editados |
| 1102 | `TAG_RECTANGLE_SIMPLE_STELLATED` | 32 | 0 | Rectángulo simple (centro/ancho/alto), estrellado |
| 1103 | `TAG_RECTANGLE_SIMPLE_STELLATED_REFORMED` | var | 0 | Rectángulo simple (centro/ancho/alto), estrellado, con caminos de arista editados |
| 1104 | `TAG_RECTANGLE_SIMPLE_ROUNDED` | 24 | 0 | Rectángulo simple (centro/ancho/alto), esquinas redondeadas |
| 1105 | `TAG_RECTANGLE_SIMPLE_ROUNDED_REFORMED` | var | 0 | Rectángulo simple (centro/ancho/alto), esquinas redondeadas, con caminos de arista editados |
| 1106 | `TAG_RECTANGLE_SIMPLE_ROUNDED_STELLATED` | 48 | 0 | Rectángulo simple (centro/ancho/alto), esquinas redondeadas, estrellado |
| 1107 | `TAG_RECTANGLE_SIMPLE_ROUNDED_STELLATED_REFORMED` | var | 0 | Rectángulo simple (centro/ancho/alto), esquinas redondeadas, estrellado, con caminos de arista editados |
| 1108 | `TAG_RECTANGLE_COMPLEX` | 24 | 0 | Rectángulo complejo (centro/ejes) |
| 1109 | `TAG_RECTANGLE_COMPLEX_REFORMED` | var | 0 | Rectángulo complejo (centro/ejes), con caminos de arista editados |
| 1110 | `TAG_RECTANGLE_COMPLEX_STELLATED` | 40 | 0 | Rectángulo complejo (centro/ejes), estrellado |
| 1111 | `TAG_RECTANGLE_COMPLEX_STELLATED_REFORMED` | var | 0 | Rectángulo complejo (centro/ejes), estrellado, con caminos de arista editados |
| 1112 | `TAG_RECTANGLE_COMPLEX_ROUNDED` | 32 | 0 | Rectángulo complejo (centro/ejes), esquinas redondeadas |
| 1113 | `TAG_RECTANGLE_COMPLEX_ROUNDED_REFORMED` | var | 0 | Rectángulo complejo (centro/ejes), esquinas redondeadas, con caminos de arista editados |
| 1114 | `TAG_RECTANGLE_COMPLEX_ROUNDED_STELLATED` | 56 | 0 | Rectángulo complejo (centro/ejes), esquinas redondeadas, estrellado |
| 1115 | `TAG_RECTANGLE_COMPLEX_ROUNDED_STELLATED_REFORMED` | var | 0 | Rectángulo complejo (centro/ejes), esquinas redondeadas, estrellado, con caminos de arista editados |
| 1200 | `TAG_POLYGON_COMPLEX` | 26 | 0 | Polígono complejo (nº lados, centro, ejes) |
| 1201 | `TAG_POLYGON_COMPLEX_REFORMED` | var | 0 | Polígono complejo (centro/ejes), con caminos de arista editados |
| 1212 | `TAG_POLYGON_COMPLEX_STELLATED` | 42 | 0 | Polígono complejo (centro/ejes), estrellado |
| 1213 | `TAG_POLYGON_COMPLEX_STELLATED_REFORMED` | var | 0 | Polígono complejo (centro/ejes), estrellado, con caminos de arista editados |
| 1214 | `TAG_POLYGON_COMPLEX_ROUNDED` | 34 | 0 | Polígono complejo (centro/ejes), esquinas redondeadas |
| 1215 | `TAG_POLYGON_COMPLEX_ROUNDED_REFORMED` | var | 0 | Polígono complejo (centro/ejes), esquinas redondeadas, con caminos de arista editados |
| 1216 | `TAG_POLYGON_COMPLEX_ROUNDED_STELLATED` | 58 | 0 | Polígono complejo (centro/ejes), esquinas redondeadas, estrellado |
| 1217 | `TAG_POLYGON_COMPLEX_ROUNDED_STELLATED_REFORMED` | var | 0 | Polígono complejo (centro/ejes), esquinas redondeadas, estrellado, con caminos de arista editados |
| 1900 | `TAG_REGULAR_SHAPE_PHASE_1` | var | 0 | Forma regular genérica v1 (incluye centro UT; legacy) |
| 1901 | `TAG_REGULAR_SHAPE_PHASE_2` | var | 35206 | Forma regular genérica v2: flags, lados, ejes, matriz, estrellado, redondeo y caminos |
| 2000 | `TAG_FONT_DEF_TRUETYPE` | var | 61 | Definición de fuente TrueType: nombre completo, tipo de letra y PANOSE; referenciable |
| 2001 | `TAG_FONT_DEF_ATM` | var | 0 | Definición de fuente ATM/Type1; referenciable |
| 2100 | `TAG_TEXT_STORY_SIMPLE` | 12 | 140 | Historia de texto con posición simple (DocCoord + autokern) |
| 2101 | `TAG_TEXT_STORY_COMPLEX` | 28 | 25 | Historia de texto con matriz completa (+ autokern) |
| 2110 | `TAG_TEXT_STORY_SIMPLE_START_LEFT` | 12 | 2 | Historia de texto sobre camino (simple start left) |
| 2111 | `TAG_TEXT_STORY_SIMPLE_START_RIGHT` | 12 | 0 | Historia de texto sobre camino (simple start right) |
| 2112 | `TAG_TEXT_STORY_SIMPLE_END_LEFT` | 12 | 0 | Historia de texto sobre camino (simple end left) |
| 2113 | `TAG_TEXT_STORY_SIMPLE_END_RIGHT` | 12 | 0 | Historia de texto sobre camino (simple end right) |
| 2114 | `TAG_TEXT_STORY_COMPLEX_START_LEFT` | 36 | 3 | Historia de texto sobre camino (complex start left) |
| 2115 | `TAG_TEXT_STORY_COMPLEX_START_RIGHT` | 36 | 0 | Historia de texto sobre camino (complex start right) |
| 2116 | `TAG_TEXT_STORY_COMPLEX_END_LEFT` | 36 | 0 | Historia de texto sobre camino (complex end left) |
| 2117 | `TAG_TEXT_STORY_COMPLEX_END_RIGHT` | 36 | 2 | Historia de texto sobre camino (complex end right) |
| 2150 | `TAG_TEXT_STORY_WORD_WRAP_INFO` | 5 | 172 | Anchura de columna y flag de ajuste de línea |
| 2151 | `TAG_TEXT_STORY_INDENT_INFO` | 8 | 172 | Sangrías izquierda y derecha de la historia |
| 2200 | `TAG_TEXT_LINE` | 0 | 328 | Línea de texto (contenedor, sin payload) |
| 2201 | `TAG_TEXT_STRING` | var | 381 | Cadena de texto (UTF-16LE, sin terminador) |
| 2202 | `TAG_TEXT_CHAR` | 2 | 67 | Un carácter de texto (UTF-16) |
| 2203 | `TAG_TEXT_EOL` | 0 | 256 | Fin de línea/párrafo |
| 2204 | `TAG_TEXT_KERN` | 8 | 56 | Kern manual (desplazamiento x,y) |
| 2205 | `TAG_TEXT_CARET` | 0 | 0 | Posición del cursor de texto |
| 2206 | `TAG_TEXT_LINE_INFO` | 12 | 328 | Métricas de la línea: ancho, alto y distancia a la anterior |
| 2900 | `TAG_TEXT_LINESPACE_RATIO` | 4 | 93 | Interlineado proporcional (FIXED16) |
| 2901 | `TAG_TEXT_LINESPACE_ABSOLUTE` | 4 | 1 | Interlineado absoluto (millipoints) |
| 2902 | `TAG_TEXT_JUSTIFICATION_LEFT` | 0 | 0 | Justificación izquierda |
| 2903 | `TAG_TEXT_JUSTIFICATION_CENTRE` | 0 | 30 | Justificación centrada |
| 2904 | `TAG_TEXT_JUSTIFICATION_RIGHT` | 0 | 5 | Justificación derecha |
| 2905 | `TAG_TEXT_JUSTIFICATION_FULL` | 0 | 3 | Justificación completa |
| 2907 | `TAG_TEXT_FONT_TYPEFACE` | 4 | 249 | Referencia al record de definición de fuente |
| 2908 | `TAG_TEXT_BOLD_ON` | 0 | 61 | Negrita activada |
| 2909 | `TAG_TEXT_BOLD_OFF` | 0 | 0 | Negrita desactivada |
| 2910 | `TAG_TEXT_ITALIC_ON` | 0 | 35 | Cursiva activada |
| 2911 | `TAG_TEXT_ITALIC_OFF` | 0 | 0 | Cursiva desactivada |
| 2912 | `TAG_TEXT_UNDERLINE_ON` | 0 | 0 | Subrayado activado |
| 2913 | `TAG_TEXT_UNDERLINE_OFF` | 0 | 0 | Subrayado desactivado |
| 2914 | `TAG_TEXT_SCRIPT_ON` | 8 | 0 | Super/subíndice con desplazamiento y tamaño explícitos |
| 2915 | `TAG_TEXT_SCRIPT_OFF` | 0 | 0 | Fin de super/subíndice |
| 2916 | `TAG_TEXT_SUPERSCRIPT_ON` | 0 | 6 | Superíndice con valores implícitos |
| 2917 | `TAG_TEXT_SUBSCRIPT_ON` | 0 | 6 | Subíndice con valores implícitos |
| 2918 | `TAG_TEXT_TRACKING` | 4 | 29 | Tracking (espaciado entre caracteres) |
| 2919 | `TAG_TEXT_ASPECT_RATIO` | 4 | 77 | Relación de aspecto horizontal del texto (FIXED16) |
| 2920 | `TAG_TEXT_BASELINE` | 4 | 101 | Desplazamiento de línea base (millipoints) |
| 3500 | `TAG_OVERPRINTLINEON` | 0 | 0 | Sobreimpresión de línea activada |
| 3501 | `TAG_OVERPRINTLINEOFF` | 0 | 0 | Sobreimpresión de línea desactivada |
| 3502 | `TAG_OVERPRINTFILLON` | 0 | 0 | Sobreimpresión de relleno activada |
| 3503 | `TAG_OVERPRINTFILLOFF` | 0 | 0 | Sobreimpresión de relleno desactivada |
| 3504 | `TAG_PRINTONALLPLATESON` | 0 | 0 | Imprimir en todas las planchas |
| 3505 | `TAG_PRINTONALLPLATESOFF` | 0 | 0 | No imprimir en todas las planchas |
| 3506 | `TAG_PRINTERSETTINGS` | 45 | 58 | Configuración de impresión del documento |
| 3507 | `TAG_IMAGESETTING` | 15 | 58 | Ajustes de filmación (resolución, trama, función) |
| 3508 | `TAG_COLOURPLATE` | 22 | 0 | Definición de plancha de color (tipo, color spot, ángulo, frecuencia) |
| 3509 | `TAG_PRINTMARKDEFAULT` | 1 | 228 | Marca de impresión predefinida (1 byte de identificador) |
| 3510 | `TAG_PRINTMARKCUSTOM` | var | 0 | Marca de impresión personalizada (subárbol) |
| 4000 | `TAG_VARIABLEWIDTHFUNC` | 4 | 0 | Función de anchura variable del trazo (id predefinido) |
| 4001 | `TAG_VARIABLEWIDTHTABLE` | var | 0 | Tabla de anchuras variables del trazo |
| 4002 | `TAG_STROKETYPE` | 4 | 0 | Tipo de trazo (handle a definición de trazo) |
| 4003 | `TAG_STROKEDEFINITION` | var | 0 | Definición de trazo: handle, flags, repeticiones |
| 4004 | `TAG_STROKEAIRBRUSH` | var | 0 | Datos de aerógrafo del trazo |
| 4010 | `TAG_NOISEFILL` | 61 | 112 | Relleno de ruido fractal |
| 4011 | `TAG_NOISETRANSPARENTFILL` | 56 | 2 | Transparencia de ruido fractal |
| 4012 | `TAG_MOULD_BOUNDS` | 16 | 197 | Rectángulo del objeto original moldeado |
| 4013 | `TAG_PATHREF_IDENTICAL` | — | 0 | Camino idéntico a otro record de camino (solo referencia) |
| 4014 | `TAG_PATHREF_TRANSLATE` | — | 0 | Camino igual a otro más una traslación (definido, sin implementación) |
| 4015 | `TAG_EXPORT_HINT` | — | 6 | Pista de exportación (tipo, tamaño, bpp, opciones) |
| 4020 | `TAG_WEBADDRESS` | var | 0 | Atributo de hiperenlace (URL + frame) |
| 4021 | `TAG_WEBADDRESS_BOUNDINGBOX` | var | 0 | Hiperenlace con caja delimitadora |
| 4030 | `TAG_LAYER_FRAMEPROPS` | 5 | 1 | Propiedades de frame de animación de la capa (retardo + flags) |
| 4031 | `TAG_SPREAD_ANIMPROPS` | 28 | 59 | Propiedades de animación del spread (bucle, retardo, paleta…) |
| 4040 | `TAG_WIZOP` | var | 6 | Propiedades de plantilla/WizOp (strings Unicode) |
| 4041 | `TAG_WIZOP_STYLE` | — | 0 | Definición de estilo de plantilla |
| 4042 | `TAG_WIZOP_STYLEREF` | 4 | 0 | Referencia a un estilo de plantilla |
| 4050 | `TAG_SHADOWCONTROLLER` | 29 | 98 | Controlador de sombra: tipo, penumbra, desplazamiento, ángulo, escala… |
| 4051 | `TAG_SHADOW` | 24 | 98 | Nodo sombra: bias/gain del perfil y oscuridad |
| 4052 | `TAG_BEVEL` | 24 | 22 | Controlador de bisel: tipo, indent, ángulo de luz, exterior, contraste, inclinación |
| 4053 | `TAG_BEVATTR_INDENT` | 4 | 0 | Atributo de bisel: indentación |
| 4054 | `TAG_BEVATTR_LIGHTANGLE` | 8 | 0 | Atributo de bisel: ángulo de luz |
| 4055 | `TAG_BEVATTR_CONTRAST` | — | 0 | Atributo de bisel: contraste |
| 4056 | `TAG_BEVATTR_TYPE` | 4 | 0 | Atributo de bisel: tipo |
| 4057 | `TAG_BEVELINK` | — | 22 | Nodo de tinta del bisel (sin payload) |
| 4060 | `TAG_BLENDER_CURVEPROP` | 16 | 0 | Proporciones de recorrido del blend sobre curva |
| 4061 | `TAG_BLEND_PATH` | var | 0 | Camino sobre el que se mezcla |
| 4062 | `TAG_BLENDER_CURVEANGLES` | 16 | 0 | Ángulos inicial/final del blend sobre curva |
| 4063 | `TAG_GROUPTRANSP` | — | 0 | Grupo con transparencia (declarado, sin implementación) |
| 4064 | `TAG_CACHEBMP` | — | 0 | Bitmap cacheado (declarado, sin implementación) |
| 4065 | `TAG_CACHEDNODESGROUP` | — | 0 | Grupo de nodos cacheados (declarado, sin implementación) |
| 4066 | `TAG_CONTOURCONTROLLER` | 41 | 4 | Controlador de contorno: pasos, ancho, tipo y perfiles |
| 4067 | `TAG_CONTOUR` | 0 | 4 | Nodo contorno generado (sin payload) |
| 4070 | `TAG_SETSENTINEL` | 0 | 59 | Centinela de los «sets» de la galería de nombres |
| 4071 | `TAG_SETPROPERTY` | var | 0 | Propiedad de un set con nombre |
| 4072 | `TAG_BLENDPROFILES` | 48 | 480 | Perfiles bias/gain del blend (objeto, atributo, posición) |
| 4073 | `TAG_BLENDERADDITIONAL` | 17 | 508 | Datos extra del blender (índices, curva, flags) |
| 4074 | `TAG_NODEBLENDPATH_FILLED` | 4 | 0 | Indica si el camino del blend está relleno |
| 4075 | `TAG_LINEARFILLMULTISTAGE` | var | 60 | Degradado lineal multietapa (rampa de colores) |
| 4076 | `TAG_CIRCULARFILLMULTISTAGE` | var | 4 | Degradado circular multietapa |
| 4077 | `TAG_ELLIPTICALFILLMULTISTAGE` | var | 2 | Degradado elíptico multietapa |
| 4078 | `TAG_CONICALFILLMULTISTAGE` | var | 6 | Degradado cónico multietapa |
| 4079 | `TAG_BRUSHATTR` | 33 | 2 | Atributo de pincel aplicado a un objeto |
| 4080 | `TAG_BRUSHDEFINITION` | 4 | 2 | Definición de pincel (handle) |
| 4081 | `TAG_BRUSHDATA` | var | 2 | Datos del pincel |
| 4082 | `TAG_MOREBRUSHDATA` | 68 | 2 | Datos adicionales del pincel |
| 4083 | `TAG_MOREBRUSHATTR` | 72 | 2 | Atributos adicionales del pincel |
| 4084 | `TAG_CLIPVIEWCONTROLLER` | 0 | 0 | Controlador de ClipView (recorte) |
| 4085 | `TAG_CLIPVIEW` | 0 | 0 | Nodo ClipView |
| 4086 | `TAG_FEATHER` | 20 | 75 | Atributo de desvanecido (feather): tamaño y perfil |
| 4087 | `TAG_BARPROPERTY` | var | 59 | Propiedades de barra de navegación |
| 4088 | `TAG_SQUAREFILLMULTISTAGE` | var | 2 | Degradado cuadrado multietapa |
| 4102 | `TAG_EVENMOREBRUSHDATA` | 8 | 2 | Más datos del pincel |
| 4103 | `TAG_EVENMOREBRUSHATTR` | 9 | 2 | Más atributos del pincel |
| 4104 | `TAG_TIMESTAMPBRUSHDATA` | var | 0 | Marcas de tiempo del pincel |
| 4105 | `TAG_BRUSHPRESSUREINFO` | 28 | 2 | Información de presión del pincel |
| 4106 | `TAG_BRUSHPRESSUREDATA` | var | 0 | Datos de presión del pincel |
| 4107 | `TAG_BRUSHATTRPRESSUREINFO` | 28 | 2 | Información de presión del atributo pincel |
| 4108 | `TAG_BRUSHCOLOURDATA` | — | 0 | Datos de color del pincel (declarado, sin implementación) |
| 4109 | `TAG_BRUSHPRESSURESAMPLEDATA` | var | 0 | Muestras de presión del pincel |
| 4110 | `TAG_BRUSHTIMESAMPLEDATA` | — | 0 | Muestras temporales del pincel (declarado, sin implementación) |
| 4111 | `TAG_BRUSHATTRFILLFLAGS` | 1 | 2 | Flags de relleno del atributo pincel |
| 4112 | `TAG_BRUSHTRANSPINFO` | 40 | 2 | Información de transparencia del pincel |
| 4113 | `TAG_BRUSHATTRTRANSPINFO` | 40 | 2 | Información de transparencia del atributo pincel |
| 4114 | `TAG_DOCUMENTNUDGE` | 4 | 59 | Tamaño del desplazamiento por teclado (millipoints) |
| 4115 | `TAG_BITMAP_PROPERTIES` | 12 | 44 | Propiedades de un bitmap (referencia + flags) |
| 4116 | `TAG_DOCUMENTBITMAPSMOOTHING` | 5 | 59 | Suavizado de bitmaps del documento |
| 4117 | `TAG_XPE_BITMAP_PROPERTIES` | var | 0 | Propiedades extendidas XPE de un bitmap |
| 4118 | `TAG_DEFINEBITMAP_XPE` | 0 | 0 | Marcador de bitmap generado por XPE |
| 4119 | `TAG_CURRENTATTRIBUTES` | 1 | 106 | Contenedor de atributos actuales del documento (grupo 1=tinta, 2=texto) |
| 4120 | `TAG_CURRENTATTRIBUTEBOUNDS` | 16 | 298 | Caja delimitadora asociada a los atributos actuales |
| 4121 | `TAG_LINEARFILL3POINT` | 48 | 17 | Degradado lineal de 3 puntos (no perpendicular) |
| 4122 | `TAG_LINEARFILLMULTISTAGE3POINT` | var | 0 | Degradado lineal multietapa de 3 puntos |
| 4123 | `TAG_LINEARTRANSPARENTFILL3POINT` | 43 | 10 | Transparencia lineal de 3 puntos |
| 4124 | `TAG_DUPLICATIONOFFSET` | 8 | 53 | Desplazamiento de duplicado del documento |
| 4125 | `TAG_LIVE_EFFECT` | var | 0 | Nodo de efecto vivo (ID de efecto + XML de edición) |
| 4126 | `TAG_LOCKED_EFFECT` | var | 0 | Efecto bloqueado (rasterizado) |
| 4127 | `TAG_FEATHER_EFFECT` | var | 0 | Efecto de desvanecido como nodo |
| 4128 | `TAG_COMPOUNDRENDER` | 20 | 7 | Aviso de render compuesto + caja delimitadora |
| 4129 | `TAG_OBJECTBOUNDS` | 16 | 0 | Caja delimitadora del objeto (2 DocCoord) |
| 4130 | `TAG_DIMENSION` | — | 0 | Cota/dimensión (declarado, sin implementación) |
| 4131 | `TAG_SPREAD_PHASE2` | — | 0 | Spread v2 (declarado; solo mencionado en comentarios) |
| 4132 | `TAG_CURRENTATTRIBUTES_PHASE2` | — | 0 | Atributos actuales v2 (declarado; solo en comentarios) |
| 4134 | `TAG_SPREAD_FLASHPROPS` | — | 0 | Propiedades Flash del spread (declarado, sin implementación) |
| 4135 | `TAG_PRINTERSETTINGS_PHASE2` | — | 0 | Configuración de impresión v2 (declarado, sin implementación) |
| 4136 | `TAG_DOCUMENTINFORMATION` | — | 0 | Información del documento (declarado, sin implementación) |
| 4137 | `TAG_CLIPVIEW_PATH` | — | 0 | Camino de recorte de ClipView (declarado, sin implementación) |
| 4138 | `TAG_DEFINEBITMAP_PNG_REAL` | — | 0 | PNG real (declarado, sin implementación) |
| 4200 | `TAG_TEXT_TAB` | 0 | 0 | Tabulador horizontal |
| 4201 | `TAG_TEXT_LEFT_INDENT` | 4 | 0 | Sangría izquierda del párrafo |
| 4202 | `TAG_TEXT_FIRST_INDENT` | 4 | 0 | Sangría de primera línea |
| 4203 | `TAG_TEXT_RIGHT_INDENT` | 4 | 0 | Sangría derecha del párrafo |
| 4204 | `TAG_TEXT_RULER` | var | 0 | Regla de tabulaciones del párrafo |
| 4205 | `TAG_TEXT_STORY_HEIGHT_INFO` | — | 0 | Altura de la historia de texto (declarado, sin implementación) |
| 4206 | `TAG_TEXT_STORY_LINK_INFO` | — | 0 | Enlace entre áreas de texto (declarado, sin implementación) |
| 4207 | `TAG_TEXT_STORY_TRANSLATION_INFO` | — | 0 | Traslación de la historia (declarado, sin implementación) |

### 4.2. Categoría: navegación y control del árbol

| tag | record | payload |
|---|---|---|
| 0 | `TAG_UP` | vacío |
| 1 | `TAG_DOWN` | vacío |
| 2 | `TAG_FILEHEADER` | §1.4 |
| 3 | `TAG_ENDOFFILE` | vacío — el parser **debe parar** aquí (`Kernel/cxfile.cpp:2466-2472`) |
| 10 | `TAG_ATOMICTAGS` | `u32[size/4]` — lista de tags atómicos (`Kernel/cxfile.cpp:2562-2585`) |
| 11 | `TAG_ESSENTIALTAGS` | `u32[size/4]` — lista de tags esenciales (`Kernel/cxfile.cpp:2600-2624`) |
| 12 | `TAG_TAGDESCRIPTION` | `u32 num_tags`, luego `num_tags` × (`u32 tag`, `UNICODE-Z descripción`) (`Kernel/cxfile.cpp:2640+`) |
| 30 | `TAG_STARTCOMPRESSION` | §3.2 |
| 31 | `TAG_ENDCOMPRESSION` | §3.4 |

### 4.3. Categoría: documento, spreads, páginas y capas

**`TAG_DOCUMENT` (40), `TAG_CHAPTER` (41), `TAG_SPREAD` (42), `TAG_LAYER` (43)** — payload
vacío; su contenido cuelga entre `TAG_DOWN`/`TAG_UP`.

**`TAG_SPREADINFORMATION` (45), 17 bytes** — `Kernel/rechdoc.cpp:596-625`

| Off | Tipo | Campo |
|---|---|---|
| 0 | `i32` | `width` — ancho de página (millipoints) |
| 4 | `i32` | `height` — alto de página |
| 8 | `i32` | `margin` — margen del pasteboard alrededor de las páginas |
| 12 | `i32` | `bleed` — sangrado (0 = ninguno) |
| 16 | `u8` | `flags` |

`flags`: bit 0 = doble página (`DoublePageSpread`), bit 1 = mostrar sombra de página.
⚠️ **Incoherencia en el código original**: el handler de import usa el bit 0 para DPS
(`Kernel/rechdoc.cpp:617-620`) mientras el código de descripción de depuración usa el
bit 2 (`Kernel/rechdoc.cpp:1332-1336`). Seguir el handler (bit 0) y tratar el bit 2 como
desconocido (§11).

**`TAG_GRIDRULERSETTINGS` (46), 17 bytes** — `Kernel/rechdoc.cpp:1020-1035`

| Off | Tipo | Campo |
|---|---|---|
| 0 | `i32` (REFERENCE) | unidad usada por la rejilla |
| 4 | `f64` | divisiones |
| 12 | `u32` | subdivisiones |
| 16 | `u8` | tipo de rejilla (0 = rectangular, 1 = isométrica) |

**`TAG_GRIDRULERORIGIN` (47), 8 bytes** — `DocCoord origin`.

**`TAG_LAYERDETAILS` (48) / `TAG_GUIDELAYERDETAILS` (49)** — `Kernel/rechdoc.cpp:790-800`

| Off | Tipo | Campo |
|---|---|---|
| 0 | `u8` | `flags` |
| 1 | `UNICODE-Z` | nombre de la capa |
| … | `i32` | (solo en `TAG_GUIDELAYERDETAILS`) referencia al color de las guías |

`flags` (`Kernel/cxfdefs.h:199-205`):

| bit | valor | significado |
|---|---|---|
| 0 | 0x01 | visible |
| 1 | 0x02 | bloqueada |
| 2 | 0x04 | imprimible |
| 3 | 0x08 | activa |
| 4 | 0x10 | fondo de página |
| 5 | 0x20 | capa de fondo |

**`TAG_LAYER_FRAMEPROPS` (4030), 5 bytes** — `u32 delay` (centésimas de s) + `u8 flags`
con `0x01`=solid, `0x02`=overlay, `0x04`=hidden (`Kernel/cxfdefs.h:207-211`,
`Kernel/rechdoc.cpp:1680-1690`).

**`TAG_SPREADSCALING_ACTIVE` (52) / `_INACTIVE` (53), 24 bytes** —
`Kernel/rechdoc.cpp:1165-1172`: `f64 drawing_scale`, `i32 drawing_units` (REFERENCE a
unidad), `f64 real_scale`, `i32 real_units`.

**`TAG_SPREAD_ANIMPROPS` (4031), 28 bytes** — 7 × `u32`:
`loop`, `global_delay`, `dither`, `web_palette`, `colours_palette`, `num_colours`,
`flags` (`Kernel/spread.cpp:3790-3806`, `Kernel/rechdoc.cpp:1600-1625`).

**`TAG_GUIDELINE` (112), 5 bytes** — `u8 type` (`GUIDELINE_HORZ`/`VERT`) + `i32 ordinate`
(leída con `ReadYOrd` si horizontal, `ReadXOrd` si vertical → **se le suma el origen de
coordenadas**, §5.3) (`Kernel/rechdoc.cpp:965-985`).

**`TAG_CURRENTATTRIBUTES` (4119), 1 byte** — `u8 group_id`
(1 = `ATTRIBUTEGROUP_INK`, 2 = `ATTRIBUTEGROUP_TEXT`, `Kernel/cxfdefs.h:600-602`).
Es un **nodo atómico contenedor**: sus hijos (entre DOWN/UP) son los atributos actuales
del documento, *no* objetos del dibujo. Un importador que solo quiera geometría puede
saltarse todo su subárbol.

**`TAG_CURRENTATTRIBUTEBOUNDS` (4120), 16 bytes** — `DocCoord lo`, `DocCoord hi`.

**`TAG_DUPLICATIONOFFSET` (4124), 8 bytes** — `i32 dx`, `i32 dy`.

**`TAG_SETSENTINEL` (4070)**, vacío; **`TAG_SETPROPERTY` (4071)**, variable:
`UNICODE-Z nombre`, `i16 num_props`, luego pares (`i16 tipo`, datos de la propiedad)
(`Kernel/ngsentry.cpp:400-420`, `Kernel/rechdoc.cpp:1750-1800`).

**`TAG_BARPROPERTY` (4087)**, variable: `i32 num_bars`, luego por barra
`i32 spacing`, `u8 code`, `u8 same_size` (`Kernel/rechdoc.cpp:1860-1910`).

### 4.4. Categoría: definiciones de color

**`TAG_DEFINERGBCOLOUR` (50), 3 bytes** — `u8 r`, `u8 g`, `u8 b`
(`Kernel/colcomp.cpp:1604-1610`). **No aparece en ningún fichero del corpus**: Xara
siempre escribe colores complejos (o usa las referencias negativas predefinidas).

**`TAG_DEFINECOMPLEXCOLOUR` (51), 29 bytes + nombre** — ver §9.2.

### 4.5. Categoría: bitmaps y sonido

**`TAG_DEFINEBITMAP_*` (65–71)** — records **streamed**, sin comprimir
(`Kernel/bmpcomp.cpp:2096-2175` escritura, `Kernel/bmpcomp.cpp:1290-1440` lectura):

| Off | Tipo | Campo |
|---|---|---|
| 0 | `UNICODE-Z` | nombre del bitmap (p. ej. `"Default"`) |
| … | *(solo tag 71, JPEG8BPP)* `u8 n_menos_1` + `RGBTRIPLE[n]` | paleta para reconstruir 8 bpp |
| … | bytes | **fichero de imagen completo y tal cual** (PNG con su firma `\x89PNG`, JPEG, GIF, BMP…) |

Mapeo tag → formato (`Kernel/bmpcomp.cpp:1320-1345`):
65 = BMP, 66 = GIF, 67 = JPEG, 68 = PNG, 69 = BMP comprimido (zip), 70 = WAV (sonido),
71 = JPEG 24 bpp + paleta (representa un bitmap original de 8 bpp).

Verificado en `testfiles/TestBitmapFill.xar`: tag 68, tamaño 2177,
payload = `"Default\0"` en UTF-16 (16 bytes) seguido de `89 50 4E 47 0D 0A 1A 0A …`.

**`TAG_PREVIEWBITMAP_*` (60–64)** — igual pero **sin el nombre**: el payload es
directamente el fichero de imagen. Verificado: en `testfiles/*.xar` el record 61 empieza
por `47 49 46 38 37 61` (`GIF87a`). El importador original simplemente hace *seek* y lo
descarta (`Kernel/rechbmp.cpp:305-325`). Es el thumbnail del documento.

**`TAG_NODE_BITMAP` (198), 36 bytes** — `Kernel/cxfnbmp.cpp:118-130`:
`DocCoord p0..p3` + `i32 bitmap_ref`.
Los 4 puntos son las esquinas del paralelogramo que encuadra la imagen
(`NodeBitmap::Parallel[0..3]`): la distancia `p0→p1` es el **ancho** y `p1→p2` el **alto**
(`Kernel/nodebmp.cpp:468-469`); al convertirlo en relleno de bitmap se usa
`p3` como *start point*, `p2` como *end point* y `p0` como *end point 2*
(`Kernel/nodebmp.cpp:1210-1212`).

**`TAG_NODE_CONTONEDBITMAP` (199), 44 bytes** — igual + `i32 start_colour_ref`
+ `i32 end_colour_ref`.

**`TAG_BITMAP_PROPERTIES` (4115), 12 bytes** — `i32 bitmap_ref`, `u8 flags`,
7 bytes reservados a 0 (`Kernel/bmpcomp.cpp:1810-1826`).

**`TAG_DOCUMENTBITMAPSMOOTHING` (4116), 5 bytes** — `u8 flags` + 4 reservados
(`Kernel/camfiltr.cpp:6885-6895`).

**`TAG_XPE_BITMAP_PROPERTIES` (4117)**, variable: `i32 bmp_ref`, `u8 flags`, `u8 0`,
`i32 master_record`, `UNICODE-Z nombre`, `BSTR xml` (en XaraLX el XML está desactivado:
`PORTNOTE` en `Kernel/bmpcomp.cpp:1780-1800`).

### 4.6. Categoría: vistas, unidades e información del documento

| tag | record | payload |
|---|---|---|
| 80 | `TAG_VIEWPORT` (16) | `DocCoord lo`, `DocCoord hi` (`Kernel/viewcomp.cpp:505-515`) |
| 81 | `TAG_VIEWQUALITY` (1) | `u8 quality` (`Kernel/viewcomp.cpp:565-572`) |
| 82 | `TAG_DOCUMENTVIEW` (24) | `FIXED16 scale`, `DocCoord lo`, `DocCoord hi`, `u32 flags` (`Kernel/viewcomp.cpp:678-690`) |
| 85/86 | `TAG_DEFINE_*USERUNIT` (28+strings) | `UNICODE-Z nombre`, `UNICODE-Z abreviatura`, `f64 tamaño_unidad`, `i32 unidad_base` (REFERENCE), `f64 numerador`, `f64 denominador` (`Kernel/unitcomp.cpp:950-1000`) |
| 87 | `TAG_DEFINE_DEFAULTUNITS` (8) | `i32 page_units`, `i32 font_units` (REFERENCEs, normalmente negativas: §5.2) |
| 90 | `TAG_DOCUMENTCOMMENT` (var) | `UNICODE-Z comentario` |
| 91 | `TAG_DOCUMENTDATES` (8) | `i32 creation`, `i32 last_saved` (segundos estilo `time_t`) |
| 92 | `TAG_DOCUMENTUNDOSIZE` (4) | `u32 bytes` |
| 93 | `TAG_DOCUMENTFLAGS` (4) | `u32 flags`: bit0 = todas las capas visibles, bit1 = multicapa (`Kernel/infocomp.cpp:744-752`) |
| 4114 | `TAG_DOCUMENTNUDGE` (4) | `u32 nudge` en millipoints |

### 4.7. Categoría: objetos geométricos

Caminos (100–103, 113–116, 111, 118, 4013) → **§7**.
Formas regulares (1000–1901) → §4.7.1.

#### 4.7.1. Formas regulares

Las familias 1000/1100/1200 son **legacy**: Xara X y posteriores escriben *siempre*
`TAG_REGULAR_SHAPE_PHASE_2` (1901). En el corpus: 35 206 records 1901 y **cero** de
cualquier otra forma regular. Aun así, un importador completo debería soportarlas porque
ficheros antiguos (Camelot 1.5) las usan.

**`TAG_REGULAR_SHAPE_PHASE_2` (1901)** — escritura `Kernel/cxfrgshp.cpp:338-360`,
lectura `Kernel/rechrshp.cpp:245-295`:

| Off | Tipo | Campo |
|---|---|---|
| 0 | `u8` | `flags` |
| 1 | `u16` | `num_sides` |
| 3 | `DocCoord` | `major_axis` (vector, sin trasladar por el origen) |
| 11 | `DocCoord` | `minor_axis` |
| 19 | `Matrix` (24) | matriz de transformación (sí trasladada, §5.4) |
| 43 | `f64` | `stellation_radius` |
| 51 | `f64` | `stellation_offset` |
| 59 | `f64` | `primary_curvature` |
| 67 | `f64` | `secondary_curvature` |
| 75 | path | `primary_edge_path` (formato absoluto §7.2) |
| … | path | `secondary_edge_path` |

`flags` (`Kernel/cxfrgshp.cpp:481-495`): `0x01` circular (elipse/círculo),
`0x02` estrellado, `0x04` curvatura primaria activa, `0x08` curvatura de estrellado activa.
El centro se asume `(0,0)` **en el espacio sin transformar**: la posición real la aporta
la matriz. (`TAG_REGULAR_SHAPE_PHASE_1` (1900) es idéntico pero con un `DocCoord
ut_centre` extra tras `num_sides`; solo existe en ficheros muy antiguos,
`Kernel/cxftags.h:360-374`.)

Tamaño observado típico: 119 bytes (75 + 40 de un path de 4 puntos + 4 del path vacío).

**Familias legacy** (`Kernel/cxfellp.cpp:157-186`, `Kernel/cxfrect.cpp:196-470`,
`Kernel/cxfpoly.cpp:178-320`) — reglas de composición del payload, en este orden:

1. `u16 num_sides` — solo polígonos.
2. *simple*: `DocCoord centre`, `i32 width`, `i32 height`
   *complex*: `DocCoord centre`, `DocCoord major_axis`, `DocCoord minor_axis`
   *…_reformed complex*: `DocCoord ut_major`, `DocCoord ut_minor`, `Matrix` (sin centro).
3. *stellated*: `f64 stellation_radius`, `f64 stellation_offset`.
4. *rounded*: `f64 curvature` (o `f64 primary` + `f64 secondary` si además es estrellado).
5. *reformed*: uno o dos paths absolutos (`primary`, `secondary`).

### 4.8. Categoría: contenedores y efectos

| tag | record | payload |
|---|---|---|
| 104 | `TAG_GROUP` | vacío (`Kernel/group.cpp:1684`) |
| 105 | `TAG_BLEND` (3) | `u16 num_steps`, `u8 flags` (`Kernel/nodeblnd.cpp:3546-3556`). Flags (`Kernel/cxfdefs.h:352-357`): bit0 one-to-one, bit1 antialias, bit2 tangencial; bits 4-7 = tipo de interpolación de color (`(f & 0xF0) >> 4`) |
| 106 | `TAG_BLENDER` (8) | `i32 path_index_start`, `i32 path_index_end` (`Kernel/nodebldr.cpp:7188-7194`) |
| 4073 | `TAG_BLENDERADDITIONAL` (17) | `i32 blended_on_curve`, `i32 node_blend_path_index` (−1 si −2), `i32 obj_index_start`, `i32 obj_index_end`, `u8 bitfield` (bit0 = invertido) |
| 4072 | `TAG_BLENDPROFILES` (48) | 6 × `f64`: bias/gain de objeto, de atributo y de posición |
| 4060 | `TAG_BLENDER_CURVEPROP` (16) | `f64 prop_start`, `f64 prop_end` |
| 4062 | `TAG_BLENDER_CURVEANGLES` (16) | `f64 angle_start`, `f64 angle_end` |
| 4061 | `TAG_BLEND_PATH` (var) | path absoluto (`Kernel/ndbrshpt.cpp:296-306`) |
| 4074 | `TAG_NODEBLENDPATH_FILLED` (4) | `i32 filled` |
| 107 | `TAG_MOULD_ENVELOPE` (4) | `i32 threshold` (`Kernel/nodemold.cpp:2705-2727`) |
| 108 | `TAG_MOULD_PERSPECTIVE` (4) | `i32 threshold` |
| 109 | `TAG_MOULD_GROUP` | vacío |
| 110 | `TAG_MOULD_PATH` (var) | path absoluto que define la malla (`Kernel/ndmldpth.cpp:590-600`) |
| 4012 | `TAG_MOULD_BOUNDS` (16) | `DocCoord lo`, `DocCoord hi` del objeto original (`Kernel/ndmldgrp.cpp:800-810`) |
| 4050 | `TAG_SHADOWCONTROLLER` (29) | `u8 shadow_type`, `i32 penumbra_width`, `i32 offset_x`, `i32 offset_y`, `i32 floor_angle` (radianes codificados en entero), `i32 floor_height` (×100), `i32 scale` (×100), `i32 feather_or_glow_width` (`Kernel/nodecont.cpp:1613-1650`) |
| 4051 | `TAG_SHADOW` (24) | `f64 bias`, `f64 gain`, `f64 darkness` (`Kernel/nodeshad.cpp:1800-1815`) |
| 4052 | `TAG_BEVEL` (24) | `i32 type`, `i32 indent`, `i32 light_angle`, `i32 is_outer`, `i32 contrast`, `i32 tilt` (`Kernel/nbevcont.cpp:500-520`). ⚠️ El tamaño **no** está en `cxfdefs.h`; está codificado a mano en el escritor |
| 4057 | `TAG_BEVELINK` | vacío |
| 4053-4056 | `TAG_BEVATTR_*` (4) | `i32 valor` (indent / light angle / contrast / type) (`Kernel/nodebev.cpp:3378-3400`) |
| 4066 | `TAG_CONTOURCONTROLLER` (41) | `i32 steps`, `i32 width`, `u8 type`, `f64 obj_bias`, `f64 obj_gain`, `f64 attr_bias`, `f64 attr_gain` (`Kernel/ncntrcnt.cpp:1600-1620`) |
| 4067 | `TAG_CONTOUR` | vacío |
| 4084/4085 | `TAG_CLIPVIEWCONTROLLER` / `TAG_CLIPVIEW` | vacíos (`Kernel/nodeclip.cpp:503`) |
| 4086 | `TAG_FEATHER` (20) | `i32 size` (millipoints), `f64 bias`, `f64 gain` (`Kernel/fthrattr.cpp:2225-2240`) |
| 4125 | `TAG_LIVE_EFFECT` (var) | `u8 flags`, `f64 pixels_per_inch`, `UNICODE-Z effect_id`, `UNICODE-Z display_name`, `UTF16STR xml_edits` (`Kernel/nodeliveeffect.cpp:2648-2680`) |
| 4126/4127 | `TAG_LOCKED_EFFECT`, `TAG_FEATHER_EFFECT` | mismos campos que 4125 (mismo escritor) |
| 4128 | `TAG_COMPOUNDRENDER` (20) | `u32 reservado (0)`, `DocCoord lo`, `DocCoord hi` (`Kernel/group.cpp:1720-1735`) |
| 4129 | `TAG_OBJECTBOUNDS` (16) | `DocCoord lo`, `DocCoord hi` (`Kernel/cxftext.cpp:1035-1050`) |
| 4015 | `TAG_EXPORT_HINT` (var) | `u32 type`, `u32 width`, `u32 height`, `u32 bpp`, `ASCII-Z options` (`Kernel/exphint.cpp:230-245`) |
| 4020 | `TAG_WEBADDRESS` (var) | `UNICODE-Z url`, `UNICODE-Z frame` (`Kernel/rechattr.cpp:300-330`) |
| 4021 | `TAG_WEBADDRESS_BOUNDINGBOX` (var) | `DocCoord lo` **intercalada**, `DocCoord hi` **intercalada**, `UNICODE-Z url`, `UNICODE-Z frame` (`Kernel/rechattr.cpp:355-380`) |
| 189 | `TAG_USERVALUE` (var) | `UNICODE-Z clave`, `UNICODE-Z valor` |
| 4040 | `TAG_WIZOP` (var) | 4 × `UNICODE-Z`: nombre interno, pregunta, parámetro, cadena vacía (`Kernel/tmpltatr.cpp:395-415`) |
| 4042 | `TAG_WIZOP_STYLEREF` (4) | `i32 referencia al estilo` |

### 4.9. Categoría: impresión y filmación

| tag | record | payload |
|---|---|---|
| 3500-3505 | `TAG_OVERPRINT*`, `TAG_PRINTONALLPLATES*` | vacíos (atributos booleanos) |
| 3506 | `TAG_PRINTERSETTINGS` (45) | 45 campos de 1 byte y enteros, escritos con macros (`Kernel/princomp.cpp:700-780`). Poco interesante para un importador gráfico |
| 3507 | `TAG_IMAGESETTING` (15) | `i32 print_resolution`, `f64 screen_frequency`, `u16 screen_function`, `u8 flags` (`Kernel/princomp.cpp:880-895`) |
| 3508 | `TAG_COLOURPLATE` (22) | `u8 type`, `i32 spot_colour_ref`, `f64 screen_angle`, `f64 screen_frequency`, `u8 flags` (`Kernel/princomp.cpp:960-985`) |
| 3509 | `TAG_PRINTMARKDEFAULT` (1) | `u8 id` de la marca (`Kernel/prnmkcom.cpp:710-720`) |
| 3510 | `TAG_PRINTMARKCUSTOM` (var) | marca personalizada; su contenido es un **subárbol** de objetos |

### 4.10. Categoría: pinceles y trazos (strokes)

Todos son *poco frecuentes* (solo `testfiles/Brush Test.xar` los usa en el corpus).

| tag | record | payload resumido |
|---|---|---|
| 4002 | `TAG_STROKETYPE` (4) | `u32 handle` de definición de trazo (`Kernel/strkattr.cpp:540-555`) |
| 4000 | `TAG_VARIABLEWIDTHFUNC` (4) | `u32 0`, `u32 id_función` — ⚠️ el escritor emite **8** bytes aunque `cxfdefs.h` declare 4 (`Kernel/strkattr.cpp:1340-1350`) |
| 4001 | `TAG_VARIABLEWIDTHTABLE` (var) | tabla de anchuras |
| 4003 | `TAG_STROKEDEFINITION` (var) | `u32 handle`, `u32 flags`, `u32 num_repeats`, `u32 0` (`Kernel/strkcomp.cpp:865-880`) |
| 4079 | `TAG_BRUSHATTR` (33) | `u32 brush_handle`, `i32 spacing`, `u8 flags`, `f64 rotate_angle`, `i32 path_offset_type`, `i32 path_offset_value`, `f64 scaling` (`Kernel/ppbrush.cpp:7355-7375`) |
| 4080 | `TAG_BRUSHDEFINITION` (4) | `u32 handle` (`Kernel/brshcomp.cpp:3350-3360`) |
| 4082/4083 | `TAG_MOREBRUSHDATA` (68) / `TAG_MOREBRUSHATTR` (72) | 11×`i32` + 3×`f64` (+ `i32` extra en ATTR) |
| 4102/4103 | `TAG_EVENMOREBRUSHDATA` (8) / `…ATTR` (9) | 2×`i32` (+ `u8`) |
| 4105/4107 | `TAG_BRUSHPRESSUREINFO` / `…ATTRPRESSUREINFO` (28) | 7×`i32` |
| 4112/4113 | `TAG_BRUSHTRANSPINFO` / `…ATTRTRANSPINFO` (40) | 6×`i32` + 2×`f64` |
| 4111 | `TAG_BRUSHATTRFILLFLAGS` (1) | `u8` con `0x1` local fill, `0x2` local transp, `0x4` named colour (`Kernel/cxfdefs.h:585-588`) |

### 4.11. Tags declarados pero **sin implementación** en Xara LX

Los siguientes 21 tags están definidos en `Kernel/cxftags.h` pero **no tienen ni escritor
ni lector** en todo el árbol de código (búsqueda exhaustiva de referencias fuera de las
tablas `cxftags.h`, `cxfdefs.h`, `cxfrech.cpp`, `cxftree.cpp`, `cxfmap.cpp`).
**Su layout es desconocido** y debe tratarse como “tag desconocido” (§2.3):

```
20  TAG_NONRENDERSECTION_START      21  TAG_NONRENDERSECTION_END
22  TAG_RENDERING_PAUSE             23  TAG_RENDERING_RESUME
95  TAG_NAMEGAL_DOCCOMP            187  TAG_DEFINEARROW
4014 TAG_PATHREF_TRANSLATE         4063 TAG_GROUPTRANSP
4064 TAG_CACHEBMP                  4065 TAG_CACHEDNODESGROUP
4108 TAG_BRUSHCOLOURDATA           4110 TAG_BRUSHTIMESAMPLEDATA
4130 TAG_DIMENSION                 4134 TAG_SPREAD_FLASHPROPS
4135 TAG_PRINTERSETTINGS_PHASE2    4136 TAG_DOCUMENTINFORMATION
4137 TAG_CLIPVIEW_PATH             4138 TAG_DEFINEBITMAP_PNG_REAL
4205 TAG_TEXT_STORY_HEIGHT_INFO    4206 TAG_TEXT_STORY_LINK_INFO
4207 TAG_TEXT_STORY_TRANSLATION_INFO
```

Además, `TAG_SPREAD_PHASE2` (4131) y `TAG_CURRENTATTRIBUTES_PHASE2` (4132) solo aparecen
**comentados** en `Kernel/camfiltr.cpp:7014-7015`: están reservados para versiones
posteriores de Xara (Xara Xtreme Pro comercial) y **pueden aparecer en ficheros modernos**
sin que este código sepa leerlos (§11).

`TAG_DEFINEARROW` (187) merece mención aparte: el formato prevé flechas personalizadas
definidas por el usuario, pero Xara LX solo sabe escribir/leer las 8 flechas predefinidas
por referencia negativa (§8.6).

### 4.12. Categoría: texto

Referencias: `Kernel/rechtext.cpp` (lectura de todos los records de texto),
`Kernel/cxftext.cpp` (escritura), `Kernel/cxfdefs.h:445-500` (tamaños).

#### 4.12.1. Definiciones de fuente (referenciables)

**`TAG_FONT_DEF_TRUETYPE` (2000)** y **`TAG_FONT_DEF_ATM` (2001)**, tamaño variable
(`Kernel/fontcomp.cpp:893-930`):

| Tipo | Campo |
|---|---|
| `UNICODE-Z` | nombre completo de la fuente (p. ej. `"Arial"`) |
| `UNICODE-Z` | nombre del *typeface* |
| `CCPanose` (10 bytes) | número PANOSE |

Verificado: `TextDesigns/SimpleText.xar` → size 34 = 12 + 12 + 10.
**No se embeben los glifos**: el fichero solo guarda la identificación de la fuente
(el nombre `embeddedFonts.xar` del corpus es engañoso; sigue usando estos records).

#### 4.12.2. Historias de texto (nodos raíz de un bloque de texto)

| tag | record | payload |
|---|---|---|
| 2100 | `TAG_TEXT_STORY_SIMPLE` (12) | `DocCoord anchor`, `i32 autokern` |
| 2101 | `TAG_TEXT_STORY_COMPLEX` (28) | `Matrix` (24), `i32 autokern` |
| 2110-2113 | `TAG_TEXT_STORY_SIMPLE_{START,END}_{LEFT,RIGHT}` (12) | `DocCoord`, `i32 autokern` — texto sobre camino: el nombre indica desde qué extremo y en qué dirección fluye |
| 2114-2117 | `TAG_TEXT_STORY_COMPLEX_{START,END}_{LEFT,RIGHT}` (36) | `Matrix` (24), `ANGLE rotation`, `ANGLE shear`, `i32 autokern` |

El campo `autokern` se lee con `ReadINT32noError` (`Kernel/rechtext.cpp:756-770`): si el
record es de la versión antigua (4 bytes menos) simplemente no está. **El lector Rust debe
hacer lo mismo.**

#### 4.12.3. Estructura interna de una historia

```
TAG_TEXT_STORY_*            (nodo historia)
  TAG_DOWN
    TAG_TEXT_STORY_WORD_WRAP_INFO   (i32 width, u8 word_wrap_on)      [5 bytes]
    TAG_TEXT_STORY_INDENT_INFO      (i32 left_indent, i32 right_indent)[8 bytes]
    TAG_TEXT_LINE                   (vacío)  <- una por línea
      TAG_DOWN
        TAG_TEXT_LINE_INFO          (i32 width, i32 height, i32 dist_prev) [12]
        TAG_TEXT_STRING             (UTF-16 sin NUL)  <- texto plano
        TAG_TEXT_CHAR               (u16)             <- carácter suelto con atributos propios
        TAG_TEXT_KERN               (i32 dx, i32 dy)
        TAG_TEXT_TAB                (vacío)
        TAG_TEXT_EOL                (vacío)
        ... atributos de texto (2900-2920, 4201-4204) ...
      TAG_UP
  TAG_UP
```

`TAG_TEXT_STRING` (2201) se importa de forma peculiar: el lector inserta **solo el primer
carácter** como nodo, y expande el resto al terminar el subárbol, copiando los atributos
hijos del primer carácter (`Kernel/rechtext.cpp:620-662`). Para un importador nuevo esto
es irrelevante salvo por una consecuencia semántica: **los atributos que cuelgan de un
`TAG_TEXT_STRING` aplican a toda la cadena**.

#### 4.12.4. Atributos de texto

| tag | record | payload |
|---|---|---|
| 2900 | `TAG_TEXT_LINESPACE_RATIO` (4) | `FIXED16 ratio` |
| 2901 | `TAG_TEXT_LINESPACE_ABSOLUTE` (4) | `i32 millipoints` |
| 2902-2905 | `TAG_TEXT_JUSTIFICATION_{LEFT,CENTRE,RIGHT,FULL}` (0) | vacío |
| 2906 | `TAG_TEXT_FONT_SIZE` (4) | `i32 millipoints` (10 pt → 10000) |
| 2907 | `TAG_TEXT_FONT_TYPEFACE` (4) | `i32` **referencia** al record `TAG_FONT_DEF_*` |
| 2908/2909 | `TAG_TEXT_BOLD_ON/OFF` (0) | vacío |
| 2910/2911 | `TAG_TEXT_ITALIC_ON/OFF` (0) | vacío |
| 2912/2913 | `TAG_TEXT_UNDERLINE_ON/OFF` (0) | vacío |
| 2914 | `TAG_TEXT_SCRIPT_ON` (8) | `FIXED16 offset`, `FIXED16 size` |
| 2915 | `TAG_TEXT_SCRIPT_OFF` (0) | vacío |
| 2916/2917 | `TAG_TEXT_SUPERSCRIPT_ON` / `SUBSCRIPT_ON` (0) | vacío (valores implícitos) |
| 2918 | `TAG_TEXT_TRACKING` (4) | `i32` — el tipo interno es `MILLIPOINT` (`Kernel/txtattr.h:413`), pero se aplica escalado al cuerpo de la fuente; **unidad exacta ambigua, verificar contra render** (§11) |
| 2919 | `TAG_TEXT_ASPECT_RATIO` (4) | `FIXED16` |
| 2920 | `TAG_TEXT_BASELINE` (4) | `i32 millipoints` |
| 4201/4202/4203 | `TAG_TEXT_{LEFT,FIRST,RIGHT}_INDENT` (4) | `i32 millipoints` |
| 4204 | `TAG_TEXT_RULER` (var) | `u16 num_entries`; por entrada: `u8 type_and_flags`, `i32 position`, y —si el tipo lo indica— `u16 decimal_char` y/o `u16 tab_filler_char` (`Kernel/rechtext.cpp:1547-1580`) |

---

## 5. Sistema de coordenadas y unidades

### 5.1. Millipoints

La unidad interna de Xara es el **millipoint** = 1/1000 de punto PostScript
(`Kernel/units.h:115-132`, `Kernel/rechdoc.cpp:598` usa el tipo `MILLIPOINT`).

| Unidad | millipoints | constante |
|---|---:|---|
| 1 millipoint | 1 | `MP_MP_VAL` |
| 1 punto (pt) | 1 000 | `PT_MP_VAL` |
| 1 pica | 12 000 | `PI_MP_VAL` |
| 1 pulgada | 72 000 | `IN_MP_VAL` |
| 1 pie | 864 000 | `FT_MP_VAL` |
| 1 yarda | 2 592 000 | `YD_MP_VAL` |
| 1 milla | 4 561 920 000 | `MI_MP_VAL` |
| 1 milímetro | 2 834.652715 | `MM_MP_VAL` |
| 1 centímetro | 28 346.52715 | `CM_MP_VAL` |
| 1 metro | 2 834 652.715 | `M_MP_VAL` |
| 1 kilómetro | 2 834 652 715 | `KM_MP_VAL` |
| 1 píxel (a 96 dpi) | 750 | `PX_MP_VAL` |

Conversiones útiles para el importador:

```rust
pub const MP_PER_PT: f64 = 1000.0;
pub const MP_PER_IN: f64 = 72_000.0;
pub const MP_PER_MM: f64 = 2834.652715;   // = 72000 / 25.4
pub const MP_PER_PX96: f64 = 750.0;

#[inline] pub fn mp_to_pt(v: i32) -> f64 { v as f64 / MP_PER_PT }
#[inline] pub fn mp_to_mm(v: i32) -> f64 { v as f64 / MP_PER_MM }
#[inline] pub fn mp_to_px(v: i32, dpi: f64) -> f64 { v as f64 * dpi / MP_PER_IN }
```

### 5.2. Precisión y rango

Las coordenadas son `i32` con signo → rango ±2 147 483 647 millipoints ≈ ±2 147 483 pt
≈ ±29 826 pulgadas ≈ ±757 metros. La resolución es 1/1000 pt ≈ 0.35 µm.
**Un importador debe usar `i32` (no `f32`) para las coordenadas nativas** y convertir a
punto flotante solo al final: `f32` pierde precisión a partir de ~16.7 millones de
millipoints (≈ 233 pt de error en el peor caso de un documento grande).

Unidades predefinidas referenciadas por número negativo (`Kernel/cxfunits.h:104-116`):

| valor | unidad |
|---:|---|
| −1 | sin tipo |
| −2 | milímetros |
| −3 | centímetros |
| −4 | metros |
| −5 | kilómetros |
| −6 | millipoints |
| −7 | puntos de ordenador (pt) |
| −8 | picas |
| −9 | pulgadas |
| −10 | pies |
| −11 | yardas |
| −12 | millas |
| −13 | píxeles |

(`testfiles/OneLine.xar` → `TAG_DEFINE_DEFAULTUNITS` = `F3 FF FF FF F9 FF FF FF` =
−13 (píxeles) para página y −7 (puntos) para fuente.)

### 5.3. Origen y dirección del eje Y

* **Eje Y hacia arriba.** El sistema es cartesiano clásico: `Spread::SetPageSize` coloca
  “la esquina **inferior** izquierda de la página inferior-izquierda” en el origen
  (`Kernel/spread.cpp:1843-1850`), y `DocRect` tiene `lo` = esquina inferior-izquierda y
  `hi` = superior-derecha.
* **Origen de coordenadas de los records: la esquina `lo` del rectángulo que engloba todas
  las páginas del spread.** Al exportar se resta ese origen a cada coordenada y al
  importar se le suma (`Kernel/cxfrec.cpp:2080-2135` y `2225-2280`,
  `CamelotFileRecord::Write/ReadCoord` con `CoordOrigin`).
  El origen se fija en `BaseCamelotFilter::SetCoordOrigin()` (`Kernel/camfiltr.cpp:5539`),
  y durante la importación se actualiza al procesar `TAG_SPREADINFORMATION`
  (`Kernel/rechdoc.cpp:640-644`).
* **Consecuencia práctica**: en un fichero de un solo spread cuyo pasteboard empieza en el
  origen del documento, `CoordOrigin == (0,0)` y las coordenadas del fichero son
  directamente coordenadas de página con Y hacia arriba, con `(0,0)` en la esquina
  inferior izquierda de la página. Verificado en `testfiles/OneLine.xar`: página de
  600 000 × 450 000 mp y el camino en (112 101, 178 899) → (283 101, 321 399), dentro de
  la página.
* **Al exportar a SVG/PDF/cualquier sistema con Y hacia abajo** hay que aplicar
  `y' = page_height - y`.

⚠️ **Matiz importante para implementadores:** solo las coordenadas leídas mediante
`ReadCoord`, `ReadCoordInterleaved`, `ReadPath`, `ReadPathRelative`, `ReadMatrix`,
`ReadXOrd` y `ReadYOrd` reciben la traslación del origen. Los **vectores** (ejes mayor y
menor de las formas regulares) se escriben con `WriteCoordTrans(...,0,0)`, es decir **sin
traslación** (`Kernel/cxfrgshp.cpp:347-348`). Al implementar hay que respetar esa
distinción: puntos → trasladados; vectores → no.

### 5.4. Matrices

Formato (24 bytes, `Kernel/cxfrec.cpp:1913-1959`):

```
FIXED16 a   FIXED16 b   FIXED16 c   FIXED16 d   INT32 e   INT32 f
```

que corresponde a la transformación afín

```
| x' |   | a  c |   | x |   | e |
|    | = |      | * |   | + |   |
| y' |   | b  d |   | y |   | f |
```

* `a, b, c, d` son **FIXED16** (÷65536) → precisión ≈ 1.5 × 10⁻⁵ en los factores de escala
  y rotación. **Esto limita la fidelidad de rotaciones**: un ángulo se representa con
  ~4.5 segundos de arco de error como máximo.
* `e, f` son millipoints y **sí** llevan la traslación del origen de coordenadas
  (`ReadMatrixTrans` suma `CoordOrigin`).

```rust
#[derive(Clone, Copy, Debug)]
pub struct Matrix { pub a: f64, pub b: f64, pub c: f64, pub d: f64, pub e: i32, pub f: i32 }

impl Matrix {
    pub fn apply(&self, p: Coord) -> Coord {
        Coord {
            x: (self.a * p.x as f64 + self.c * p.y as f64).round() as i32 + self.e,
            y: (self.b * p.x as f64 + self.d * p.y as f64).round() as i32 + self.f,
        }
    }
}
```

---

## 6. Modelo de árbol, definiciones y referencias

### 6.1. Construcción del árbol con DOWN/UP

El fichero es un recorrido en preorden del árbol de nodos del documento:

* Un record de **objeto** crea un nodo y lo inserta como **hermano siguiente** del último
  nodo insertado en el nivel actual (`BaseCamelotFilter::InsertNode`,
  `Kernel/camfiltr.cpp:5999-6040`).
* `TAG_DOWN` (1) apila el nivel: el siguiente nodo insertado será **hijo** del último nodo
  (`IncInsertLevel`, `Kernel/camfiltr.cpp:6507-6532`).
* `TAG_UP` (0) desapila: vuelve al nivel anterior y notifica al nodo padre que su subárbol
  está completo (`DecInsertLevel`, `Kernel/camfiltr.cpp:6556-6590`; es donde los nodos
  compuestos —moldes, blends, textos— hacen su inicialización diferida
  `ReadPostChildren*`).

Jerarquía canónica de un documento:

```
TAG_FILEHEADER
TAG_PREVIEWBITMAP_GIF                 (opcional, sin comprimir)
TAG_DOCUMENT
  DOWN
    TAG_DOCUMENTNUDGE, TAG_DOCUMENTBITMAPSMOOTHING, ...
    TAG_STARTCOMPRESSION
    TAG_VIEWPORT, TAG_ATOMICTAGS...
    TAG_CHAPTER
      DOWN
        TAG_SPREAD
          DOWN
            TAG_SPREADINFORMATION, TAG_SPREADSCALING_*, TAG_SPREAD_ANIMPROPS
            TAG_LAYER
              DOWN
                TAG_LAYERDETAILS
                <objetos del dibujo>
                  DOWN  <atributos e hijos del objeto>  UP
              UP
            TAG_GRIDRULERSETTINGS, TAG_GRIDRULERORIGIN
          UP
      UP
    TAG_SETSENTINEL, TAG_BARPROPERTY, unidades, info de documento, vistas
    TAG_ENDCOMPRESSION
  UP
TAG_ENDOFFILE
```

(Extraído de `testfiles/OneLine.xar`; profundidad máxima observada en el corpus: 13
niveles, en `testfiles/ProbeX16.xar`.)

Reglas especiales observadas en el código:

* Una **capa** insertada cuando el nodo de contexto ya es una capa se añade como
  *hermana*, no como hija (`Kernel/camfiltr.cpp:6006-6020`).
* Los **atributos** de un objeto se escriben como **hijos** de ese objeto
  (`NodePath::WriteBeginChildRecordsNative`, `Kernel/nodepath.cpp:2640-2683`: emite
  `TAG_DOWN`, luego `TAG_PATH_FLAGS`, luego los atributos, luego `TAG_UP`).

### 6.2. Pseudocódigo de construcción del árbol

```rust
pub struct Node { pub tag: u32, pub rec: u32, pub payload: Vec<u8>, pub children: Vec<Node> }

pub fn build_tree(records: impl Iterator<Item = Record>) -> Vec<Node> {
    let mut roots: Vec<Node> = Vec::new();
    let mut stack: Vec<Vec<Node>> = Vec::new();  // niveles abiertos
    let mut cur: Vec<Node> = Vec::new();         // hermanos del nivel actual

    for r in records {
        match r.tag {
            TAG_DOWN => { stack.push(std::mem::take(&mut cur)); }
            TAG_UP => {
                let finished = std::mem::replace(&mut cur, stack.pop().unwrap_or_default());
                if let Some(parent) = cur.last_mut() { parent.children = finished; }
                else { roots.extend(finished); }   // DOWN/UP desbalanceado: tolerar
            }
            TAG_ENDOFFILE => break,
            _ => cur.push(Node { tag: r.tag, rec: r.number, payload: r.data, children: vec![] }),
        }
    }
    while let Some(prev) = stack.pop() {           // cierre implícito al final
        let finished = std::mem::replace(&mut cur, prev);
        if let Some(parent) = cur.last_mut() { parent.children = finished; }
    }
    roots.extend(cur);
    roots
}
```

> **Nota:** en el corpus el número de `TAG_DOWN` y `TAG_UP` coincide exactamente
> (201 121 de cada uno), pero el parser debe tolerar desbalanceos (ficheros truncados).

### 6.3. Definiciones y referencias por número de record

**El número de record es la clave universal de referencia.** Se cuenta desde 1, contando
*todos* los records del fichero (incluidos `TAG_UP`/`TAG_DOWN`, la cabecera y los
comprimidos), en orden de aparición:

* Escritura: `RecordNumber++` en `CXaraFile::Write` (`Kernel/cxfile.cpp:1650`),
  `WriteRecordHeader` (`Kernel/cxfile.cpp:1709`) y `StartStreamedRecord`
  (`Kernel/cxfile.cpp:1458`).
* Lectura: `RecordNumber++` en `ReadNextRecordHeader` (`Kernel/cxfile.cpp:1863`).
* `WriteDefinitionRecord()` es idéntico a `Write()` a nivel binario
  (`Kernel/cxfile.cpp:1673-1677`); la distinción es puramente semántica.

Tipos de record que actúan como **definición referenciable**:

| definición | tags | referenciada desde |
|---|---|---|
| Color | 50, 51 | `TAG_FLATFILL`, `TAG_LINECOLOUR`, degradados, `TAG_NODE_CONTONEDBITMAP`, `TAG_COLOURPLATE`, colores padre (tintas/enlaces) |
| Bitmap | 65–71 | `TAG_BITMAPFILL`, `TAG_CONTONEBITMAPFILL`, `TAG_BITMAPTRANSPARENTFILL`, `TAG_NODE_BITMAP`, `TAG_BITMAP_PROPERTIES` |
| Fuente | 2000, 2001 | `TAG_TEXT_FONT_TYPEFACE` |
| Camino | 100–103, 113–116 | `TAG_PATHREF_TRANSFORM`, `TAG_PATHREF_IDENTICAL` |
| Unidad | 85, 86 | `TAG_DEFINE_DEFAULTUNITS`, `TAG_GRIDRULERSETTINGS`, `TAG_SPREADSCALING_*` |
| Estilo (WizOp) | 4041 | `TAG_WIZOP_STYLEREF` |
| Trazo / pincel | 4003, 4080 | `TAG_STROKETYPE`, `TAG_BRUSHATTR` (por *handle*, no por número de record) |

**Referencias negativas = objetos predefinidos (built-in)**, no son números de record:

* Colores (`Kernel/cxfcols.h:104-113`): −1 transparente/ninguno, −2 negro, −3 blanco,
  −4 rojo, −5 verde, −6 azul, −7 cian, −8 magenta, −9 amarillo, −10 key (negro CMYK).
* Bitmap (`Kernel/cxfdefs.h:158`): −1 bitmap por defecto.
* Flechas (`Kernel/cxfarrow.h:104-112`): −1 ninguna, −2 recta, −3 angulada, −4 redondeada,
  −5 punto, −6 rombo, −7 pluma, −8 pluma 2, −9 rombo hueco.
* Guiones (`Kernel/cxfdash.h:104-126`): −1..−20 patrones 1..20, −21 continuo,
  −22 patrón de capa de guías.
* Unidades: ver §5.2.

Un valor de referencia **0 significa error / ninguna** (el escritor devuelve 0 en caso de
fallo, `Kernel/cxfile.cpp:1654`).

```rust
#[derive(Copy, Clone, Debug)]
pub enum Ref { None, Builtin(i32), Record(u32) }

impl Ref {
    pub fn parse(v: i32) -> Ref {
        match v { 0 => Ref::None, n if n < 0 => Ref::Builtin(n), n => Ref::Record(n as u32) }
    }
}
```

**Implicación de diseño para el importador Rust:** hay que hacer **dos pasadas** o bien
mantener un `HashMap<u32, Definicion>` que se va llenando a medida que se leen los records
(las definiciones siempre se escriben **antes** de su primer uso, porque el exportador las
emite al vuelo: `Kernel/fillattr.cpp:5933` escribe el color justo antes del relleno que lo
usa). Una sola pasada con mapa incremental es suficiente y es lo que hace el código
original.

---

## 7. Representación de caminos (paths)

### 7.1. Tags de camino y su semántica

| tag | formato | relleno | trazo |
|---|---|---|---|
| 100 `TAG_PATH` | absoluto | no | no |
| 101 `TAG_PATH_FILLED` | absoluto | sí | no |
| 102 `TAG_PATH_STROKED` | absoluto | no | sí |
| 103 `TAG_PATH_FILLED_STROKED` | absoluto | sí | sí |
| 113 `TAG_PATH_RELATIVE` | relativo | no | no |
| 114 `TAG_PATH_RELATIVE_FILLED` | relativo | sí | no |
| 115 `TAG_PATH_RELATIVE_STROKED` | relativo | no | sí |
| 116 `TAG_PATH_RELATIVE_FILLED_STROKED` | relativo | sí | sí |

Selección del tag: `NodePath::ChooseTagValue`, `Kernel/nodepath.cpp:2372-2400`.
En el corpus **solo aparecen los relativos** (115 y 116): Xara X exporta siempre en
formato relativo (`BaseCamelotFilter::WritePathsInRelativeFormat()`,
`Kernel/camfiltr.h:251`). Los absolutos siguen apareciendo *embebidos* dentro de otros
records (formas regulares reformadas, `TAG_MOULD_PATH`, `TAG_BLEND_PATH`), que usan
siempre el formato absoluto.

Los flags "relleno/trazo" del tag **no sustituyen a los atributos**: indican si el camino
debe rellenarse y/o trazarse con los atributos heredados (§8).

### 7.2. Formato absoluto (`WritePath` / `ReadPath`, `Kernel/cxfrec.cpp:1663-1745`)

```
i32  num_coords
u8   verb[num_coords]           // uno por punto
i32  x0, i32 y0                 // num_coords pares de coordenadas absolutas
i32  x1, i32 y1
...
```

Tamaño = `4 + num_coords * 9`.

### 7.3. Formato relativo intercalado (`Kernel/cxfrec.cpp:1749-1900`)

⚠️ **No hay contador de puntos.** El lector lo deduce:
`num_coords = record_size / 9` (`Kernel/cxfrec.cpp:1837`).

El macro `RELPATHINTERLEAVE` **está definido** (`Kernel/cxfrec.cpp:134`), por lo que la
rama activa es la intercalada:

```
por cada punto i:
    u8  verb
    u8  b0..b7   // 8 bytes con los bytes de X e Y intercalados (MSB primero):
                 //   b0 = (X >> 24) & 0xFF   b1 = (Y >> 24) & 0xFF
                 //   b2 = (X >> 16) & 0xFF   b3 = (Y >> 16) & 0xFF
                 //   b4 = (X >>  8) & 0xFF   b5 = (Y >>  8) & 0xFF
                 //   b6 = (X >>  0) & 0xFF   b7 = (Y >>  0) & 0xFF
```

donde el valor almacenado es:

* para el **punto 0**: la coordenada **absoluta** (ya trasladada por `CoordOrigin`);
* para el punto `i > 0`: el **delta invertido** `D = P[i-1] - P[i]`, de modo que
  **`P[i] = P[i-1] - D`** (`Kernel/cxfrec.cpp:1885-1889`).

> Ojo al signo: **no** es `P[i] = P[i-1] + D`. El escritor calcula
> `RelX = pCoord[i-1].x - pCoord[i].x` (`Kernel/cxfrec.cpp:1812`).

**Verificación con `testfiles/OneLine.xar`** (record 115, 18 bytes):

```
06 | 00 00 01 02 B5 BA E5 D3 | 02 | FF FF FD FD 64 D3 08 5C
 ^verb MOVETO   X=0x0001B5E5=112101  Y=0x0002BAD3=178899
                ^verb LINETO  D=(0xFFFD6408, 0xFFFDD35C) = (-171000, -142500)
                -> P1 = (112101+171000, 178899+142500) = (283101, 321399)
```

El intercalado existe para que los bytes altos (casi siempre iguales, 0x00 o 0xFF) queden
contiguos y zlib los comprima mejor.

### 7.4. Verbos (`PathVerb`)

`GDraw/gconsts.h:108-112`:

| Nombre en el original | Valor | Papel |
|---|---|---|
| `PT_CLOSEFIGURE` | `0x01` | bit-flag: este punto **cierra** el subcamino |
| `PT_LINETO` | `0x02` | tipo: segmento recto |
| `PT_BEZIERTO` | `0x04` | tipo: segmento Bézier cúbico |
| `PT_MOVETO` | `0x06` | tipo: inicio de subcamino (`PT_LINETO | PT_BEZIERTO`) |
| `PT_PATHELEMENT` | `0x06` | máscara para extraer el tipo |

Los bits 3–7 no se usan; el byte de verbo solo lleva tipo (bits 1–2) y cierre (bit 0).

Decodificación:

```
tipo = verb & 0x06     // PT_PATHELEMENT
  2 -> LineTo
  4 -> BezierTo   (los puntos vienen de 3 en 3: c1, c2, extremo)
  6 -> MoveTo
cerrado = verb & 0x01  // PT_CLOSEFIGURE: este punto CIERRA el subcamino
```

Observaciones:

* Los **bézier son cúbicos** y ocupan **tres puntos consecutivos** con verbo
  `PT_BEZIERTO`: los dos primeros son los puntos de control y el tercero el extremo
  (`Kernel/nodepath.cpp:1307`, `Kernel/gclips.h:232-238`).
* `PT_CLOSEFIGURE` es un **bit** que se combina con el verbo del **último punto** del
  subcamino (p. ej. `0x03` = LineTo que cierra, `0x05` = BezierTo que cierra).
* El valor 0 no es un verbo válido.

### 7.5. `TAG_PATH_FLAGS` (111): flags por punto

Escritura: `Kernel/nodepath.cpp:2645-2680`. Se emite como **primer hijo** del record de
camino (dentro de su `TAG_DOWN`). Payload: **un byte por punto**, en el mismo orden que
los puntos del camino; `size` = número de puntos.

`Kernel/cxfdefs.h:313-315`:

| bit | valor | nombre | significado |
|---|---|---|---|
| 0 | 0x01 | `TAG_PATH_FLAGS_SMOOTH` | el punto es suave (tangentes colineales) |
| 1 | 0x02 | `TAG_PATH_FLAGS_ROTATE` | el punto es de rotación (mantiene ángulo, no longitud) |
| 2 | 0x04 | `TAG_PATH_FLAGS_ENDPOINT` | el punto es un extremo (no un punto de control) |

Estos flags son **metainformación de edición**: no afectan a la geometría renderizada
(la geometría ya está en las coordenadas). Un importador puro puede ignorarlos; un editor
debe conservarlos.

### 7.6. Referencias entre caminos

* **`TAG_PATHREF_TRANSFORM` (118), 28 bytes** (`Kernel/nodepath.cpp:2537-2560`):
  `i32 src_path_record`, `Matrix` (24). El camino de este nodo es el del record
  referenciado transformado por la matriz. Se usa para deduplicar formas repetidas.
* **`TAG_PATHREF_IDENTICAL` (4013), 4 bytes**: `i32 src_path_record`.
* **`TAG_PATHREF_TRANSLATE` (4014)**: declarado, **sin implementación** (§4.11).

Ninguno aparece en el corpus (Xara solo los emite si el filtro define una tolerancia de
caminos similares; el filtro nativo devuelve tolerancia 0,
`Kernel/native.cpp:462-466`).

### 7.7. Pseudocódigo Rust

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb { MoveTo, LineTo, CurveTo }

#[derive(Clone, Debug)]
pub struct PathPoint { pub verb: Verb, pub close: bool, pub p: Coord, pub flags: u8 }

/// Caminos absolutos: TAG_PATH..TAG_PATH_FILLED_STROKED y paths embebidos.
pub fn read_path_absolute(c: &mut Cur, origin: Coord) -> Result<Vec<PathPoint>, Err> {
    let n = c.i32()?;
    if n < 0 { return Ok(vec![]); }
    let n = n as usize;
    let verbs: Vec<u8> = (0..n).map(|_| c.u8()).collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(n);
    for v in verbs {
        let p = c.coord()?;
        out.push(mk_point(v, Coord { x: p.x + origin.x, y: p.y + origin.y }));
    }
    Ok(out)
}

/// Caminos relativos intercalados: TAG_PATH_RELATIVE*. `size` = longitud del payload.
pub fn read_path_relative(c: &mut Cur, origin: Coord) -> Result<Vec<PathPoint>, Err> {
    let n = c.remaining() / 9;          // ¡no hay contador!
    let mut out: Vec<PathPoint> = Vec::with_capacity(n);
    for i in 0..n {
        let v = c.u8()?;
        let d = c.coord_interleaved()?;
        let p = if i == 0 {
            Coord { x: d.x + origin.x, y: d.y + origin.y }   // absoluto + origen
        } else {
            let prev = out[i - 1].p;
            Coord { x: prev.x - d.x, y: prev.y - d.y }        // OJO: RESTA
        };
        out.push(mk_point(v, p));
    }
    Ok(out)
}

fn mk_point(v: u8, p: Coord) -> PathPoint {
    let verb = match v & 0x06 { 2 => Verb::LineTo, 4 => Verb::CurveTo, _ => Verb::MoveTo };
    PathPoint { verb, close: v & 0x01 != 0, p, flags: 0 }
}

/// Aplica el record TAG_PATH_FLAGS (hijo del camino) sobre los puntos ya leídos.
pub fn apply_path_flags(pts: &mut [PathPoint], data: &[u8]) {
    for (pt, f) in pts.iter_mut().zip(data.iter()) { pt.flags = *f; }
}
```

---

## 8. Atributos y su modelo de herencia

### 8.1. Modelo conceptual

Xara **no** tiene una pila de atributos con push/pop explícito en el fichero. El modelo es:

* Los atributos son **nodos hijos** del objeto al que se aplican. Se emiten dentro del
  `TAG_DOWN … TAG_UP` del objeto (`Kernel/nodepath.cpp:2640-2683`).
* Un atributo aplicado a un **grupo** o a una **capa** se hereda por todos sus
  descendientes salvo que estos lo redefinan (modelo de herencia clásico del árbol de
  documento de Camelot: `Node::FindAppliedAttribute`).
* Por tanto, para renderizar, el importador debe mantener un **contexto de atributos por
  nivel de árbol**: al entrar en un `TAG_DOWN`, clonar (o apilar) el contexto; al salir en
  `TAG_UP`, restaurarlo; y los records de atributo del nivel actual modifican la copia
  local.
* Los records `TAG_CURRENTATTRIBUTES` (4119) + hijos **no** son atributos aplicados a
  objetos: describen los atributos *por defecto* del documento (lo que se aplicaría al
  siguiente objeto dibujado por el usuario). Son un nodo **atómico**: si no interesan,
  hay que descartar todo su subárbol.

```rust
#[derive(Clone, Default)]
pub struct AttrCtx {
    pub fill: Option<Fill>,
    pub line_colour: Option<Ref>,
    pub line_width: i32,          // millipoints
    pub start_cap: CapStyle,
    pub join: JoinStyle,
    pub mitre_limit: i32,
    pub winding: WindingRule,
    pub dash: Option<Dash>,
    pub start_arrow: Option<Arrow>,
    pub end_arrow: Option<Arrow>,
    pub fill_transp: Option<Transparency>,
    pub line_transp: Option<Transparency>,
    pub feather: Option<Feather>,
    pub text: TextAttrs,
}
// Recorrido: at DOWN -> stack.push(ctx.clone());  at UP -> ctx = stack.pop();
```

### 8.2. Atributos de línea

| tag | payload | notas |
|---|---|---|
| 151 `TAG_LINECOLOUR` | `i32 colour_ref` | §9 |
| 193/194/195 `TAG_LINECOLOUR_{NONE,BLACK,WHITE}` | vacío | atajos para las referencias −1/−2/−3 (`Kernel/lineattr.cpp:928-950`) |
| 152 `TAG_LINEWIDTH` | `i32 width` (millipoints; 0 = línea de grosor "hairline") | `Kernel/lineattr.cpp:539-553` |
| 174 `TAG_STARTCAP` | `u8 cap` | 1=butt, 2=round, 3=square (`Kernel/cxfdefs.h:132-134`) |
| 175 `TAG_ENDCAP` | `u8 cap` | mismos valores. Se escriben en pareja (`Kernel/lineattr.cpp:1856-1880`) |
| 176 `TAG_JOINSTYLE` | `u8 join` | 1=mitre, 2=round, 3=bevelled (`Kernel/cxfdefs.h:136-138`) |
| 177 `TAG_MITRELIMIT` | `i32` | `Kernel/lineattr.cpp:3191-3205` |
| 178 `TAG_WINDINGRULE` | `u8 rule` | 1=nonzero, 2=negative, 3=evenodd, 4=positive (`Kernel/cxfdefs.h:140-143`) |
| 173 `TAG_LINETRANSPARENCY` | `u8 transp`, `u8 type` | §8.5 |
| 179 `TAG_QUALITY` | `i32 quality` | calidad de render |

### 8.3. Rellenos de color

Todos los degradados comparten un prefijo geométrico de 2 o 3 `DocCoord`:

* `start_point` — origen del degradado.
* `end_point` — fin del eje principal.
* `end_point2` — fin del eje secundario (solo elíptico, cuadrado, 3/4 colores, bitmap,
  fractal, ruido y lineal de 3 puntos).

y un sufijo de **perfil** `f64 bias`, `f64 gain` (añadido en Xara X; por eso los tamaños
"crecieron +16" en `Kernel/cxfdefs.h:249-262`).

| tag | tam. | payload |
|---|---:|---|
| 150 `TAG_FLATFILL` | 4 | `i32 colour_ref` |
| 190/191/192 `TAG_FLATFILL_{NONE,BLACK,WHITE}` | 0 | atajos (`Kernel/fillattr.cpp:5941-5960`) |
| 153 `TAG_LINEARFILL` | 40 | `Coord start`, `Coord end`, `i32 col_start`, `i32 col_end`, `f64 bias`, `f64 gain` (`Kernel/fillattr.cpp:6805-6815`) |
| 4121 `TAG_LINEARFILL3POINT` | 48 | igual + `Coord end2` tras `end` |
| 154 `TAG_CIRCULARFILL` | 40 | como 153 (`Kernel/fillattr.cpp:8066-8080`) |
| 155 `TAG_ELLIPTICALFILL` | 48 | `start`, `end`, `end2`, `col_start`, `col_end`, `bias`, `gain` |
| 156 `TAG_CONICALFILL` | 40 | como 153 (`Kernel/fillattr.cpp:9502-9512`) |
| 200 `TAG_SQUAREFILL` | 48 | como 155 (`Kernel/fillattr.cpp:10622-10632`) |
| 202 `TAG_THREECOLFILL` | 36 | `start`, `end`, `end2`, `col_start`, `col_end`, `col_end2` (**sin perfil**) |
| 204 `TAG_FOURCOLFILL` | 40 | `start`, `end`, `end2`, `col_start`, `col_end`, `col_end2`, `col_end3` |
| 157 `TAG_BITMAPFILL` | 44 | `start`, `end`, `end2`, `i32 bitmap_ref`, `f64 bias`, `f64 gain` |
| 158 `TAG_CONTONEBITMAPFILL` | 52 | `start`, `end`, `end2`, `i32 col_start`, `i32 col_end`, `i32 bitmap_ref`, `bias`, `gain` (`Kernel/fillattr.cpp:15290-15306`) |
| 159 `TAG_FRACTALFILL` | 69 | `start`, `end`, `end2`, `col_start`, `col_end`, `i32 seed`, `FIXED16 graininess`, `FIXED16 gravity`, `FIXED16 squash`, `i32 dpi`, `u8 tileable`, `bias`, `gain` (`Kernel/fillattr.cpp:16929-16946`) |
| 4010 `TAG_NOISEFILL` | 61 | `start`, `end`, `end2`, `col_start`, `col_end`, `FIXED16 graininess`, `i32 seed`, `i32 dpi`, `u8 tileable`, `bias`, `gain` (`Kernel/fillattr.cpp:17143-17157`) |

**Degradados multietapa** (4075–4078, 4088, 4122) — `Kernel/fillattr.cpp:6827-6845`:
mismo prefijo geométrico y los dos colores extremos, y a continuación

```
u32 num_ramp_items
por item: f64 position (0..1), i32 colour_ref
```

Ejemplo real: `TAG_LINEARFILLMULTISTAGE` de 88 bytes = 16 (2 coords) + 8 (2 refs)
+ 4 (contador) + 5 × 12 (rampa de 5 paradas).
⚠️ **Los multietapa no llevan bias/gain**, a diferencia de sus equivalentes de 2 colores.

**Mapeo / repetición del relleno** (records vacíos que modifican el relleno anterior,
`Kernel/fillattr.cpp:18456-18485`):

| tag | `Repeat` interno | significado |
|---|---|---|
| 164 `TAG_FILL_NONREPEATING` | 0/1 | no repetir |
| 163 `TAG_FILL_REPEATING` | 2 | repetir |
| 165 `TAG_FILL_REPEATINGINVERTED` | 3 | repetir invertido (espejo) |
| 206 `TAG_FILL_REPEATING_EXTRA` | 4 | repetición "extra" |

**Efecto de interpolación de color** (records vacíos): 160 `FILLEFFECT_FADE` (RGB lineal),
161 `FILLEFFECT_RAINBOW` (HSV camino corto), 162 `FILLEFFECT_ALTRAINBOW` (HSV camino largo).

### 8.4. Transparencias

Misma geometría que sus homólogos de color, pero con niveles de transparencia de 1 byte
(0 = opaco … 255 = totalmente transparente) en lugar de referencias de color:

| tag | tam. | payload |
|---|---:|---|
| 166 `TAG_FLATTRANSPARENTFILL` | 2 | `u8 transp`, `u8 type` |
| 167 `TAG_LINEARTRANSPARENTFILL` | 35 | `start`, `end`, `u8 t0`, `u8 t1`, `u8 type`, `f64 bias`, `f64 gain` |
| 4123 `TAG_LINEARTRANSPARENTFILL3POINT` | 43 | igual + `end2` |
| 168 `TAG_CIRCULARTRANSPARENTFILL` | 35 | como 167 |
| 169 `TAG_ELLIPTICALTRANSPARENTFILL` | 43 | `start`, `end`, `end2`, `t0`, `t1`, `type`, `bias`, `gain` |
| 170 `TAG_CONICALTRANSPARENTFILL` | 35 | como 167 |
| 201 `TAG_SQUARETRANSPARENTFILL` | 43 | como 169 |
| 203 `TAG_THREECOLTRANSPARENTFILL` | 28 | `start`, `end`, `end2`, `t0`, `t1`, `t2`, `type` |
| 205 `TAG_FOURCOLTRANSPARENTFILL` | 29 | `…`, `t0`, `t1`, `t2`, `t3`, `type` |
| 171 `TAG_BITMAPTRANSPARENTFILL` | 47 | `start`, `end`, `end2`, `t0`, `t1`, `type`, `i32 bitmap_ref`, `bias`, `gain` |
| 172 `TAG_FRACTALTRANSPARENTFILL` | 64 | como fractal pero con `t0,t1,type` en vez de colores |
| 4011 `TAG_NOISETRANSPARENTFILL` | 56 | como ruido con `t0,t1,type` |
| 180/181/182/207 | 0 | mapeo repetir / no repetir / repetir invertido / extra |

**Tipos de transparencia** (`Kernel/cxfdefs.h:121-131`) — el byte `type`:

| valor | modo |
|---:|---|
| 0 | none (opaco) |
| 1 | mix (normal / alpha) |
| 2 | stained glass (multiplicar) |
| 3 | bleach (screen) |
| 13 | contrast |
| 16 | saturation |
| 19 | darken |
| 22 | lighten |
| 25 | brightness |
| 28 | luminosity |

Los huecos (4–12, 14–15, …) corresponden a variantes internas del motor de render que no
se documentan en el código; **tratar valores desconocidos como `mix`**.

### 8.5. Guiones (dashes)

* **`TAG_DASHSTYLE` (183), 4 bytes**: `i32 dash_ref`. Si es negativo, es uno de los
  patrones predefinidos (−1..−20, −21 sólido, −22 capa de guías,
  `Kernel/cxfdash.h:104-126`, `Kernel/lineattr.cpp:3830-3862`).
* **`TAG_DEFINEDASH` (184) / `TAG_DEFINEDASH_SCALED` (188)**, variable
  (`Kernel/lineattr.cpp:3868-3902`) — **no es una definición referenciable**: sustituye a
  `TAG_DASHSTYLE` y define el patrón *in situ*:

| Tipo | Campo |
|---|---|
| `i32` | `dash_start` (offset inicial, millipoints) |
| `i32` | `line_width` de referencia |
| `i32` | `num_elements` |
| `i32[num_elements]` | longitudes alternas trazo/hueco (millipoints) |

La variante `_SCALED` indica que el patrón escala con el grosor de línea.

### 8.6. Flechas

**`TAG_ARROWHEAD` (185)** y **`TAG_ARROWTAIL` (186)**, 12 bytes cada uno
(`Kernel/lineattr.cpp:2203-2245`):

| Off | Tipo | Campo |
|---|---|---|
| 0 | `i32` | `arrow_ref` — negativa = flecha predefinida (§6.3) |
| 4 | `FIXED16` | escala en anchura |
| 8 | `FIXED16` | escala en altura |

### 8.7. Atributos misceláneos

| tag | payload |
|---|---|
| 179 `TAG_QUALITY` | `i32` |
| 189 `TAG_USERVALUE` | `UNICODE-Z clave`, `UNICODE-Z valor`. Si la clave es la cadena de recurso `IDS_USERATTRKEY_WEBADDRESS`, es un hiperenlace antiguo (`Kernel/rechattr.cpp:240-290`) |
| 4086 `TAG_FEATHER` | `i32 size`, `f64 bias`, `f64 gain` |
| 3500–3505 | atributos de sobreimpresión, sin payload |

---

## 9. Colores

### 9.1. Modelos de color

`Kernel/colmodel.h:199-215` (el byte `model` de `TAG_DEFINECOMPLEXCOLOUR`):

| valor | modelo | componentes (4 × FIXED24) |
|---:|---|---|
| 0 | `COLOURMODEL_INDEXED` | (uso interno, no se guarda) |
| 1 | `COLOURMODEL_CIET` | X, Y, Z, transparencia |
| 2 | `COLOURMODEL_RGBT` | R, G, B, transparencia |
| 3 | `COLOURMODEL_CMYK` | C, M, Y, K |
| 4 | `COLOURMODEL_HSVT` | H, S, V, transparencia |
| 5 | `COLOURMODEL_GREYT` | intensidad, 0, 0, 0 |
| 6 | `COLOURMODEL_WEBRGBT` | igual que RGBT, restringido a la paleta web |

Todas las componentes son **FIXED24 normalizadas a 0.0–1.0** (`raw / 2^24`), salvo el
matiz (H) de HSV, que también va normalizado 0..1 (equivale a 0–360°).
El valor especial `FIXED24(-8.0)` = `0xF8000000` marca una componente **heredada** del
color padre en colores enlazados (`Kernel/colcomp.cpp:2044-2058`).

### 9.2. `TAG_DEFINECOMPLEXCOLOUR` (51)

Escritura `Kernel/colcomp.cpp:1840-2010`, lectura `Kernel/colcomp.cpp` +
descripción en `Kernel/rechcol.cpp:259-310`.

| Off | Tipo | Campo |
|---:|---|---|
| 0 | `u8` | `r` — aproximación RGB de 8 bits (para render rápido y para lectores simples) |
| 1 | `u8` | `g` |
| 2 | `u8` | `b` |
| 3 | `u8` | `colour_model` (tabla §9.1) |
| 4 | `u8` | `colour_type` (tabla §9.3) |
| 5 | `u32` | `entry_index` — posición en la lista de colores del documento (0 si no está en la línea de color) |
| 9 | `i32` | `parent_ref` — **número de record** del color padre (0 = ninguno) |
| 13 | `FIXED24` | componente 1 |
| 17 | `FIXED24` | componente 2 |
| 21 | `FIXED24` | componente 3 |
| 25 | `FIXED24` | componente 4 |
| 29 | `UNICODE-Z` | nombre del color (puede ser cadena vacía → 2 bytes) |

Tamaño = 29 + 2·(len+1). Observado: 31 (sin nombre) hasta 65.

### 9.3. Tipos de color y colores derivados

`Kernel/colcomp.h:155-159`:

| valor | tipo | semántica de las componentes |
|---:|---|---|
| 0 | `NORMAL` | color independiente |
| 1 | `SPOT` | tinta plana (separación propia) |
| 2 | `TINT` | **tinta** del color `parent_ref`: comp1 = factor de tinte (0..1) |
| 3 | `LINKED` | color enlazado al padre; las componentes con valor `0xF8000000` se heredan del padre, el resto lo sobrescriben |
| 4 | `SHADE` | **sombra** del padre: comp1 = valor de sombra X, comp2 = valor Y (`Kernel/colcomp.cpp:1968-1983`) |

Ejemplos reales extraídos de `testfiles/OneLine.xar`:

```
record #31: rgb=(0,0,0)     model=3 (CMYK) type=0 entry=24 parent=0  comps=[0,0,0,1]      name="Black"
record #45: rgb=(255,0,0)   model=4 (HSVT) type=0 entry=0  parent=0  comps=[0,1,1,0]      name="Red"
record #69: rgb=(25,25,25)  model=3 (CMYK) type=2 entry=25 parent=31 comps=[0.9,0,0,0]    name="90% Black"
                                                       ^tinta del 90 % del record 31 (Black)
```

### 9.4. Estrategia de implementación recomendada

Para un importador que solo necesita pintar:

1. Mantener `HashMap<u32 /*record*/, ColourDef>`.
2. Al resolver una referencia:
   * `< 0` → color predefinido de la tabla de §6.3;
   * `0` → sin color;
   * `> 0` → buscar en el mapa; si no está (fichero corrupto) → negro.
3. Para obtener RGB **basta con los 3 primeros bytes del record** (`r`,`g`,`b`), que Xara
   ya calcula. Es exactamente lo que hace el importador simplificado de Xara para
   previsualización. Usar el modelo completo solo si se necesita fidelidad CMYK,
   separaciones o edición.
4. Resolución de tintas/sombras/enlaces: requiere recorrer la cadena `parent_ref`. Como el
   padre **siempre se escribe antes** que el hijo (`Kernel/colcomp.cpp:1900-1910` lo
   garantiza con un `ERROR2IF`), un mapa incremental de una sola pasada basta.

### 9.5. Perfiles de color

**El formato no almacena perfiles ICC.** No hay ningún tag de perfil en `cxftags.h`.
Lo más cercano es:

* La información de separación de color por plancha (`TAG_COLOURPLATE`, 3508), con tipo de
  plancha, color spot asociado, ángulo y frecuencia de trama.
* El modelo por color (CMYK vs RGB), que determina el espacio nominal.

La conversión CMYK→RGB del código original es la trivial
(`R = 1−min(1, C+K)`, etc., vía `ColourContext`), sin gestión de color.

---

## 10. Prioridades de implementación

### 10.1. Metodología

Se analizaron **59 ficheros `.xar`** del repositorio original (todos los de `testfiles/`,
`Designs/`, `Templates/` y `TextDesigns/`), con un total de **1 390 282 records** y **157
tags distintos** (de los 300 definidos). El script está en §12.1 y los resultados
completos en §12.2.

### 10.2. Nivel 0 — imprescindible (sin esto no se lee nada)

```
magic + TAG_FILEHEADER(2) + TAG_ENDOFFILE(3)
TAG_STARTCOMPRESSION(30) + TAG_ENDCOMPRESSION(31)      <- deflate crudo
TAG_UP(0) + TAG_DOWN(1)                                <- 28.9 % de todos los records
```

### 10.3. Nivel 1 — el 95 % de los records reales

Los **16 tags** siguientes suman el **94.96 %** de todos los records del corpus, y con el
17.º (`TAG_GROUP`) se alcanza el 96.33 %:

| # | tag | nombre | % | acumulado | ficheros |
|---:|---:|---|---:|---:|---:|
| 1 | 1 | `TAG_DOWN` | 14.47 | 14.47 | 59/59 |
| 2 | 0 | `TAG_UP` | 14.47 | 28.93 | 59/59 |
| 3 | 111 | `TAG_PATH_FLAGS` | 10.96 | 39.89 | 35 |
| 4 | 152 | `TAG_LINEWIDTH` | 9.94 | 49.83 | 59/59 |
| 5 | 116 | `TAG_PATH_RELATIVE_FILLED_STROKED` | 9.73 | 59.55 | 28 |
| 6 | 193 | `TAG_LINECOLOUR_NONE` | 5.32 | 64.88 | 55 |
| 7 | 150 | `TAG_FLATFILL` | 4.97 | 69.84 | 57 |
| 8 | 176 | `TAG_JOINSTYLE` | 4.74 | 74.59 | 25 |
| 9 | 151 | `TAG_LINECOLOUR` | 4.55 | 79.14 | 49 |
| 10 | 153 | `TAG_LINEARFILL` | 4.02 | 83.16 | 18 |
| 11 | 1901 | `TAG_REGULAR_SHAPE_PHASE_2` | 2.53 | 85.69 | 22 |
| 12 | 155 | `TAG_ELLIPTICALFILL` | 2.10 | 87.79 | 13 |
| 13 | 174 | `TAG_STARTCAP` | 1.89 | 89.68 | 7 |
| 14 | 175 | `TAG_ENDCAP` | 1.89 | 91.56 | 7 |
| 15 | 173 | `TAG_LINETRANSPARENCY` | 1.74 | 93.31 | 21 |
| 16 | 166 | `TAG_FLATTRANSPARENTFILL` | 1.65 | 94.96 | 55 |
| 17 | 104 | `TAG_GROUP` | 1.37 | 96.33 | 23 |
| 18 | 115 | `TAG_PATH_RELATIVE_STROKED` | 1.21 | 97.55 | 25 |
| 19 | 167 | `TAG_LINEARTRANSPARENTFILL` | 0.85 | 98.39 | 15 |
| 20 | 51 | `TAG_DEFINECOMPLEXCOLOUR` | 0.41 | 98.81 | **59/59** |
| 21 | 169 | `TAG_ELLIPTICALTRANSPARENTFILL` | 0.38 | 99.19 | 11 |

**Pero “95 % de los records” no equivale a “95 % de los ficheros bien leídos”.** Hay tags
poco frecuentes que están en **todos** los ficheros y son estructurales. El **conjunto
mínimo real** para abrir cualquier `.xar` del corpus y renderizarlo de forma reconocible
es la unión de los anteriores con los que aparecen en 59/59 ficheros:

```
Estructura:   0,1,2,3,10,30,31,40,41,42,43,45,46,47,48,80,82,87,91,92,93,4070,4087,4114,4116,4031
Color:        51  (+ referencias negativas predefinidas)
Geometría:    111,115,116,1901,104
Atributos:    150,151,152,153,155,166,167,169,173,174,175,176,193
```

Es decir, **~45 tags** cubren el 99.2 % de los records y el 100 % de los 59 ficheros.

### 10.4. Nivel 2 — necesario para documentos "de verdad"

Añadir, por orden de rentabilidad:

1. **Bitmaps**: 68/67/71/65/66 (definición), 198 (objeto), 157/158 (rellenos), 4115.
   Presentes en 22–24 ficheros del corpus. Basta con volcar el PNG/JPEG embebido.
2. **Texto**: 2000, 2100/2101, 2200, 2201, 2202, 2203, 2206, 2150, 2151, 2906, 2907,
   2900, 2919, 2920, 2902-2905, 2908-2917, 2918. 22 ficheros del corpus.
3. **Degradados restantes**: 154, 156, 200, 202, 204, 159, 4010 y sus transparencias;
   mapeos 160-165, 180-182, 206, 207.
4. **Grupos compuestos**: 105/106/4072/4073 (blends), 107-110/4012 (moldes),
   4050/4051 (sombras), 4052/4057 (biseles), 4066/4067 (contornos), 4086 (feather),
   4128 (compound render).
5. **Guiones y flechas**: 183, 184, 188, 185, 186.

### 10.5. Nivel 3 — raro o legacy (implementar solo si hace falta)

* **Nunca observado en el corpus pero definido y con código**: caminos absolutos
  (100-103), caminos relativos 113/114, `TAG_DEFINERGBCOLOUR` (50), formas regulares
  legacy (1000-1217, 1900), `TAG_PAGE` (44), `TAG_GUIDELINE` (112),
  `TAG_PATHREF_*` (118, 4013), `TAG_DEFINEDASH` (184), `TAG_TEXT_TAB`/`RULER`/indents
  (4200-4204), `TAG_DOCUMENTCOMMENT` (90), `TAG_VIEWQUALITY` (81),
  `TAG_CLIPVIEW*` (4084/4085), `TAG_LIVE_EFFECT` (4125-4127), `TAG_OBJECTBOUNDS` (4129),
  XPE (4117/4118).
* **Declarado sin implementación** (§4.11): 21 tags. Tratarlos como desconocidos.
* **Impresión/filmación** (3500-3510): irrelevante para renderizar; saltar.
* **Pinceles y trazos** (4000-4003, 4079-4083, 4102-4113): muy complejos y presentes en
  1 solo fichero del corpus; el resultado visual se puede aproximar ignorándolos (el
  camino base sigue estando en el fichero).

### 10.6. Orden de trabajo sugerido

```
1. Lector físico: magic, records, deflate + CRC          -> valida los 59 ficheros
2. Árbol DOWN/UP + volcado de tags                       -> herramienta `xar-dump`
3. Colores (51) + refs negativas
4. Caminos relativos (115/116) + PATH_FLAGS + verbos
5. Atributos de línea y relleno plano                    -> ya se renderiza algo útil
6. Degradados lineal/elíptico + transparencias planas
7. Formas regulares 1901 (elipses y rectángulos)
8. Grupos y capas (herencia de atributos)
9. Bitmaps
10. Texto
11. Efectos compuestos
```

---

## 11. Riesgos y ambigüedades conocidas

1. **Tamaño de record vs. versión.** Varios records han crecido con el tiempo
   (`TAG_LINEARFILL` 24→40, `TAG_TEXT_STORY_SIMPLE` 8→12, `TAG_SHADOW` 16→24,
   `TAG_BLENDERADDITIONAL` 16→17, `TAG_FEATHER` 4→20…). **Nunca asumir el tamaño
   declarado**: leer campos mientras queden bytes y aplicar valores por defecto
   (bias=0, gain=0, autokern=0…). El código original usa lecturas tolerantes
   (`ReadINT32noError`, `Kernel/cxfrec.cpp:1290`).

2. **Discrepancias entre `cxfdefs.h` y los escritores reales.**
   * `TAG_VARIABLEWIDTHFUNC_SIZE = 4` pero el escritor emite 8 bytes
     (`Kernel/strkattr.cpp:1343-1347`).
   * `TAG_BEVEL` no tiene constante de tamaño; el escritor usa 24 codificado a mano
     (`Kernel/nbevcont.cpp:507`).
   * `TAG_DOCUMENTNUDGE` usa la constante `TAG_DOCUMENTNUDGESIZE` (sin guion bajo).
   → **Fuente de verdad = el escritor**, no la constante.

3. **Bit de doble página en `TAG_SPREADINFORMATION`**: el handler de importación usa el
   bit 0 y el código de depuración el bit 2 (§4.3). Se recomienda seguir el handler.

4. **Caminos relativos sin contador**: si `size % 9 != 0` el record está corrupto o es de
   una variante desconocida. Abortar la lectura de ese camino, no adivinar.

5. **Signo del delta en caminos relativos**: es `P[i] = P[i-1] − D`. Un error de signo
   produce geometría reflejada y es difícil de detectar en formas simétricas.

6. **`RELPATHINTERLEAVE`**: el código contiene dos formatos alternativos para los caminos
   relativos (intercalado y no intercalado, `Kernel/cxfrec.cpp:1763,1846`). En Xara LX el
   macro está **definido**, así que el formato real es el intercalado. Ficheros escritos
   por una hipotética versión con el macro desactivado (verbos agrupados al principio +
   coordenadas normales) **no serían legibles** con el lector intercalado, y **no hay
   forma de distinguirlos por el tag**. No se ha encontrado ningún fichero así en el
   corpus; riesgo teórico.

7. **Tags de Xara posteriores (Xtreme Pro, Designer Pro).** `TAG_SPREAD_PHASE2` (4131),
   `TAG_CURRENTATTRIBUTES_PHASE2` (4132) y todo el rango >4138 pueden aparecer en ficheros
   creados por versiones comerciales más recientes. El mecanismo `TAG_ATOMICTAGS` /
   `TAG_ESSENTIALTAGS` está diseñado precisamente para eso: **implementarlo desde el
   principio** o se corromperán los árboles al leer ficheros modernos.

8. **Precisión FIXED16 en matrices.** Escalas y rotaciones tienen 16 bits fraccionarios.
   Al componer varias transformaciones el error se acumula. Conviene trabajar en `f64`
   internamente y no re-cuantizar.

9. **Componentes de color heredadas.** El valor `0xF8000000` (FIXED24 −8.0) no es un color
   válido: significa "heredar del padre". Si se interpreta literalmente se obtienen
   colores absurdos.

10. **Records streamed y compresión.** Si se implementa el escritor, hay que respetar el
    cierre/reapertura del bloque comprimido alrededor de cada bitmap; si no, Xara original
    no podrá leer el fichero. Para el **lector** no importa.

11. **El CRC solo cubre el bloque comprimido.** No hay checksum del fichero completo ni de
    los records no comprimidos (cabecera, preview, bitmaps). Un `.xar` truncado dentro de
    un bitmap no se detecta hasta que falla el siguiente record.

12. **`TAG_ENDOFFILE` puede no ser el último byte.** El lector debe parar ahí y no asumir
    que `pos == len`. (En el corpus siempre coincide: 0 bytes de cola en los 59 ficheros.)

13. **Nodos atómicos con hijos derivados.** Si se ignora `TAG_SHADOWCONTROLLER` pero se
    insertan sus hijos (`TAG_SHADOW` + el objeto duplicado), se pintará la sombra como si
    fuese un objeto independiente. Hay que respetar la semántica atómica.

14. **Unidades de ángulo inconsistentes.** `ANGLE` es FIXED16 en radianes, pero
    `TAG_SHADOWCONTROLLER` guarda su ángulo como `i32` con una codificación propia
    (`FloorAngleToINT32`, `Kernel/nodecont.cpp:1633`) y `TAG_BEVEL` usa grados enteros.
    Revisar caso por caso.

15. **Unidad de `TAG_TEXT_TRACKING`.** El tipo C++ es `MILLIPOINT`
    (`Kernel/txtattr.h:413`) pero el valor se combina con el cuerpo de la fuente al
    renderizar; conviene calibrar contra un render de referencia antes de fijar la
    conversión.

16. **Cadenas**: `TAG_TEXT_STRING` no lleva terminador; el resto sí. Mezclar ambos
    criterios provoca desincronización dentro del record.

---

## 12. Apéndices

### 12.1. Script de validación (parser mínimo en Python)

Ubicación: `scratchpad/xarparse.py` + `scratchpad/dump.py`
(directorio de trabajo de la sesión). Núcleo del parser:

```python
import struct, zlib

MAGIC = b'XARA\xa3\xa3\x0d\x0a'

def records(path):
    data = open(path, 'rb').read()
    assert data[:8] == MAGIC
    pos, dec, buf, bpos, crc, total = 8, None, b'', 0, 0, 0
    def read(n):
        nonlocal pos, buf, bpos, crc, total
        if dec is None:
            d = data[pos:pos+n]; pos += len(d); return d
        while len(buf) - bpos < n and not dec.eof:
            chunk = data[pos:pos+4096]; pos += len(chunk)
            buf += dec.decompress(dec.unconsumed_tail + chunk)
            if not chunk: break
        d = buf[bpos:bpos+n]; bpos += len(d)
        crc = zlib.crc32(d, crc); total += len(d)
        return d
    while True:
        h = read(8)
        if len(h) < 8: return
        tag, size = struct.unpack('<II', h)
        if tag == 30:                       # STARTCOMPRESSION
            ver = struct.unpack('<I', read(size))[0]
            dec = zlib.decompressobj(-15)   # deflate CRUDO
            buf, bpos, crc, total = b'', 0, 0, 0
            yield tag, size, b''; continue
        if tag == 31:                       # ENDCOMPRESSION
            crc_calc, len_calc = crc & 0xffffffff, total
            unused = dec.unused_data; dec = None
            pos -= (len(unused) - 8)        # el trailer va SIN comprimir
            crc_f, len_f = struct.unpack('<II', data[pos-8:pos])
            assert (crc_f, len_f) == (crc_calc, len_calc)
            yield tag, size, b''; continue
        payload = read(size) if size else b''
        yield tag, size, payload
        if tag == 3: return                 # ENDOFFILE
```

Resultado sobre los 59 ficheros: **0 errores**, CRC y longitud correctos en los 103
bloques comprimidos, 0 bytes de cola sobrante en todos los ficheros.

### 12.2. Resultados del análisis del corpus

**Ficheros analizados** (59): `testfiles/*.xar` (18), `Designs/*.xar` (19),
`Templates/*.xar` (8), `TextDesigns/*.xar` (14).

* Todos son de tipo **`CXN`** (nativo).
* Todos llevan **compresión** con `compression_version = 99` (zlib 0.99, tipo 0 = deflate).
* Todos tienen preview GIF salvo uno.
* Productores observados (campo `producer` / `producer_version` / `producer_build`):

| veces | producer | versión | build |
|---:|---|---|---|
| 17 | `Xara Xtreme` | 3.0 | `0.4366 (Xara)` |
| 14 | `Xara Xtreme` | 3.0 | `0.4480 (Xara)` |
| 8 | `Xara Xtreme` | 3.0 | `0.4224 (Gerry)` |
| 6 | `Xara X` | 3.0 | `0.2704 (MarkG)` |
| 6 | `Xara Xtreme` | 3.0 | `0.4308 (SimonM)` |
| 3 | `Xara Xtreme` | 3.0 | `0.3993 (Gavin)` |
| 3 | `Xara Xtreme` | 3.0 | `0.4372 (Xara)` |
| 1 | `Xara Xtreme` | 3.0 | `0.4293 (SimonM)` |
| 1 | `X` | *(vacío)* | *(vacío)* |

  ⚠️ El último caso (`Templates/animation.xar`, cabecera de solo 36 bytes) demuestra que
  las tres cadenas pueden estar vacías o truncadas: **el lector no debe exigirlas**.
* Profundidad máxima de árbol: 4 (plantillas) … 13 (`ProbeX16.xar`).
* Total: 1 390 282 records, 157 tags distintos.

**Tamaños de payload observados vs. declarados**: coinciden en el 100 % de los tags de
tamaño fijo. Muestra:

| tag | declarado | observado |
|---|---|---|
| 45 `SPREADINFORMATION` | 17 | 17 |
| 51 `DEFINECOMPLEXCOLOUR` | 29 + nombre | 31, 37, 41, 43, 49, 51, 53, 55, 57, 63, 65 |
| 105 `BLEND` | 3 | 3 |
| 106 `BLENDER` | 8 | 8 |
| 153 `LINEARFILL` | 40 | 40 |
| 155 `ELLIPTICALFILL` | 48 | 48 |
| 159 `FRACTALFILL` | 69 | 69 |
| 166 `FLATTRANSPARENTFILL` | 2 | 2 |
| 167 `LINEARTRANSPARENTFILL` | 35 | 35 |
| 1901 `REGULAR_SHAPE_PHASE_2` | var | 119 (35 178×), 155 (28×) |
| 2000 `FONT_DEF_TRUETYPE` | var | 34, 36, 54, 74, 78 |
| 4050 `SHADOWCONTROLLER` | 29 | 29 |
| 4051 `SHADOW` | 24 | 24 |
| 4052 `BEVEL` | (24, no declarado) | 24 |
| 4073 `BLENDERADDITIONAL` | 17 | 17 |
| 4086 `FEATHER` | 20 | 20 |
| 4115 `BITMAP_PROPERTIES` | 12 | 12 |

**Ejemplo completo — `testfiles/OneLine.xar`** (2 261 bytes, 88 records, profundidad 5):

```
#1   TAG_FILEHEADER              41   CXN / 3996 / "Xara X" "3.0" "0.2704 (MarkG)"
#2   TAG_PREVIEWBITMAP_GIF     1196   GIF87a...
#3   TAG_DOCUMENT                 0
#4   TAG_DOWN                     0
#5   TAG_DOCUMENTNUDGE            4
#6   TAG_DOCUMENTBITMAPSMOOTHING  5
#7   TAG_STARTCOMPRESSION         4   version=99  -> a partir de aquí, deflate
#8   TAG_VIEWPORT                16
#9..17 TAG_ATOMICTAGS             4   (9 records, un tag cada uno)
#18  TAG_CHAPTER                  0
#19  TAG_DOWN / #20 TAG_SPREAD / #21 TAG_DOWN
#22  TAG_SPREADINFORMATION       17   600000 x 450000 mp, margen 576000, bleed 0, flags 2
#23  TAG_SPREADSCALING_INACTIVE  24
#24  TAG_SPREAD_ANIMPROPS        28
#25  TAG_LAYER 0 / #26 TAG_DOWN
#27  TAG_LAYERDETAILS            17   flags=0x0D, "Layer 1"
#28  TAG_PATH_RELATIVE_STROKED   18   MoveTo(112101,178899) LineTo(283101,321399)
#29  TAG_DOWN
#30  TAG_PATH_FLAGS               2   0x05, 0x05  (smooth|endpoint)
#31  TAG_DEFINECOMPLEXCOLOUR     41   CMYK "Black"
#32  TAG_LINECOLOUR               4   -> record 31
#33  TAG_LINEWIDTH                4   500 mp
#34  TAG_FLATFILL                 4   -> record 31
#35  TAG_UP / #36 TAG_UP
#37  TAG_GRIDRULERSETTINGS       17
#38  TAG_GRIDRULERORIGIN          8
#39  TAG_UP
#40  TAG_SETSENTINEL / TAG_BARPROPERTY / unidades / fechas / flags / impresión / vista
#87  TAG_ENDCOMPRESSION           8   CRC32 + tamaño descomprimido (verificados OK)
#88  TAG_ENDOFFILE                0
```

### 12.3. Índice de ficheros fuente C++ relevantes

| Fichero | Contenido |
|---|---|
| `Kernel/cxftags.h` | **Lista completa de tags** (300) |
| `Kernel/cxfdefs.h` | Magic, tamaños de record, enums de cap/join/winding/transparencia, flags de capa/camino/blend |
| `Kernel/cxfile.h/.cpp` | Clase `CXaraFile`: apertura, magic, lectura/escritura de records, compresión, despacho a handlers |
| `Kernel/cxfrec.h/.cpp` | Clase `CXaraFileRecord`: **serialización de todos los tipos primitivos**, coords, paths, matrices, strings |
| `Kernel/cxfrech.h/.cpp` | Clase base de los *record handlers* + tabla tag→nombre legible |
| `Kernel/cxfmap.cpp` | Mapa tag → handler |
| `Kernel/cxftree.h/.cpp` | Diálogo de depuración del árbol de records (clasificación de tags por categoría) |
| `Kernel/camfiltr.h/.cpp` | `BaseCamelotFilter`: cabecera, árbol de inserción, tags atómicos/esenciales, origen de coordenadas |
| `Kernel/native.cpp`, `Kernel/webfiltr.cpp` | Filtros `CXN` / `CXW` / `CXM` |
| `Kernel/zstream.cpp`, `Kernel/ccfile.cpp` | Capa zlib: deflate crudo, CRC, trailer |
| `Kernel/cxfcols.h`, `cxfarrow.h`, `cxfdash.h`, `cxfunits.h` | Tablas de referencias negativas predefinidas |
| `Kernel/colcomp.cpp`, `rechcol.cpp`, `colmodel.h` | Colores |
| `Kernel/nodepath.cpp`, `GDraw/gconsts.h` | Caminos y verbos |
| `Kernel/cxfellp/cxfrect/cxfpoly/cxfrgshp.cpp`, `rech*.cpp` | Formas regulares |
| `Kernel/fillattr.cpp` (23 k líneas) | **Todos** los rellenos y transparencias |
| `Kernel/lineattr.cpp` | Atributos de línea, guiones, flechas |
| `Kernel/rechtext.cpp`, `cxftext.cpp`, `nodetxts.cpp`, `fontcomp.cpp` | Texto y fuentes |
| `Kernel/bmpcomp.cpp`, `rechbmp.cpp`, `cxfnbmp.cpp` | Bitmaps |
| `Kernel/rechdoc.cpp`, `spread.cpp`, `layer.cpp`, `viewcomp.cpp`, `unitcomp.cpp`, `infocomp.cpp` | Documento, spreads, capas, vistas, unidades |
| `Kernel/nodeblnd.cpp`, `nodebldr.cpp`, `nodemold.cpp`, `nodeshad.cpp`, `nodecont.cpp`, `nbevcont.cpp`, `ncntrcnt.cpp`, `nodeclip.cpp`, `fthrattr.cpp`, `nodeliveeffect.cpp` | Efectos y nodos compuestos |
| `Kernel/princomp.cpp`, `prnmkcom.cpp`, `isetattr.cpp` | Impresión y filmación |
| `Kernel/cxftfile.h/.cpp` | Formato de **texto** para plantillas Flare (fuera de alcance) |

### 12.4. Esqueleto de importador en Rust

```rust
//! Lector de ficheros .xar (CXF). Todo little-endian.
//! Dependencias sugeridas: flate2 (inflate raw), thiserror.

pub const TAG_UP: u32 = 0;
pub const TAG_DOWN: u32 = 1;
pub const TAG_FILEHEADER: u32 = 2;
pub const TAG_ENDOFFILE: u32 = 3;
pub const TAG_ATOMICTAGS: u32 = 10;
pub const TAG_ESSENTIALTAGS: u32 = 11;
pub const TAG_STARTCOMPRESSION: u32 = 30;
pub const TAG_ENDCOMPRESSION: u32 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind { Native, Web, MinimalWeb }   // CXN / CXW / CXM

#[derive(Debug, Clone)]
pub struct FileHeader {
    pub kind: FileKind,
    pub uncompressed_size: u32,
    pub link_id: u32,
    pub precompression: u32,     // debe ser 0
    pub producer: String,
    pub producer_version: String,
    pub producer_build: String,
}

pub fn parse_file_header(data: &[u8]) -> Result<FileHeader, Err> {
    let mut c = Cur::new(data);
    let ty = c.take(3)?;
    let kind = match ty {
        b"CXN" => FileKind::Native,
        b"CXW" => FileKind::Web,
        b"CXM" => FileKind::MinimalWeb,
        _ => return Err(Err::BadFileType),
    };
    let uncompressed_size = c.u32()?;
    let link_id = c.u32()?;
    let precompression = c.u32()?;
    // Las cadenas pueden faltar en ficheros antiguos: tolerar EOF.
    let producer = c.ascii_z().unwrap_or_default();
    let producer_version = c.ascii_z().unwrap_or_default();
    let producer_build = c.ascii_z().unwrap_or_default();
    Ok(FileHeader { kind, uncompressed_size, link_id, precompression,
                    producer, producer_version, producer_build })
}

/// Estado global del importador durante una única pasada.
pub struct Importer {
    pub header: Option<FileHeader>,
    pub origin: Coord,                          // CoordOrigin (se fija en SPREADINFORMATION)
    pub colours: HashMap<u32, ColourDef>,       // record -> color
    pub bitmaps: HashMap<u32, BitmapDef>,
    pub fonts:   HashMap<u32, FontDef>,
    pub paths:   HashMap<u32, Vec<PathPoint>>,  // para TAG_PATHREF_*
    pub atomic:  HashSet<u32>,
    pub essential: HashSet<u32>,
    pub attr_stack: Vec<AttrCtx>,
    pub ctx: AttrCtx,
    skip_subtree_depth: Option<usize>,          // descarte de subárbol atómico
    depth: usize,
}

impl Importer {
    pub fn handle(&mut self, rec: &Record) -> Result<(), Err> {
        // 1) Gestión del descarte de subárboles atómicos desconocidos
        if let Some(d) = self.skip_subtree_depth {
            match rec.tag {
                TAG_DOWN => { self.depth += 1; return Ok(()); }
                TAG_UP => {
                    self.depth -= 1;
                    if self.depth <= d { self.skip_subtree_depth = None; }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }

        let mut c = Cur::new(&rec.data);
        match rec.tag {
            TAG_FILEHEADER => { self.header = Some(parse_file_header(&rec.data)?); }
            TAG_DOWN => { self.depth += 1; self.attr_stack.push(self.ctx.clone()); }
            TAG_UP   => { self.depth -= 1; if let Some(p) = self.attr_stack.pop() { self.ctx = p; } }
            TAG_ATOMICTAGS    => while c.remaining() >= 4 { self.atomic.insert(c.u32()?); },
            TAG_ESSENTIALTAGS => while c.remaining() >= 4 { self.essential.insert(c.u32()?); },

            45 => { /* SPREADINFORMATION: fija tamaño de página y CoordOrigin */ }
            51 => { self.colours.insert(rec.number, parse_complex_colour(&mut c)?); }
            111 => { /* PATH_FLAGS: aplicar al último camino */ }
            113..=116 => {
                let pts = read_path_relative(&mut c, self.origin)?;
                self.paths.insert(rec.number, pts.clone());
                self.emit_path(pts, rec.tag);
            }
            100..=103 => {
                let pts = read_path_absolute(&mut c, self.origin)?;
                self.paths.insert(rec.number, pts.clone());
                self.emit_path(pts, rec.tag);
            }
            150 => { self.ctx.fill = Some(Fill::Flat(Ref::parse(c.i32()?))); }
            190 => { self.ctx.fill = Some(Fill::Flat(Ref::Builtin(-1))); }   // none
            191 => { self.ctx.fill = Some(Fill::Flat(Ref::Builtin(-2))); }   // black
            192 => { self.ctx.fill = Some(Fill::Flat(Ref::Builtin(-3))); }   // white
            151 => { self.ctx.line_colour = Some(Ref::parse(c.i32()?)); }
            193 => { self.ctx.line_colour = Some(Ref::Builtin(-1)); }
            152 => { self.ctx.line_width = c.i32()?; }
            174 => { self.ctx.start_cap = CapStyle::from(c.u8()?); }
            176 => { self.ctx.join = JoinStyle::from(c.u8()?); }
            166 => { let t = c.u8()?; let k = c.u8()?; self.ctx.fill_transp = Some(Transparency::flat(t, k)); }
            // ... resto de tags ...
            TAG_ENDOFFILE => { /* fin */ }
            unknown => {
                if self.essential.contains(&unknown) { return Err(Err::UnsupportedEssential(unknown)); }
                if self.atomic.contains(&unknown) { self.skip_subtree_depth = Some(self.depth); }
                // en cualquier otro caso: simplemente se ignora el record
            }
        }
        Ok(())
    }
}
```

**Notas de diseño:**

* `#[repr(C, packed)]` **no** es aplicable a la mayoría de payloads (campos no alineados y
  cadenas de longitud variable). Solo tiene sentido para bloques homogéneos como los 4
  `FIXED24` de un color o los 4 `DocCoord` de `TAG_NODE_BITMAP`, y aun así hay que
  convertir el endianness explícitamente (`u32::from_le_bytes`), así que es preferible el
  lector campo a campo mostrado en §2.5.
* El importador debe ser **tolerante por defecto**: cualquier record que no se entienda se
  salta usando su `size`; solo los tags esenciales justifican abortar.
* Conviene exponer una API de bajo nivel (`Iterator<Item = Record>`) independiente del
  modelo de documento, para poder escribir herramientas de volcado y tests de round-trip.

---

*Fin del documento.*
