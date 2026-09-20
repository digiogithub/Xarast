# Licencia de Xarast y política de sala limpia

> Decisión tomada el 2026-09-20. Este documento es **normativo**: cualquier
> contribución (humana o de un agente) debe cumplirlo.

---

## 1. La decisión

**Xarast se publica bajo `MIT OR Apache-2.0`** (doble licencia a elección del
receptor), que es la convención del ecosistema Rust.

Encargo recibido: *"una licencia compatible con la original, la que dé más
libertad"*. `MIT OR Apache-2.0` cumple las dos condiciones a la vez, y es la
combinación que las maximiza.

---

## 2. Por qué esta y no otra

### 2.1 Qué es exactamente "la original"

Xara LX es **GPL-2.0-only** — no "or later". Lo dice literalmente su `LICENSE`:

> *"under the terms of the GNU General Public License version 2 as published by
> the Free Software Foundation"*

Añade una **excepción de enlazado** para wxWidgets, wxXtra y la librería binaria
CDraw, y reserva las marcas "Xara", "Xara LX", "Xara Xtreme".

Que sea `only` y no `or later` importa mucho, y es la clave de todo lo demás.

### 2.2 "Compatible" es una relación de un solo sentido

La compatibilidad entre licencias no es simétrica. Lo que significa aquí:

| Licencia de Xarast | ¿Se puede combinar nuestro código en un proyecto GPL-2.0-only? | ¿Podemos usar el ecosistema Rust (Apache-2.0)? |
|---|---|---|
| **MIT** | ✅ Sí | ✅ Sí |
| **Apache-2.0** | ❌ **No** (la cláusula de patentes es incompatible con GPL-2.0-only) | ✅ Sí |
| **MIT OR Apache-2.0** | ✅ Sí (por la rama MIT) | ✅ Sí |
| GPL-2.0-only | ✅ Sí | ❌ **No** (no podríamos usar crates Apache-2.0) |
| GPL-3.0-or-later | ❌ **No** (GPL-2-only y GPL-3 son incompatibles entre sí) | ✅ Sí |

Dos consecuencias que descartan las alternativas:

- **GPL-3.0-or-later** (lo que había propuesto inicialmente) es la opción
  *menos* compatible con el original: GPL-2.0-only y GPL-3.0 **no se pueden
  combinar**. Habría sido un error.
- **GPL-2.0-only** sería máxima compatibilidad pero mínima libertad, y además
  nos dejaría fuera de gran parte del ecosistema Rust, que es masivamente
  Apache-2.0.

`MIT OR Apache-2.0` da el resultado que se pedía: quien reciba Xarast puede
hacer prácticamente cualquier cosa con él —incluido incorporarlo a un proyecto
GPL-2.0-only por la rama MIT, o a uno propietario— y nosotros podemos usar
cualquier crate permisivo. La rama Apache-2.0 añade una **concesión explícita de
patentes** de la que MIT carece.

### 2.3 La condición que hace válida esta elección

Una licencia permisiva **solo es legítima si Xarast no es obra derivada** de
Xara LX. Si copiáramos o tradujéramos código GPL-2.0-only, estaríamos obligados
a GPL-2.0-only, y publicar bajo MIT sería una infracción.

Por eso la licencia y la sección 3 son inseparables: **la política de sala
limpia no es una buena práctica opcional, es el requisito que sostiene la
licencia.**

---

## 3. Política de sala limpia

### 3.1 Qué SÍ se puede tomar del original

Son **hechos**, no expresión creativa, y son necesarios para la
interoperabilidad:

- Números, nombres y semántica de los **tags** del formato `.xar`.
- **Layouts binarios**: orden de campos, tamaños, endianness, unidades.
- Nombres de campos y su significado, cuando describen una **interfaz** o un
  formato de datos.
- **Comportamiento observable**: qué hace una herramienta, qué produce un
  efecto, en qué orden se aplican los atributos.
- **Algoritmos descritos en prosa** o reformulados en pseudocódigo propio.
- **Referencias cruzadas** del tipo `Kernel/document.cpp:415` — citar dónde vive
  una lógica es legítimo y valiosísimo para verificar.

### 3.2 Qué NO se puede tomar

- Cuerpos de función copiados o traducidos línea a línea.
- Definiciones de clase con su sintaxis y orden.
- Comentarios del original.
- Tablas de constantes copiadas tal cual cuando no sean datos del formato.
- Recursos con copyright: iconos, mapas de bits, *clipart*, textos de la UI,
  ficheros de ayuda. **Los `.xar` de `testfiles/` y `Designs/` se usan solo
  localmente como corpus de validación; no se redistribuyen con Xarast.**
- Las **marcas** "Xara" y derivadas: ni en el nombre del producto, ni en la UI,
  ni en materiales, ni de forma que sugiera afiliación.

### 3.3 Regla operativa

> Se lee el original para **entender** y **documentar**. Se implementa desde la
> documentación (`docs/research/*`), no desde el código.

En la práctica: si estás escribiendo Rust con el fichero `.cpp` abierto al lado
copiando estructura, lo estás haciendo mal. Si estás escribiendo Rust desde
`docs/research/01-formato-xar.md`, lo estás haciendo bien.

Los documentos de `docs/research/` han pasado una **auditoría de higiene**
(`docs/research/00-informe-higiene.md`) que reescribió los fragmentos demasiado
próximos al original y conservó los hechos de formato como tablas y pseudocódigo.

### 3.4 Nombre y marcas

"Xarast" es un nombre nuevo. Aun así, para evitar confusión con las marcas de
Xara Group Ltd:

- En ningún sitio se afirma ni se sugiere continuidad, afiliación o respaldo.
- El `LICENSE` incluye el descargo de marcas.
- La cadena "Xara" solo aparece en contextos descriptivos y veraces
  ("importa ficheros de Xara Xtreme"), que es uso nominativo legítimo.

---

## 4. Licencias de terceros admisibles

Con `MIT OR Apache-2.0` como licencia del proyecto:

| Licencia de la dependencia | ¿Admisible? | Nota |
|---|---|---|
| MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unlicense, CC0 | ✅ | Sin fricción |
| MPL-2.0 | ⚠️ Con cuidado | *Copyleft* por fichero. Admisible si se usa sin modificar. Afecta p. ej. a `resvg`/`usvg` |
| LGPL (enlace dinámico) | ⚠️ Evitar | Complica el AppImage y el enlazado estático |
| **GPL / AGPL** | ❌ **No** | Contaminaría todo el binario y rompería la licencia permisiva |
| Sin licencia declarada | ❌ No | |

**Control automático:** `cargo deny check licenses` en CI, con la lista de
permitidas configurada en `deny.toml`. Ninguna dependencia entra sin pasarlo.

Punto de atención conocido: el diseño del motor de render valora `resvg`/`usvg`
(MPL-2.0) y algún backend de compresión con doble licencia BSD/GPL-2 (`zstd`);
ambos se documentan en `deny.toml` con su justificación.

---

## 5. Ficheros del repositorio

| Fichero | Contenido |
|---|---|
| `LICENSE` | Declaración de doble licencia + descargo sobre Xara |
| `LICENSE-MIT` | Texto MIT |
| `LICENSE-APACHE` | Texto Apache-2.0 |
| `deny.toml` | Política de licencias verificable en CI |
