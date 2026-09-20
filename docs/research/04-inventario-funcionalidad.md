# 04 — Inventario completo de funcionalidad de Xara Xtreme (Xara LX)

> **Propósito**: backlog de paridad funcional para la reimplementación en Rust (proyecto *Xarast*).
> **Fuente**: árbol de código original `/home/user/xara-xtreme` (Xara LX / Xara Xtreme for Linux, GPLv2, © 1993–2006 Xara Group Ltd).
> **Volumen del original**: ~825 `.cpp` + ~915 `.h`, repartidos en `Kernel/` (modelo de documento, operaciones, filtros, galerías), `tools/` (herramientas interactivas), `wxOil/` (capa OIL = *OS Interface Layer*, wxWidgets), `GDraw/` (motor de rasterizado propietario CDraw), `filters/SVGFilter/` (filtro SVG externo).
> **Fecha de análisis**: 2026-09-19.

## Cómo leer este documento

- **Complejidad de reimplementación**: `Baja` (< 1 semana-persona), `Media` (1–4 sem.), `Alta` (1–3 meses), `Muy alta` (> 3 meses o requiere investigación algorítmica).
- **Prioridad**: `P0` = MVP de la primera versión Linux en Rust; `P1` = segunda oleada (producto usable a diario); `P2` = paridad avanzada; `P3` = legado / nicho / probablemente descartable.
- Los ficheros C++ citados son rutas **relativas a la raíz del árbol original**.

---

## 1. Tabla maestra de funcionalidades

### 1.1 Documento, páginas, spreads y capas

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Documento | Modelo de documento como árbol de nodos | Todo el documento es un árbol `Node` (tipado, con atributos como hijos). Base de render, undo, hit-test y serialización. | `Kernel/node.cpp`, `Kernel/nodedoc.cpp`, `Kernel/document.cpp`, `Kernel/basedoc.cpp` | Muy alta | P0 |
| Documento | Componentes de documento (`DocComponent`) | Secciones enchufables del documento: colores, unidades, info, vistas, impresión, fuentes… serializadas por separado. | `Kernel/doccomp.cpp`, `Kernel/colcomp.cpp`, `Kernel/unitcomp.cpp`, `Kernel/infocomp.cpp`, `Kernel/viewcomp.cpp`, `Kernel/princomp.cpp`, `Kernel/fontcomp.cpp` | Media | P0 |
| Documento | Nuevo documento (dibujo) | Crea documento vacío con plantilla por defecto. | `Kernel/menuops.cpp` (`OPTOKEN_FILENEW_DRAWING`), `Kernel/tmpltmgr`/`wxOil/tmplmngr.cpp` | Baja | P0 |
| Documento | Nuevo documento de animación | Documento en modo animación (capas = fotogramas). | `Kernel/frameops.cpp`, `Kernel/menuops.cpp` (`OPTOKEN_FILENEW_ANIMATION`) | Media | P2 |
| Documento | Plantillas (`FileNewTemplate` 1..10, "Save as default") | Menú de plantillas, guardar documento actual como plantilla/por defecto. | `Kernel/tmpltdlg.cpp`, `Kernel/tmpltarg.cpp`, `Kernel/tmpltatr.cpp`, `wxOil/stemplate.cpp`, `wxOil/tmplmngr.cpp`, `Templates/` | Baja | P2 |
| Documento | Abrir / Guardar / Guardar como / Guardar todo | Ciclo de vida de ficheros, MRU de 9 entradas. | `Kernel/menuops.cpp`, `Kernel/filelist.cpp`, `wxOil/camdoc.cpp`, `wxOil/filedlgs.cpp` | Baja | P0 |
| Documento | Lista de archivos recientes (MRU) | 9 entradas persistidas en preferencias. | `Kernel/filelist.cpp`, `wxOil/camprofile.cpp` | Baja | P1 |
| Documento | Información del documento (`FileInfo`) | Diálogo de metadatos/estadísticas del documento. | `Kernel/finfodlg.cpp`, `Kernel/infocomp.cpp` | Baja | P2 |
| Página | Tamaño de página y configuración (`PageSetupDlg`) | Tamaños predefinidos (`DEFAULT_PAGESIZES.res`), personalizado, orientación, márgenes. | `Kernel/pagesize.cpp`, `Kernel/page.cpp`, `Kernel/npaper.cpp`, `Kernel/optspage.cpp`, `wxOil/xrc/DEFAULT_PAGESIZES.res` | Media | P0 |
| Página | Multipágina / spreads | Un documento contiene *spreads*, cada spread contiene páginas; origen de spread ajustable. | `Kernel/spread.cpp`, `Kernel/page.cpp`, `Kernel/chapter.cpp` (`OPTOKEN_SPREADORIGIN`, `OPTOKEN_RESETSPREADORIGIN`) | Alta | P1 |
| Página | Fondo de página / color de papel | Objeto papel renderizable, color de fondo, borrado de fondo. | `Kernel/paper.cpp`, `Kernel/backgrnd.cpp` (`OPTOKEN_BACKGROUND`, `OPTOKEN_DELETEPAGEBACKGROUND`) | Baja | P1 |
| Página | Bordes de impresión visibles | Muestra el área imprimible/sangrado. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWPRINTBORDERS`) | Baja | P2 |
| Capas | Modelo de capas | Capa = nodo contenedor con nombre, visibilidad, bloqueo, imprimible, snap, color de resaltado. | `Kernel/layer.cpp`, `Kernel/layermgr.cpp` | Media | P0 |
| Capas | Galería/panel de capas | Crear, borrar, renombrar, reordenar, mostrar/ocultar, bloquear, multicapa. | `Kernel/layergal.cpp`, `Kernel/sglayer.cpp`, `Kernel/layerdlg.cpp`, `Kernel/layerprp.cpp`, `Kernel/prpslyrs.cpp` | Media | P0 |
| Capas | Propiedades de capa (pestañas) | Diálogo con pestañas de propiedades de capa (nombre, estado, web). | `Kernel/layerprp.cpp` (`OPTOKEN_LAYERPROPERTYTABS`), `Kernel/layerdlg.cpp` | Baja | P1 |
| Capas | Mover selección a capa activa | Operación de traslado entre capas. | `Kernel/lattrops.cpp` (`OPTOKEN_MOVE_SEL_TO_ACTIVE_LAYER`) | Baja | P1 |
| Capas | Mover objeto una capa arriba/abajo | `Ctrl+Shift+U` / `Ctrl+Shift+D`. | `Kernel/zordops.cpp` (`OPTOKEN_MOVELAYERINFRONT`, `OPTOKEN_MOVELAYERBEHIND`) | Baja | P1 |
| Capas | Combinar capas a capa de fotograma | Conversión capas → fotograma de animación. | `Kernel/frameops.cpp` (`OPTOKEN_COMBINELAYERSTOFRAMELAYER`) | Media | P3 |
| Documento | Guías como capa especial | Las guías viven en una capa dedicada no imprimible. | `Kernel/guides.cpp`, `Kernel/prpsgds.cpp` | Baja | P1 |

### 1.2 Dibujo de formas

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Formas | Herramienta Rectángulo (`TOOL5`) | Rectángulos y rectángulos redondeados editables paramétricamente (no se convierten a path). | `tools/rectangl.cpp`, `Kernel/noderect.cpp`, `Kernel/rechrect.cpp` | Media | P0 |
| Formas | Esquinas redondeadas interactivas | Arrastre de manejador de radio en canvas. | `tools/rectangl.cpp`, `Kernel/noderect.cpp` | Media | P0 |
| Formas | Herramienta Elipse (`TOOL12`) | Elipses/círculos paramétricos con manejadores. | `tools/eliptool.cpp`, `Kernel/nodeelip.cpp`, `Kernel/rechellp.cpp` | Media | P0 |
| Formas | Herramienta QuickShape (`TOOL18`) | Polígonos y estrellas regulares: nº de lados (3–10+), estrellado, curvatura, radio/diámetro, modo elipse/polígono. | `tools/regshape.cpp`, `tools/oprshape.cpp`, `Kernel/nodershp.cpp`, `Kernel/rechrshp.cpp`, `Kernel/opsmpshp.cpp` | Alta | P1 |
| Formas | QuickShape: estelación y curvatura | Parámetros vivos `OPTOKEN_TOGGLESTELLATION`, `OPTOKEN_TOGGLECURVATURE`, `OPTOKEN_TOGGLEELIPPOLY`. | `tools/regshape.cpp`, `Kernel/nodershp.cpp` | Media | P1 |
| Formas | QuickShape: nº de lados directo | Atajos `OPTOKEN_QUICKSHAPE_NUMBERSIDES3..10`. | `tools/regshape.cpp` | Baja | P2 |
| Formas | Reshape del borde de QuickShape | Arrastrar un lado deforma toda la figura regular. | `tools/oprshape.cpp` (`OPTOKEN_RESHAPESHAPEEDGE`) | Alta | P2 |
| Formas | Restricción de proporción (Ctrl) durante creación | Cuadrado/círculo perfecto, ángulos constreñidos. | `tools/rectangl.cpp`, `tools/eliptool.cpp`, `Kernel/input.cpp` | Baja | P0 |
| Formas | Creación desde el centro | Modificador de arrastre. | `tools/rectangl.cpp`, `tools/eliptool.cpp` | Baja | P1 |
| Formas | Convertir a formas (`Ctrl+Shift+S`) | Convierte rectángulo/elipse/QuickShape/texto a paths editables. | `Kernel/shapeops.cpp` (`OPTOKEN_ARRANGEMAKESHAPES`, `OPTOKEN_MAKE_SHAPES`) | Media | P0 |
| Formas | Convertir camino a formas | Contorno (grosor de línea) → relleno. | `tools/opcntr.cpp` (`OPTOKEN_CONVERTPATHTOSHAPES`) | Alta | P1 |
| Formas | Convertir a bitmap (`Ctrl+Shift+C`) | Rasteriza la selección a un bitmap incrustado con DPI/profundidad elegibles. | `Kernel/makebmp.cpp`, `Kernel/bmpsdlg.cpp` | Media | P1 |
| Formas | Herramienta Blank (esqueleto) | Herramienta plantilla de referencia para desarrolladores. | `tools/blnktool.cpp` | Baja | P3 |

### 1.3 Edición de caminos (paths / Bézier)

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Paths | Representación de camino | Polilíneas con segmentos línea/Bézier cúbica, verbos `MoveTo/LineTo/CurveTo/CloseFigure`, subcaminos múltiples. | `Kernel/paths.cpp`, `Kernel/pathpcs.cpp`, `Kernel/nodepath.cpp` | Alta | P0 |
| Paths | Herramienta Bézier / Forma (`TOOL11`) | Edición de nodos y manejadores: seleccionar, mover, añadir, borrar, suave/cúspide, línea/curva. | `tools/beztool.cpp`, `Kernel/pathedit.cpp` | Alta | P0 |
| Paths | Herramienta Pluma (`TOOL14`) | Dibujo punto a punto de líneas y curvas con previsualización. | `tools/pentool.cpp`, `Kernel/penedit.cpp` | Media | P0 |
| Paths | Herramienta Mano alzada (`TOOL6`) | Trazo libre con ajuste de curva y suavizado configurable. | `tools/freehand.cpp`, `tools/freeinfo.cpp`, `Kernel/fitcurve.cpp`, `Kernel/rsmooth.cpp` | Alta | P0 |
| Paths | Borrador de mano alzada (rub-out) | Borrar parte del trazo recién dibujado retrocediendo con el ratón. | `tools/freehand.cpp` (cursores `IDC_FREEHANDRUBOUTCUR`) | Media | P1 |
| Paths | Retrofit / re-suavizado de trazo existente | Redibujar sobre un camino existente para modificarlo. | `tools/opretro.cpp` (`OPTOKEN_RETROFIT`, `OPTOKEN_RETROSMOOTH`) | Alta | P2 |
| Paths | Suavizar selección | Reduce nodos manteniendo forma (`OPTOKEN_SMOOTHSELECTION`). | `Kernel/opsmooth.cpp`, `Kernel/ndoptmz.cpp` | Media | P1 |
| Paths | Añadir / borrar puntos | `OPTOKEN_ADDENDPOINT`, `OPTOKEN_DELETEPOINTSOP`. | `tools/opbezier.cpp`, `Kernel/pathedit.cpp` | Media | P0 |
| Paths | Hacer línea / hacer curva | Cambia tipo de segmento seleccionado. | `tools/opbezier.cpp` (`OPTOKEN_MAKELINESOP`, `OPTOKEN_MAKECURVESOP`) | Baja | P0 |
| Paths | Punto suave / punto cúspide | Sincronización o ruptura de los manejadores. | `tools/opbezier.cpp`, `Kernel/pathedit.cpp` | Baja | P0 |
| Paths | Cerrar camino / auto-cierre | `OPTOKEN_CLOSEPATHWITHPATH`, `OPTOKEN_AUTOCLOSEPATHS`. | `tools/opbezier.cpp`, `tools/beztool.cpp` | Baja | P0 |
| Paths | Invertir camino | `OPTOKEN_REVERSEPATH` (afecta a flechas y texto en camino). | `tools/opbezier.cpp`, `Kernel/pathops.cpp` | Baja | P1 |
| Paths | Romper en puntos | `Ctrl`+botón: separar camino en los nodos seleccionados. | `Kernel/opbreak.cpp`, `Kernel/nodepath.cpp` | Media | P1 |
| Paths | Unir formas (`JoinShapes`) | Une extremos de caminos abiertos en un solo camino. | `Kernel/pathops.cpp` (`OPTOKEN_ARRANGEJOINSHAPES`, `OPTOKEN_JOINSHAPEOP`) | Media | P1 |
| Paths | Romper formas (`BreakShapes`) | Separa subcaminos de un camino compuesto en objetos distintos. | `Kernel/pathops.cpp` (`OPTOKEN_ARRANGEBREAKSHAPES`, `OPTOKEN_BREAKSHAPEOP`) | Media | P1 |
| Paths | Combinar formas: Añadir / Restar / Intersectar / Cortar | Booleanas de caminos (`Ctrl+1..4`), con winding rule. | `Kernel/combshps.cpp`, `Kernel/pathops.cpp`, `Kernel/gwinding.cpp`, `Kernel/clamp.cpp` | Muy alta | P1 |
| Paths | Regla de relleno (non-zero / even-odd) | Atributo `AttrWindingRule`. | `Kernel/lineattr.cpp/.h`, `Kernel/gwinding.cpp` | Baja | P0 |
| Paths | Inset path (camino interior/exterior) | Desplazamiento paralelo de camino; base de contornos y biseles. | `Kernel/pathstrk.cpp`, `Kernel/pathtrap.cpp`, `tools/opcntr.cpp` (`OPTOKEN_TOGGLEINSETPATH`) | Muy alta | P2 |
| Paths | Nudge de puntos de camino | Mover nodos con cursores en 6 granularidades (1/5/10/1-quinto/píxel 1/píxel 10). | `Kernel/pathndge.cpp`, `Kernel/opnudge.cpp` | Baja | P1 |
| Paths | Seleccionar / deseleccionar todos los puntos | `OPTOKEN_SELECTALLPATHPOINTS`, `OPTOKEN_DESELECTALLPATHPOINTS`. | `Kernel/pathedit.cpp`, `tools/beztool.cpp` | Baja | P0 |
| Paths | Utilidades geométricas de camino | Longitud, punto en parámetro, tangente, bounding box exacto, aplanado. | `Kernel/pathutil.cpp`, `Kernel/pathproc.cpp`, `Kernel/paths.cpp` | Alta | P0 |
| Paths | Doble camino (`NodeDoublePath`) | Camino con dos trazados para efectos de anchura variable. | `Kernel/ndbldpth.cpp` | Media | P3 |

### 1.4 Selección y transformación

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Selección | Herramienta Selector unificada (`TOOL7`) | Seleccionar, mover, escalar, rotar, sesgar, y editar rellenos — todo en un único modo. Es la herramienta insignia de Xara. | `tools/selector.cpp`, `tools/selinfo.cpp`, `tools/rendsel.cpp` | Muy alta | P0 |
| Selección | Doble estado escala/rotación | Segundo clic en la selección conmuta manejadores de escala ⇄ rotación/sesgo (con centro de rotación arrastrable). | `tools/selector.cpp`, `tools/rotate.h`, `tools/oprotate.cpp` | Alta | P0 |
| Selección | Selección por marquesina (drag box) | Rectángulo de selección, con modo "tocar" vs "encerrar". | `tools/selector.cpp`, `Kernel/opdragbx.cpp` (`OPTOKEN_SELECTOR_DRAGBOX`) | Baja | P0 |
| Selección | Selección aditiva / conmutada | `Shift`+clic añade/quita; clic simple sobre seleccionado conmuta estado. | `tools/selector.cpp`, `Kernel/selop.cpp` | Baja | P0 |
| Selección | Selección "dentro de grupo" (leaf/under) | `Ctrl`+clic selecciona el objeto interior sin desagrupar; `Alt`+clic selecciona el de debajo. Cursores dedicados. | `tools/selector.cpp`, `Kernel/hittest.cpp`, `wxOil/xrc/IDCSR_SEL_LEAF.cur`, `IDCSR_SEL_UNDER.cur` | Alta | P0 |
| Selección | Seleccionar todo / ninguno | `Ctrl+A` / `Esc`. | `Kernel/selall.cpp`, `Kernel/selop.cpp` | Baja | P0 |
| Selección | Rango de selección y estado | `SelRange`, caché de estado y atributos comunes de la selección. | `Kernel/range.cpp`, `Kernel/selstate.cpp`, `Kernel/editsel.cpp` | Alta | P0 |
| Selección | Blobs (manejadores) configurables | Mostrar blobs de objeto, de contorno, de relleno y de bounding box de forma independiente. | `tools/rendsel.cpp`, `Kernel/blobs.cpp`, `wxOil/handles.cpp` | Media | P0 |
| Transformación | Traslación | Arrastre / nudge; matriz afín. | `tools/tranlate.cpp`, `Kernel/matrix.cpp`, `Kernel/trans2d.cpp` | Baja | P0 |
| Transformación | Escala | Con o sin proporción, con o sin escalar grosores de línea. | `tools/opscale.cpp`, `tools/opscale2.cpp`, `Kernel/tranform.cpp` | Media | P0 |
| Transformación | Rotación | Alrededor de un centro arbitrario, con ángulo numérico en la infobar. | `tools/oprotate.cpp`, `Kernel/tranform.cpp` | Media | P0 |
| Transformación | Sesgo (shear) | Manejadores laterales y campo numérico. | `tools/opshear.cpp` | Media | P0 |
| Transformación | Squash (escala no uniforme libre) | Deformación con los 4 manejadores de esquina independientes. | `tools/opsquash.cpp` | Media | P1 |
| Transformación | Voltear horizontal / vertical | Botones de infobar del selector. | `tools/opflip.cpp` | Baja | P0 |
| Transformación | Copiar y transformar (`Copy+drag`) | Arrastre con `+`/botón secundario deja copia. | `Kernel/transop.cpp` (`OPTOKEN_COPYANDTRANSFORM`), `tools/selector.cpp` | Baja | P0 |
| Transformación | Infobar numérica X/Y/W/H/ángulo/sesgo | Campos editables con botones de incremento y 9 puntos de anclaje (grid NW..SE). | `tools/selinfo.cpp`, recursos `IDC_SEL_GRID_*`, `IDC_SEL_BUMP_*` | Media | P0 |
| Transformación | Escala de líneas conmutable | Opción "escalar grosor de línea con el objeto". | `tools/selinfo.cpp` (`IDC_SEL_SCALELINES`), `Kernel/linwthop.cpp` | Baja | P1 |
| Transformación | Bloqueo de aspecto | Candado de proporción en la infobar. | `tools/selinfo.cpp` (`IDC_SEL_PADLOCK`) | Baja | P0 |
| Transformación | Transformaciones dentro de grupos | Propagación correcta de matrices a hijos y a rellenos. | `Kernel/grptrans.cpp`, `Kernel/group.cpp` | Alta | P0 |
| Transformación | Nudge por teclado (24 variantes) | Cursores ×{1, 5, 10, 1/5, píxel 1, píxel 10} × 4 direcciones. | `Kernel/opnudge.cpp`, `wxOil/xrc/STANDARD_HOTKEYS.res` | Baja | P0 |
| Estructura | Agrupar / desagrupar | `Ctrl+G` / `Ctrl+U`; grupos anidados; desagrupado especial (debug). | `Kernel/group.cpp`, `Kernel/groupops.cpp` | Media | P0 |
| Estructura | Orden Z completo | Traer al frente, enviar atrás, adelante/atrás un paso, adelante/atrás una capa. | `Kernel/zordops.cpp` | Baja | P0 |
| Estructura | Alineación y distribución | Diálogo de alineación con 9 anclas + distribución en ambos ejes. | `Kernel/aligndlg.cpp` (`OPTOKEN_OPALIGN`, `OPTOKEN_ARRANGEALIGNMENT`) | Media | P0 |
| Estructura | Duplicar (`Ctrl+D`) | Copia con desplazamiento configurable. | `Kernel/cutop.cpp` (`OPTOKEN_DUPLICATE`), `Kernel/optsedit.cpp` (offset) | Baja | P0 |
| Estructura | Clonar (`Ctrl+K`) | Copia exactamente en la misma posición. | `Kernel/cutop.cpp` (`OPTOKEN_CLONE`) | Baja | P0 |
| Estructura | Cortar / Copiar / Pegar / Pegar en la misma posición | `Ctrl+X/C/V`, `Ctrl+Shift+V`. | `Kernel/cutop.cpp`, `wxOil/natclipm.cpp`, `wxOil/clipext.cpp` | Media | P0 |
| Estructura | Pegar atributos (`Ctrl+Shift+A`) | Aplica sólo los atributos del portapapeles a la selección. | `Kernel/cutop.cpp` (`OPTOKEN_PASTEATTRIBUTES`) | Media | P1 |
| Estructura | Borrar | `Del` / `Backspace`. | `Kernel/cutop.cpp` (`OPTOKEN_DELETE`) | Baja | P0 |
| Estructura | Arrastrar y soltar en el documento | Drag&drop de colores, bitmaps, ficheros y elementos de galería sobre el canvas. | `Kernel/sgdrag.cpp`, `Kernel/draginfo.cpp`, `wxOil/dragmgr.cpp`, `wxOil/dragtrgt.cpp`, `wxOil/dragcol.cpp`, `wxOil/dragbmp.cpp` | Alta | P1 |
| Estructura | Hit-testing preciso | Test contra geometría real (incluye relleno, contorno, texto y bitmaps con alfa). | `Kernel/hittest.cpp`, `Kernel/clicarea.cpp`, `wxOil/grndclik.cpp` | Alta | P0 |

### 1.5 Atributos y color

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Atributos | Sistema de atributos como nodos | Los atributos son nodos hermanos que afectan a los nodos siguientes; herencia por recorrido del árbol. | `Kernel/nodeattr.cpp`, `Kernel/attrmgr.cpp`, `Kernel/attrappl.cpp`, `Kernel/attraggl.cpp`, `Kernel/attrmap.cpp` | Muy alta | P0 |
| Atributos | Atributos actuales / por herramienta | Cada herramienta mantiene sus atributos "actuales" para nuevos objetos. | `Kernel/attrmgr.cpp`, `Kernel/isetattr.cpp` | Media | P0 |
| Atributos | Aplicar atributo interactivo / repetir | `OPTOKEN_APPLYATTRINTERACTIVE`, `OPTOKEN_REPEATAPPLYATTRIB`. | `Kernel/attrappl.cpp` | Media | P1 |
| Color | Modelos de color: RGB, CMYK, HSV, escala de grises, CIE | `COLOURMODEL_RGBT / CMYK / HSVT / GREYT / CIET`. Conversión vía contextos de color. | `Kernel/colmodel.h`, `Kernel/colcontx.cpp`, `Kernel/colormgr.cpp` | Alta | P0 |
| Color | Colores de documento con nombre | Lista de colores del documento, nombrado, renombrado, edición global (cambia todos los usos). | `Kernel/doccolor.cpp`, `Kernel/colcomp.cpp`, `Kernel/cnamecol.cpp`, `Kernel/newcol.cpp` | Alta | P0 |
| Color | Colores vinculados (tinte, sombra, matiz, enlace) | Un color puede derivarse de otro como tinte/sombra/matiz; al editar el padre cambian todos los hijos. **Diferenciador clave de Xara.** | `Kernel/doccolor.cpp`, `Kernel/coldlog.cpp`, `Kernel/colourix.cpp` | Alta | P1 |
| Color | Editor de color avanzado | Diálogo con selector 2D + slider, entrada numérica por modelo, tipo de color (normal/spot/tinte…). | `Kernel/coldlog.cpp`, `wxOil/colpick.cpp`, `wxOil/colourmat.cpp`, `Kernel/colcontx.cpp` | Alta | P0 |
| Color | Barra de color (paleta en pantalla) | Paleta horizontal bajo el documento; clic = relleno, clic derecho/`Shift` = línea; menú contextual. | `wxOil/ccolbar.cpp`, `Kernel/colmenu.cpp`, `Kernel/palmenu.cpp`, `Kernel/collist.cpp`, `Kernel/colclist.cpp` | Media | P0 |
| Color | Cuentagotas / selector de color (`Ctrl+E`) | Toma color de cualquier píxel del documento o de la pantalla. | `wxOil/dragpick.cpp`, `wxOil/colpick.cpp`, `Kernel/menuops.cpp` (`OPTOKEN_UTILCOLOUR`), cursores `IDC_COLOURPICKERCURSOR*` | Media | P1 |
| Color | Sin color / relleno nulo | Entrada especial "no colour" en paleta y galería. | `Kernel/colgal.cpp`, `wxOil/ccolbar.cpp` (`IDC_EDIT_NOCOLOUR`) | Baja | P0 |
| Color | Paletas: ordenar por tono/luminancia/uso | `OPTOKEN_PALETTE_SORT_BY_HUE / LUMINANCE / USE`. | `Kernel/palmenu.cpp`, `Kernel/colgal.cpp` | Baja | P2 |
| Color | Paleta web-safe y color transparente | `OPTOKEN_PALETTE_WEB_SAFE`, `OPTOKEN_PALETTE_TRANSPARENT(_BACKGROUND)`. | `Kernel/palmenu.cpp`, `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | Media | P2 |
| Color | Colores planos (spot) y separación | Soporte de tintas planas para impresión. | `Kernel/colplate.cpp`, `Kernel/xsepsops.cpp` | Alta | P3 |
| Color | Gestión de color / corrección | Contextos de color por dispositivo, calibración de monitor/impresora. | `Kernel/colormgr.cpp`, `Kernel/colcontx.cpp`, `Kernel/colcomp.cpp` | Alta | P3 |
| Línea | Grosor de línea | `AttrLineWidth`, con grosor "hairline" y escalado opcional. | `Kernel/lineattr.cpp`, `Kernel/linwthop.cpp`, `Kernel/linedef.cpp` | Baja | P0 |
| Línea | Color de línea y transparencia de línea | `AttrStrokeColour`, `AttrStrokeTransp`. | `Kernel/lineattr.cpp` | Baja | P0 |
| Línea | Patrones de guiones (`AttrDashPattern`) | Galería de estilos de línea discontinua. | `Kernel/lineattr.cpp`, `Kernel/sgline.cpp`, `Kernel/sgline2.cpp` | Media | P1 |
| Línea | Terminaciones (caps) y uniones (joins) + mitre limit | `AttrStartCap`, `AttrJoinType`, `AttrMitreLimit`. | `Kernel/lineattr.cpp` | Baja | P0 |
| Línea | Flechas de inicio y fin | `AttrStartArrow`, `AttrEndArrow` con galería de puntas. | `Kernel/arrows.cpp`, `Kernel/lineattr.cpp`, `Kernel/sgline.cpp` | Media | P1 |
| Línea | Tipos de trazo (stroke types) | Trazos vectoriales aplicados al camino (galería de líneas). | `Kernel/strkattr.cpp`, `Kernel/strkcomp.cpp`, `Kernel/mkstroke.cpp`, `Kernel/ppstroke.cpp`, `Kernel/ppvecstr.cpp`, `Kernel/sgstroke.cpp` | Alta | P2 |
| Línea | Anchura variable / presión | Perfil de anchura a lo largo del trazo, entrada de tableta. | `Kernel/strkattr.cpp`, `Kernel/pressure.cpp`, `Kernel/brpress.cpp`, `wxOil/tablet.cpp` | Alta | P2 |
| Línea | Pinceles (brushes) | Pinceles basados en objetos repetidos a lo largo del camino, con espaciado, rotación, escala, presión y aleatoriedad. | `Kernel/brshattr.cpp`, `Kernel/brushop.cpp`, `Kernel/brushdlg.cpp`, `Kernel/brshdata.cpp`, `Kernel/brshcomp.cpp`, `Kernel/ppbrush.cpp`, `Kernel/ndbrshmk.cpp`, `Kernel/sgbrush.cpp`, `tools/opdrbrsh.cpp` | Muy alta | P2 |
| Estilos | Estilos / atributos nombrados | `AttrStyle`, aplicación de conjuntos de atributos con nombre. | `Kernel/styles.cpp`, `Kernel/userattr.cpp` | Media | P3 |

### 1.6 Rellenos y transparencias

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Rellenos | Relleno plano | `AttrFlatColourFill`. | `Kernel/fillattr.cpp` | Baja | P0 |
| Rellenos | Relleno lineal (degradado) | `AttrLinearColourFill` con manejadores de inicio/fin en canvas. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Media | P0 |
| Rellenos | Relleno radial / circular | `AttrRadialColourFill`, `AttrCircularColourFill` con centro y radios arrastrables. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Media | P0 |
| Rellenos | Relleno cónico | `AttrConicalColourFill`. | `Kernel/fillattr.cpp` | Media | P1 |
| Rellenos | Relleno cuadrado (square) | `AttrSquareColourFill` (degradado en 4 esquinas). | `Kernel/fillattr.cpp` | Media | P2 |
| Rellenos | Relleno de 3 y 4 colores | `AttrThreeColColourFill`, `AttrFourColColourFill`: degradados de malla simple. **Diferenciador de Xara.** | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Alta | P2 |
| Rellenos | Rampas multi-parada | Varias paradas de color intermedias en un degradado, arrastrables en canvas. | `Kernel/fillramp.cpp`, `Kernel/gradtbl.cpp`, `tools/filltool.cpp` | Alta | P1 |
| Rellenos | Relleno de bitmap | `AttrBitmapColourFill` con manejadores de escala/rotación/sesgo del bitmap y mosaico (tile). | `Kernel/fillattr.cpp`, `Kernel/bitmapfx.cpp`, `tools/filltool.cpp` | Alta | P1 |
| Rellenos | Relleno fractal y ruido | `AttrFractalColourFill`, `AttrNoiseColourFill`, texturas procedurales con grano y semilla. | `Kernel/fracfill.cpp`, `Kernel/fraclist.cpp`, `Kernel/noise1.cpp`, `Kernel/noisebas.cpp`, `Kernel/noisef.cpp` | Alta | P3 |
| Rellenos | Mapeo de relleno: lineal / seno | `AttrFillMappingLinear`, `AttrFillMappingSin` — perfil de interpolación. | `Kernel/fillattr.cpp` | Baja | P2 |
| Rellenos | Efectos de relleno: fade / rainbow / alt rainbow | Interpolación por RGB o recorriendo el círculo cromático en uno u otro sentido. **Diferenciador de Xara.** | `Kernel/fillattr.cpp` (`AttrFillEffectFade/Rainbow/AltRainbow`) | Media | P1 |
| Rellenos | Perfil de relleno (bias/gain) | Curva de aceleración del degradado, editable con gadget interactivo. | `Kernel/biasgain.cpp`, `Kernel/biasdlg.cpp`, `Kernel/biasgdgt.cpp` (`OPTOKEN_FILLPROFILE`) | Media | P1 |
| Rellenos | Edición del relleno **en el lienzo** | Flechas y blobs del degradado directamente sobre el objeto; arrastrar un color de la paleta a un extremo cambia esa parada. **El diferenciador más característico de Xara.** | `tools/filltool.cpp`, `Kernel/opgrad.cpp`, `Kernel/fillndge.cpp`, cursores `IDC_CANDROPONFILL*` | Alta | P0 |
| Rellenos | Nudge del relleno por teclado | 24 variantes `OPTOKEN_FILLNUDGE*`. | `Kernel/fillndge.cpp` | Baja | P2 |
| Rellenos | Mutar relleno (cambiar de tipo conservando colores) | `OPTOKEN_MUTATEFILL`. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Media | P1 |
| Rellenos | Rellenos en perspectiva dentro de moulds | El relleno se deforma con el envolvente/perspectiva. | `Kernel/nodemold.cpp` (`RemovePerspectiveFills`), `Kernel/gmould.cpp` | Alta | P3 |
| Transp. | Herramienta de transparencia (`TOOL17`) | Mismos tipos geométricos que los rellenos, pero sobre el canal alfa, editable en canvas. | `tools/filltool.cpp` (`TranspTool`), `Kernel/fillattr.cpp` | Alta | P0 |
| Transp. | Transparencia plana | `AttrFlatTranspFill`. | `Kernel/fillattr.cpp` | Baja | P0 |
| Transp. | Transparencia graduada (lineal/radial/cónica/cuadrada/3-4 colores/bitmap/fractal) | Todas las geometrías de relleno replicadas en transparencia. | `Kernel/fillattr.cpp` | Alta | P1 |
| Transp. | Tipos de mezcla: Mix, Stained Glass, Bleach | Modos base de composición de la transparencia. | `Kernel/fillval.h` (`TranspType`), `Kernel/fillattr.cpp`, `GDraw/gdraw.h` | Media | P0 |
| Transp. | Modos avanzados: Contrast, Saturation, Darken, Lighten, Brightness, Luminosity, Hue, Bevel | Modos de mezcla tipo *blend modes*, en variantes plana y graduada. **Diferenciador de Xara.** | `Kernel/fillval.h`, `GDraw/gdraw.h`, `wxOil/grndrgn.cpp` | Alta | P2 |
| Transp. | Transparencia de grupo | Aplicar transparencia al grupo como unidad (`OPTOKEN_GROUPTRANSP`, `OPTOKEN_UNGROUPTRANSP`). | `Kernel/group.cpp`, `Kernel/fillattr.cpp` | Alta | P1 |
| Transp. | Perfil de transparencia | `OPTOKEN_TRANSPFILLPROFILE` (bias/gain sobre alfa). | `Kernel/biasgain.cpp`, `tools/filltool.cpp` | Baja | P2 |
| Transp. | Feather (desvanecido de bordes) | Suavizado del borde del objeto con anchura y perfil. **Diferenciador de Xara.** | `Kernel/opfeathr.cpp`, `Kernel/fthrattr.cpp`, `Kernel/fthrconv.cpp`, `wxOil/fthelper.cpp` | Alta | P1 |

### 1.7 Texto

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Texto | Herramienta de texto (`TOOL21`, `F8`) | Texto simple (punto), texto en columna (ancho fijo) y texto en camino. | `tools/texttool.cpp`, `tools/textinfo.cpp`, `tools/textops.cpp` | Muy alta | P0 |
| Texto | Modelo de texto (story/line/char) | Árbol `NodeTextStory` → `NodeTextLine` → `TextChar`. | `Kernel/nodetext.cpp`, `Kernel/nodetxtl.cpp`, `Kernel/nodetxts.cpp` | Alta | P0 |
| Texto | Texto en camino | `Fit Text to Curve`, con inversión del camino (`Ctrl+Shift+R`). | `Kernel/ndtxtpth.cpp`, `tools/textops.cpp` (`OPTOKEN_FITTEXTTOPATH`, `OPTOKEN_REVERSESTORYPATH`) | Alta | P1 |
| Texto | Edición en el lienzo con cursor | Inserción, selección con ratón y teclado, arrastre de selección. | `tools/texttool.cpp`, `Kernel/textacts.cpp` (`OPTOKEN_TEXTSELECTION`) | Alta | P0 |
| Texto | Tipografía y tamaño | `AttrTxtFontTypeface`, `AttrTxtFontSize`. | `Kernel/txtattr.cpp`, `Kernel/fontman.cpp`, `Kernel/fontlist.cpp` | Media | P0 |
| Texto | Negrita / cursiva / subrayado | `AttrTxtBold`, `AttrTxtItalic`, `AttrTxtUnderline`. | `Kernel/txtattr.cpp` | Baja | P0 |
| Texto | Justificación izquierda/centro/derecha/completa | `AttrTxtJustification` + 4 optokens. | `Kernel/txtattr.cpp`, `tools/textops.cpp` | Baja | P0 |
| Texto | Tracking y kerning (incl. auto-kern) | Ajuste global de espaciado y kerning por par; kerning manual X/Y. | `Kernel/txtattr.cpp`, `tools/textops.cpp` (`OPTOKEN_KERNTEXT`, `OPTOKEN_AUTOKERNTEXT`) | Media | P1 |
| Texto | Interlineado (line spacing) | `AttrTxtLineSpace`, absoluto o proporcional. | `Kernel/txtattr.cpp` | Baja | P0 |
| Texto | Relación de aspecto de glifos | `AttrTxtAspectRatio` (estirado horizontal). | `Kernel/txtattr.cpp` | Baja | P2 |
| Texto | Superíndice / subíndice / línea base | `AttrTxtScript`, `AttrTxtBaseLine`. | `Kernel/txtattr.cpp` | Baja | P1 |
| Texto | Márgenes, sangría de primera línea y tabuladores | `AttrTxtLeftMargin`, `AttrTxtRightMargin`, `AttrTxtFirstIndent`, `AttrTxtRuler` con tabuladores L/C/R/decimal arrastrables en la regla. | `Kernel/txtattr.cpp`, `tools/textinfo.cpp`, cursores `IDCSR_TEXT_*TAB.cur` | Alta | P2 |
| Texto | Regla de texto interactiva | Regla contextual con márgenes y tabuladores mientras se edita. | `tools/texttool.cpp`, `Kernel/rulers.cpp`, `wxOil/oilruler.cpp` | Alta | P2 |
| Texto | Gestión de fuentes (FreeType / TrueType / ATM) | Enumeración, caché, sustitución de fuentes ausentes, panose matching. | `Kernel/fontman.cpp`, `Kernel/fntcache.cpp`, `Kernel/ccpanose.cpp`, `Kernel/fttyplis.cpp`, `wxOil/ftfonts.cpp`, `wxOil/ttfonts.cpp`, `wxOil/atmfonts.cpp`, `wxOil/fontbase.cpp` | Alta | P0 |
| Texto | Galería de fuentes con previsualización | Miniaturas por fuente, generación en segundo plano. | `wxOil/sgfonts.cpp`, `wxOil/sgdfonts.cpp`, `wxOil/fontpgen.cpp`, `Kernel/crthumb.cpp` | Media | P1 |
| Texto | Conversión de texto a caminos | Vía "Convertir a formas". | `Kernel/shapeops.cpp`, `Kernel/nodetext.cpp` | Media | P0 |
| Texto | Pegar texto como texto | `OPTOKEN_TEXTPASTE`. | `tools/textops.cpp`, `wxOil/natclipm.cpp` | Baja | P1 |
| Texto | Unicode y codificaciones | Manejo de cadenas Unicode, conversión de codificación. | `wxOil/unicdman.cpp`, `Kernel/impstr.cpp` | Media | P0 |

### 1.8 Bitmaps y fotos

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Bitmaps | Nodo bitmap y bitmaps incrustados | Bitmaps almacenados en el documento con lista de referencias y recuento de uso. | `Kernel/nodebmp.cpp`, `Kernel/bitmap.cpp`, `Kernel/bmplist.cpp`, `wxOil/oilbitmap.cpp` | Alta | P0 |
| Bitmaps | Caché de bitmaps y de nodos | Caché de render por nodo, con políticas FIFO/aleatoria/débil y límite de memoria. | `Kernel/bitmapcache.cpp`, `Kernel/bitmapcachekey.cpp`, `Kernel/nodecach.cpp`, `Kernel/cache.cpp`, `Kernel/cachfifo.cpp`, `Kernel/cachrand.cpp`, `Kernel/cachweak.cpp` | Alta | P1 |
| Bitmaps | Galería de bitmaps | Lista de bitmaps del documento con miniaturas, borrado, sustitución, info. | `Kernel/sgbitmap.cpp`, `Kernel/bmplist.cpp` | Media | P0 |
| Bitmaps | Propiedades y conversión de profundidad | 1/4/8/24/32 bpp, paleta indexada, alfa. | `Kernel/bmpcomp.cpp`, `wxOil/dibconv.cpp`, `wxOil/dibutil.cpp`, `wxOil/cbmpdata.cpp` | Media | P1 |
| Bitmaps | Efectos de bitmap (Bfx): brillo/contraste | Diálogo con previsualización. | `Kernel/bfxdlg.cpp`, `Kernel/bfxbase.cpp`, `Kernel/bfxop.cpp`, `wxOil/bfxalu.cpp`, `wxOil/bfxpixop.cpp` | Media | P2 |
| Bitmaps | Bfx: profundidad de color / paleta | Reducción de colores con dithering. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_COLOURDEPTH`), `wxOil/gpalopt.cpp` | Media | P2 |
| Bitmaps | Bfx: voltear y rotar | Rotación 90/180/270 y volteos del bitmap. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_FLIPROTATE`) | Baja | P2 |
| Bitmaps | Bfx: redimensionar | Cambio de resolución con remuestreo. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_RESIZE`) | Media | P2 |
| Bitmaps | Bfx: efectos especiales (menú de 10+) | Desenfoque, enfoque, relieve, etc. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFX_SPECIALEFFECTS`), `Kernel/bfxatom.cpp`, `Kernel/bfxitem.cpp`, `Kernel/bfxmngr.cpp` | Media | P3 |
| Bitmaps | Plug-ins de efectos de bitmap (estilo Photoshop) | Gestor de plug-ins externos con undo. | `Kernel/plugin.cpp`, `Kernel/plugmngr.cpp`, `Kernel/plugop.cpp`, `Kernel/plugopun.cpp`, `Kernel/bfxopun.cpp`, `Kernel/optsplug.cpp` | Alta | P3 |
| Bitmaps | Trazador de bitmap (auto-trace) | Convierte bitmap en vectores (diálogo con parámetros de trazado). | `Kernel/tracedlg.cpp`, `Kernel/tracectl.cpp`, `Kernel/tracergn.cpp` | Muy alta | P2 |
| Bitmaps | Relleno de bitmap con manipuladores | Ver §1.6. | `tools/filltool.cpp` | Alta | P1 |
| Bitmaps | Transparencia de bitmap (máscara) | `AttrBitmapTranspFill` como canal alfa procedural. | `Kernel/fillattr.cpp`, `Kernel/maskedrr.cpp`, `wxOil/maskfilt.cpp` | Alta | P2 |
| Bitmaps | Previsualización de bitmap / miniaturas | Miniaturas para galerías y para el formato nativo. | `Kernel/bmapprev.cpp`, `Kernel/crthumb.cpp`, `wxOil/thumb.cpp`, `Kernel/prvwflt.cpp` | Media | P1 |
| Bitmaps | Bitmaps en modo "sequence" (animación) | Secuencias de bitmaps para GIF animado. | `Kernel/bmpseq.cpp`, `Kernel/bmpsrc.cpp` | Media | P3 |
| Bitmaps | Importación asíncrona de bitmaps | Carga en segundo plano con barra de progreso. | `Kernel/impbmp.cpp` (`OPTOKEN_ASYNCHBITMAPIMPORT`), `wxOil/progress.cpp` | Media | P2 |

### 1.9 Efectos vivos (Live Effects) y post-proceso

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Live FX | Herramienta Live Effects (`TOOL26`) | Aplica efectos de bitmap **no destructivos** a objetos vectoriales; el vector sigue siendo editable. **Diferenciador de Xara.** | `tools/liveeffectstool.cpp`, `tools/liveeffectsinfo.cpp`, `tools/opliveeffects.cpp` | Muy alta | P3 |
| Live FX | Pila de efectos (effects stack) | Varios efectos encadenados por objeto, reordenables, con bloqueo y resolución por efecto. | `Kernel/effects_stack.cpp`, `Kernel/nodeliveeffect.cpp`, `Kernel/nodepostpro.cpp` | Alta | P3 |
| Live FX | Añadir / editar / insertar / quitar / quitar todos | Botones `IDC_CCBUTTON_LE_*` en la infobar. | `tools/opliveeffects.cpp` (`OPTOKEN_APPLY_LIVEEFFECT`, `OPTOKEN_EDIT_LIVEEFFECT`, `OPTOKEN_DELETE_LIVEEFFECT`, `OPTOKEN_DELETEALL_LIVEEFFECT`) | Media | P3 |
| Live FX | Bloqueo y resolución de efecto | `OPTOKEN_CHANGE_EFFECT_LOCK`, `..._LOCKALL`, `..._RES`. | `tools/opliveeffects.cpp`, `Kernel/effects_stack.cpp` | Media | P3 |
| Live FX | Puente XPE (Xara Picture Editor) | Filtro/plug-in externo que recibe datos Xar para editar el efecto. | `Kernel/xpfilter.cpp`, `Kernel/xpfcaps.cpp`, `Kernel/xpfrgn.cpp`, `wxOil/xpoilflt.cpp` (`OPTOKEN_XPE_EDIT`) | Alta | P3 |
| Live FX | Efectos legado | Compatibilidad con efectos de versiones anteriores. | `tools/opliveeffects.cpp` (`OPTOKEN_EDIT_LEGACYEFFECT`) | Baja | P3 |
| Live FX | Atributo offscreen / renderizado a bitmap intermedio | Infraestructura de render a superficie auxiliar para efectos y transparencia de grupo. | `Kernel/offattr.cpp`, `wxOil/offscrn.cpp`, `Kernel/pmaskrgn.cpp` | Alta | P2 |

### 1.10 Blend, Mould, Contour, Shadow, Bevel, ClipView

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Blend | Herramienta Blend (`TOOL16`, `F7`) | Interpolación entre dos o más objetos: forma, color, atributos y posición. **Diferenciador de Xara.** | `tools/blndtool.cpp`, `Kernel/nodeblnd.cpp`, `Kernel/gblend.cpp`, `Kernel/blndhelp.cpp` | Muy alta | P2 |
| Blend | Nº de pasos / distancia entre pasos | Dos modos de control del blend. | `tools/blndtool.cpp` (`OPTOKEN_CHANGEBLENDSTEPS`, `OPTOKEN_CHANGEBLENDDISTANCE`) | Media | P2 |
| Blend | Perfiles de blend (objeto y atributos) | Curvas bias/gain independientes para la posición y para los atributos. | `tools/blndtool.cpp` (`OPTOKEN_CHANGEBLENDPROFILE`), `Kernel/biasgain.cpp` | Media | P2 |
| Blend | Blend sobre camino | Adjuntar/desadjuntar camino guía, modo tangencial, 1-a-1, antialias. | `tools/blndtool.cpp` (`OPTOKEN_ADDBLENDPATH`, `OPTOKEN_DETACHBLENDPATH`, `OPTOKEN_BLENDTANGENTIAL`, `OPTOKEN_BLENDONETOONE`, `OPTOKEN_BLENDANTIALIAS`) | Alta | P2 |
| Blend | Remapeo de nodos del blend | Ajustar la correspondencia de puntos entre los dos extremos. | `tools/blndtool.cpp`, cursor `IDC_BLENDABLEREMAPCURSOR.cur` | Alta | P3 |
| Blend | Editar objetos extremo del blend | `OPTOKEN_EDITBLENDENDOBJECT`; edición viva con el blend recalculándose. | `tools/blndtool.cpp` | Alta | P2 |
| Blend | Quitar blend | `OPTOKEN_REMOVEBLEND`. | `Kernel/nodeblnd.cpp` | Baja | P2 |
| Mould | Herramienta Mould (`TOOL19`, `F6`) | Envolventes (envelope) y perspectivas aplicadas a cualquier objeto o grupo. **Diferenciador de Xara.** | `tools/moldtool.cpp`, `Kernel/nodemold.cpp`, `Kernel/moldshap.cpp`, `Kernel/moldedit.cpp` | Muy alta | P2 |
| Mould | Envelope rectangular y presets | Envolvente por defecto, banner, circular, cóncavo, elíptico. | `Kernel/moldenv.cpp`, recursos `IDC_BTN_*ENVELOPE` | Alta | P2 |
| Mould | Perspectiva rectangular y presets | Perspectiva por defecto, suelo, tejado, izquierda, derecha; punto de fuga arrastrable. | `Kernel/moldpers.cpp` (`OPTOKEN_DRAGVANISHPOINT`) | Alta | P2 |
| Mould | Copiar / pegar envolvente y perspectiva | `OPTOKEN_COPYMOULD`, `OPTOKEN_PASTEENVELOPE`, `OPTOKEN_PASTEPERSPECTIVE`. | `tools/moldtool.cpp` | Media | P3 |
| Mould | Rotar / desadjuntar / quitar mould, rejilla de mould | `OPTOKEN_ROTATEMOULD`, `OPTOKEN_DETACHMOULD`, `OPTOKEN_REMOVEMOULD`, `OPTOKEN_TOGGLEMOULDGRID`. | `tools/moldtool.cpp`, `Kernel/gmould.cpp` | Media | P3 |
| Mould | Nodos moldeados (grupo, tinta, camino) | Infraestructura de nodos deformados. | `Kernel/ndmldgrp.cpp`, `Kernel/ndmldink.cpp`, `Kernel/ndmldpth.cpp`, `Kernel/nodemldr.cpp` | Alta | P2 |
| Contour | Herramienta Contour (`TOOL24`, `Ctrl+F7`) | Contornos interiores/exteriores con N pasos, distancia, tipo de unión y transición de color. | `tools/cntrtool.cpp`, `tools/opcntr.cpp`, `Kernel/nodecntr.cpp`, `Kernel/ncntrcnt.cpp` | Muy alta | P2 |
| Contour | Parámetros de contorno | Pasos, distancia/anchura, interior/exterior, perfil de objeto y de atributos, tipo de unión (mitre/round/bevel), tipo de color. | `tools/cntrtool.cpp` (`OPTOKEN_CHANGECONTOUR*`) | Alta | P2 |
| Shadow | Herramienta Sombra (`TOOL22`, `Ctrl+F2`) | Sombras suaves en tiempo real: pared, suelo, resplandor (glow) y feather. **Diferenciador de Xara.** | `tools/shadtool.cpp`, `tools/opshadow.cpp`, `tools/shadinfo.cpp`, `tools/ShadowTl.cpp`, `Kernel/nodeshad.cpp`, `Kernel/bshadow.cpp` | Muy alta | P1 |
| Shadow | Parámetros de sombra | Posición, ángulo, altura, escala, penumbra (desenfoque), oscuridad, perfil. | `tools/opshadow.cpp` (`OPTOKEN_SHADOWANGLE`, `SHADOWHEIGHT`, `SHADOWPENUMBRA`, `SHADOWDARKNESS`, `SHADOWPROFILE`, `SHADOWSCALE`) | Alta | P1 |
| Shadow | Glow (resplandor) | Sombra tipo halo con anchura propia. | `tools/opshadow.cpp` (`OPTOKEN_GLOWWIDTH`) | Media | P2 |
| Bevel | Herramienta Bisel (`TOOL23`, `Ctrl+F3`) | Biseles 3D interiores/exteriores con tipo de perfil, ángulo e inclinación de luz y contraste. | `tools/bevtool.cpp`, `tools/opbevel.cpp`, `tools/bevinfo.cpp`, `Kernel/nodebev.cpp`, `Kernel/beveler.cpp`, `Kernel/attrbev.cpp`, `Kernel/bevfill.cpp`, `Kernel/nbevcont.cpp`, `Kernel/ppbevel.cpp`, `Kernel/bevtrap.cpp` | Muy alta | P2 |
| Bevel | Parámetros de bisel | Indentación, ángulo de luz, inclinación, contraste, tipo, uniones (mitre/round/bevel), interior/exterior. | `Kernel/attrbev.cpp` (`AttrBevelIndent`, `AttrBevelLightAngle`, `AttrBevelContrast`, `AttrBevelType`, `AttrBevelLightTilt`) | Alta | P2 |
| ClipView | Aplicar / quitar ClipView | Recorte de objetos por la forma superior (máscara vectorial viva). | `tools/opclip.cpp` (`OPTOKEN_APPLY_CLIPVIEW`, `OPTOKEN_REMOVE_CLIPVIEW`), `Kernel/nodeclip.cpp`, `Kernel/clipattr.cpp`, `Kernel/ndclpcnt.cpp`, `Kernel/clipint.cpp` | Alta | P1 |
| Feather | Feather sobre cualquier objeto | Ver §1.6; incluye perfil y tamaño en la infobar dedicada (`IDD_BUTTBAR_FEATHER`). | `Kernel/opfeathr.cpp` (`OPTOKEN_FEATHER`, `OPTOKEN_UNFEATHER`, `OPTOKEN_FEATHERSIZE`, `OPTOKEN_FEATHERPROFILE`) | Alta | P1 |

### 1.11 Galerías y paneles

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Galerías | Infraestructura de galerías | Panel acoplable genérico con árbol de grupos, búsqueda, orden, opciones, arrastre. | `Kernel/sgallery.cpp`, `Kernel/sgbase.cpp`, `Kernel/sgtree.cpp`, `Kernel/sgmenu.cpp`, `Kernel/sginit.cpp`, `Kernel/gallery.cpp`, `wxOil/galbar.cpp` | Alta | P1 |
| Galerías | Galería de colores (`F9`) | Colores del documento y paletas, crear/editar/borrar/renombrar, arrastrar a objetos. | `Kernel/sgcolour.cpp`, `Kernel/colgal.cpp` | Media | P0 |
| Galerías | Galería de capas (`F10`) | Ver §1.1. | `Kernel/sglayer.cpp`, `Kernel/layergal.cpp` | Media | P0 |
| Galerías | Galería de bitmaps (`F11`) | Ver §1.8. | `Kernel/sgbitmap.cpp` | Media | P0 |
| Galerías | Galería de líneas (`F12`) | Estilos de línea, guiones, flechas, tipos de trazo y pinceles. | `Kernel/sgline.cpp`, `Kernel/sgline2.cpp`, `Kernel/sgstroke.cpp`, `Kernel/sgbrush.cpp`, `wxOil/sglinepr.cpp` | Media | P1 |
| Galerías | Galería de fuentes (`Shift+F9`) | Ver §1.7. | `wxOil/sgfonts.cpp`, `wxOil/sgdfonts.cpp` | Media | P1 |
| Galerías | Galería de clipart (`Shift+F10`) | Biblioteca indexada en disco con miniaturas y descarga web. | `Kernel/sglcart.cpp`, `Kernel/sglib.cpp`, `Kernel/sglbase.cpp`, `Kernel/sgscan.cpp`, `wxOil/sgliboil.cpp`, `wxOil/sgindgen.cpp` | Alta | P3 |
| Galerías | Galería de rellenos (`Shift+F11`) | Biblioteca de texturas/degradados predefinidos. | `Kernel/sglfills.cpp` | Media | P3 |
| Galerías | Galería de fotogramas (`Shift+F12`) | Fotogramas de animación: nuevo, copiar, borrar, mover, propiedades, previsualización. | `Kernel/sgframe.cpp`, `Kernel/frameops.cpp` | Media | P3 |
| Galerías | Galería de nombres (`Ctrl+Shift+F9`) | Nombres/conjuntos aplicados a objetos: selección por nombre, exportabilidad, triggers web. Base del modelo de rollovers y slices. | `Kernel/sgname.cpp`, `Kernel/ngcore.cpp`, `Kernel/ngdialog.cpp`, `Kernel/ngitem.cpp`, `Kernel/ngiter.cpp`, `Kernel/ngprop.cpp`, `Kernel/ngscan.cpp`, `Kernel/ngsentry.cpp`, `Kernel/ngsetop.cpp`, `Kernel/ngdrag.cpp` | Alta | P2 |
| Galerías | Descarga de bibliotecas web | Añadir carpetas/bibliotecas web, descarga de miniaturas e ítems. | `Kernel/inetop.cpp` (`OPTOKEN_OPADDWEBFOLDERS`, `OPTOKEN_OPDOWNLOAD`, `OPTOKEN_OPTHUMBDOWNLOAD`), `wxOil/camnet.cpp`, `wxOil/lddirect.cpp` | Alta | P3 |
| Galerías | Búsqueda / orden / opciones de galería | `OPTOKEN_SGSEARCHDLG`, `OPTOKEN_SGSORTDLG`, `OPTOKEN_SGOPTIONSDLG`. | `Kernel/sgmenu.cpp`, `Kernel/sgscanf.cpp` | Media | P3 |
| Paneles | Barras acoplables y personalizables | Barras de botones configurables (`ToolbarDlg`), 11 barras predefinidas + barra de estado, reglas y barra de color. | `Kernel/bars.cpp`, `Kernel/stdbars.cpp`, `Kernel/barcreationdlg.cpp`, `wxOil/basebar2.cpp`, `wxOil/dockbar.cpp`, `wxOil/cstatbar.cpp` | Alta | P2 |
| Paneles | Infobar contextual por herramienta | Cada herramienta publica su propia barra de opciones. | `Kernel/infobar.cpp`, `tools/*info.cpp`, `wxOil/xrc/*bar*.xrc` | Alta | P0 |
| Paneles | Barra de estado con indicadores | Coordenadas, snap, calidad de render, modo impresión, transparencia. | `Kernel/statline.cpp`, `wxOil/cstatbar.cpp`, recursos `IDB_SL_*` | Media | P1 |
| Paneles | Menús contextuales | Menú del botón derecho sensible al contexto (objeto, galería, paleta, regla). | `Kernel/contmenu.cpp`, `Kernel/colmenu.cpp`, `Kernel/palmenu.cpp`, `Kernel/prvwmenu.cpp`, `wxOil/oilmenus.cpp`, `Kernel/menuitem.cpp` | Media | P0 |
| Paneles | Ayuda de burbuja / status help | Texto de ayuda por gadget en barra de estado y tooltip. | `wxOil/bblwnd.cpp`, `wxOil/ctrlhelp.cpp`, `wxOil/helptabs.cpp` | Baja | P2 |

### 1.12 Importación

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Importación | Arquitectura de filtros | Registro de filtros, familias (vector/bitmap/texto/paleta), detección por contenido y extensión, opciones por filtro. | `Kernel/filters.cpp`, `Kernel/filtrmgr.cpp`, `Kernel/camfiltr.cpp`, `Kernel/impexpop.cpp`, `wxOil/oilfltrs.cpp` | Alta | P0 |
| Importación | Formato nativo `.xar` (v2, comprimido) | Lectura del formato Xar: árbol de records tipados, compresión zlib, referencias, bitmaps incrustados. | `Kernel/native.cpp`, `Kernel/cxfile.cpp`, `Kernel/cxftree.cpp`, `Kernel/cxfrec.cpp`, `Kernel/cxftfile.cpp`, `Kernel/zinflate.cpp`, `Kernel/zstream.cpp`, `Kernel/rech*.cpp` | Muy alta | P0 |
| Importación | Formato `.web` (web nativo minimalista) | Variante reducida del nativo. | `Kernel/webfiltr.cpp` | Media | P2 |
| Importación | SVG | Filtro SVG externo (importa geometría, estilos y degradados). | `filters/SVGFilter/svgimporter.cpp`, `import.cpp`, `gradients.cpp`, `styles.cpp`, `svgfilter.cpp` | Muy alta | P0 |
| Importación | PNG | Importación nativa con alfa. | `wxOil/pngfiltr.cpp`, `wxOil/pngutil.cpp`, `Kernel/pngfuncs.cpp` | Baja | P0 |
| Importación | JPEG | Importación nativa (libjpeg). | `Kernel/imjpeg.cpp`, `Kernel/jpgsrc.cpp`, `Kernel/jpgermgr.cpp`, `Kernel/jpgprgrs.cpp` | Baja | P0 |
| Importación | GIF (incl. animado) | Importación con transparencia y secuencias. | `wxOil/giffiltr.cpp`, `wxOil/gifutil.cpp`, `Kernel/bmpseq.cpp` | Media | P1 |
| Importación | BMP / DIB | Vía ImageMagick y filtro propio. | `wxOil/imgmgkft.cpp`, `wxOil/dibutil.cpp`, `Kernel/bitfilt.cpp` | Baja | P1 |
| Importación | TIFF, PSD, PDF, PICT, PNM/PPM, XPM, ICO, PCD | Delegados al binario externo **ImageMagick** (`convert`). | `wxOil/imgmgkft.cpp`, `Kernel/filters.cpp` (bloque `ImageMagickFilter*`) | Media | P2 |
| Importación | PPM / PGM / PBM nativos | Filtros propios. | `wxOil/ppmfiltr.cpp` | Baja | P3 |
| Importación | EPS genérico + Adobe Illustrator (AI, AI5, AI8) | Intérprete PostScript parcial con pila y manejadores de operadores. | `Kernel/epsfiltr.cpp`, `Kernel/epsstack.cpp`, `Kernel/epsclist.cpp`, `Kernel/epssitem.cpp`, `Kernel/epscdef.cpp`, `Kernel/ai_eps.cpp`, `Kernel/ai5_eps.cpp`, `Kernel/ai8_eps.cpp`, `Kernel/ai_grad.cpp`, `Kernel/ai_layer.cpp`, `Kernel/ai_bmp.cpp`, `Kernel/ai_epsrr.cpp` | Muy alta | P2 |
| Importación | EPS de Photoshop, FreeHand, CorelDRAW 3/4, ArtWorks | Variantes del intérprete EPS. | `Kernel/coreleps.cpp`, `Kernel/freeeps.cpp`, `Kernel/aw_eps.cpp`, `Kernel/cameleps.cpp`, `Kernel/nativeps.cpp` | Alta | P3 |
| Importación | CorelDRAW `.cdr` | Filtro nativo CDR (relleno, contorno, texto). | `Kernel/cdrfiltr.cpp`, `Kernel/cdrfill.cpp`, `Kernel/cdroutl.cpp`, `Kernel/cdrtext.cpp`, `wxOil/cdrbitm.cpp` | Muy alta | P3 |
| Importación | Corel CMX 16/32 bits | Filtro CMX completo con árbol de records. | `Kernel/cmxifltr.cpp`, `Kernel/cmxfiltr.cpp`, `Kernel/cmxicmds.cpp`, `Kernel/cmxibits.cpp`, `Kernel/cmxirefs.cpp`, `Kernel/cmxistut.cpp`, `Kernel/cmxrendr.cpp`, `Kernel/cmxtree.cpp`, `Kernel/cmxdcobj.cpp` | Muy alta | P3 |
| Importación | WMF / EMF (metafiles Windows) | Filtro de metafichero. | `wxOil/metafilt.cpp`, `wxOil/metaview.cpp`, `wxOil/clipmap.cpp` | Alta | P3 |
| Importación | Acorn Draw, Sprite (RISC OS) | Formatos históricos. | `Kernel/drawfltr.cpp`, `wxOil/` (SpriteFilter) | Media | P3 |
| Importación | Paletas: MS, PaintShop Pro, Corel, Adobe (ACO/ACT), JCW | Filtros de paletas de color. | `Kernel/impcol.cpp`, `Kernel/expcol.cpp`, `wxOil/oilfltrs.cpp` | Baja | P3 |
| Importación | Texto ANSI / Unicode / RTF | Filtros de texto (condicionales). | `Kernel/textfltr.cpp`, `Kernel/impstr.cpp` | Media | P3 |
| Importación | Importar desde URL (`Ctrl+Shift+Y`) | Descarga y importa un recurso remoto. | `Kernel/urlimp.cpp`, `Kernel/inetop.cpp`, `wxOil/camnet.cpp` | Media | P3 |
| Importación | Importar en capas / opciones | Preferencias: importar con capas, abrir con capas, bitmaps en capas, colores sin nombre. | `Kernel/filters.cpp` (prefs `ImportWithLayers`, `OpenWithLayers`, `ImportBitmapsOntoLayers`, `AddUnnamedColours`) | Baja | P1 |
| Importación | Soltar fichero en la ventana | `OPTOKEN_DROPPEDFILE`. | `Kernel/impexpop.cpp`, `wxOil/dragtrgt.cpp` | Baja | P1 |

### 1.13 Exportación

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Exportación | Diálogo de exportación con previsualización | Vista previa con comparación de opciones, tamaño estimado de fichero, zoom. | `Kernel/bmpsdlg.cpp`, `Kernel/bmpexdoc.cpp`, `Kernel/bmpexprw.cpp`, `Kernel/prevwdlg.cpp`, `wxOil/filesize.cpp` | Alta | P1 |
| Exportación | Exportar selección / página / dibujo | Ámbito de exportación configurable, con DPI y antialias. | `Kernel/expbmp.cpp`, `Kernel/impexpop.cpp` | Media | P0 |
| Exportación | PNG (con alfa, entrelazado, paleta) | Exportador nativo. | `wxOil/outptpng.cpp`, `wxOil/pngfiltr.cpp` | Baja | P0 |
| Exportación | JPEG (calidad, progresivo) | Exportador nativo. | `Kernel/exjpeg.cpp`, `Kernel/jpgdest.cpp` | Baja | P0 |
| Exportación | GIF (paleta, dithering, transparencia, entrelazado) | Exportador con optimización de paleta. | `wxOil/outptgif.cpp`, `wxOil/giffiltr.cpp`, `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | Media | P1 |
| Exportación | GIF animado | Guarda las capas/fotogramas como GIF animado con retardos y disposal. | `Kernel/frameops.cpp` (`OPTOKEN_SAVEANIMATEDGIF`), `Kernel/animparams.cpp`, `Kernel/bmpseq.cpp`, `wxOil/outptgif.cpp` | Alta | P3 |
| Exportación | BMP / DIB | Exportador nativo. | `wxOil/outptdib.cpp` | Baja | P2 |
| Exportación | TIFF / PSD / PDF / otros vía ImageMagick | Delegación al binario externo. | `wxOil/imgmgkft.cpp` | Media | P2 |
| Exportación | Formato nativo `.xar` | Escritura del árbol de records con compresión. | `Kernel/native.cpp`, `Kernel/nativeop.cpp`, `Kernel/cxf*.cpp`, `Kernel/zdeflate.cpp`, `Kernel/zdftrees.cpp` | Muy alta | P0 |
| Exportación | Formato `.web` | Guardado web nativo (`OPTOKEN_SAVEASWEB`). | `Kernel/webfiltr.cpp` | Media | P3 |
| Exportación | SVG | Filtro SVG externo de exportación con diálogo de opciones. | `filters/SVGFilter/export.cpp`, `svgexportdialog.cpp`, `svgfilterui.cpp` | Alta | P0 |
| Exportación | EPS / PostScript | Exportación EPS (nativo Camelot y genérico), con prólogo PS. | `Kernel/saveeps.cpp`, `Kernel/cameleps.cpp`, `Kernel/nativeps.cpp`, `Kernel/psrndrgn.cpp`, `wxOil/psdc.cpp`, `wxOil/xrc/prolog.ps`, `setup.ps`, `spotfunc.ps` | Alta | P2 |
| Exportación | Flash / SWF | Exportador vectorial a Flash: formas, texto, bitmaps, sprites, botones. (Compilado sólo en debug en Xara LX.) | `Kernel/swffiltr.cpp`, `Kernel/swfexpdc.cpp`, `Kernel/swfshape.cpp`, `Kernel/swftext.cpp`, `Kernel/swfbitmp.cpp`, `Kernel/swfsprit.cpp`, `Kernel/swfbuttn.cpp`, `Kernel/swfplace.cpp`, `Kernel/swffont.cpp`, `Kernel/swfrndr.cpp` | Muy alta | P3 |
| Exportación | HTML + imagemap | Genera página HTML con mapa de imagen a partir de las URLs de los objetos. | `Kernel/htmlexp.cpp`, `Kernel/htmlfltr.cpp`, `Kernel/htmllist.cpp`, `Kernel/imagemap.cpp` | Alta | P3 |
| Exportación | Image slicing (`Ctrl+I`) | Corta el dibujo en trozos según objetos nombrados y exporta HTML + imágenes + rollovers. | `Kernel/slice.cpp`, `Kernel/slicehelper.cpp`, `tools/slicetool.cpp`, `Kernel/opimgset.cpp` | Muy alta | P3 |
| Exportación | Exportación por "sets" (name gallery) | Exportar sólo los objetos de un conjunto nombrado. | `Kernel/ngsetop.cpp` (`OPTOKEN_EXPORT_SETS`) | Media | P3 |
| Exportación | Hints de exportación / parámetros persistidos | Recuerda opciones por formato en el documento. | `Kernel/exphint.cpp`, `Kernel/exagal.cpp` | Baja | P2 |
| Exportación | Exportar paleta de colores | Volcado de la paleta a fichero. | `Kernel/expcol.cpp` | Baja | P3 |

### 1.14 Web y animación

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Web | Dirección web por objeto (`Ctrl+Shift+W`) | `AttrWebAddress`: URL + frame destino asociada a cualquier objeto. | `Kernel/webattr.cpp`, `Kernel/webaddr.cpp`, `Kernel/hlinkdlg.cpp`, `Kernel/urldlg.cpp` | Media | P3 |
| Web | Estados de rollover (4 estados) | Default / Mouse over / Clicked / Selected, como capas nombradas. | `Kernel/slice.cpp` (`RolloverState`), `Kernel/opbarcreation.cpp` | Alta | P3 |
| Web | Creación de barras de botones | Genera N copias del botón en capas de estado, con nombres `button1..N` y mutación por estado. | `Kernel/opbarcreation.cpp`, `Kernel/barcreationdlg.cpp`, `Kernel/opdupbar.cpp`, `Kernel/bldbrdef.cpp` | Alta | P3 |
| Web | Imagemap (client-side) | Genera `<map>` con áreas poligonales/rectangulares. | `Kernel/imagemap.cpp` | Media | P3 |
| Web | Previsualización en navegador | `OPTOKEN_FRAME_BROWSERPREVIEW`. | `Kernel/frameops.cpp`, `wxOil/camnet.cpp` | Baja | P3 |
| Web | Preferencias web | Opciones de exportación web por defecto. | `Kernel/webprefs.cpp`, `Kernel/webflags.cpp`, `Kernel/webop.cpp`, `Kernel/webparam.cpp` | Baja | P3 |
| Animación | Modelo de fotogramas sobre capas | Cada capa = un fotograma; propiedades de fotograma (retardo, disposal, overlay/solid). | `Kernel/frameops.cpp`, `Kernel/animparams.cpp`, `Kernel/sgframe.cpp`, recursos `IDB_FGAL_OVERLAY/SOLID` | Alta | P3 |
| Animación | Nuevo / copiar / borrar / mover fotograma | Operaciones de la galería de fotogramas. | `Kernel/frameops.cpp` (`OPTOKEN_FRAME_NEWFRAME`, `COPYFRAME`, `DELETEFRAME`) | Media | P3 |
| Animación | Propiedades de animación (bucle, retardo global) | `OPTOKEN_FRAME_ANIPROPERTIES`, `OPTOKEN_GIFANIMPROPERTYTABS`. | `Kernel/animparams.cpp`, `Kernel/frameops.cpp` | Media | P3 |
| Animación | Grab frame / grab all frames | Captura fotogramas rasterizados. | `Kernel/frameops.cpp`, `Kernel/capturemanager.cpp` | Media | P3 |
| Animación | Reproductor de previsualización | Play/stop/anterior/siguiente/inicio/fin. | `Kernel/prevwdlg.cpp`, recursos `IDC_PREVIEW_*` | Media | P3 |
| Animación | Optimización de paleta de animación | Paleta común y dithering entre fotogramas. | `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | Alta | P3 |

### 1.15 Impresión y preprensa

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Impresión | Motor de impresión | Render a contexto de impresora, escalado, mosaico de páginas. | `Kernel/printing.cpp`, `Kernel/printctl.cpp`, `Kernel/prntview.cpp`, `wxOil/grndprnt.cpp`, `wxOil/prncamvw.cpp` | Alta | P2 |
| Impresión | Diálogo de impresión y configuración | Impresora, copias, rango, escalado. | `Kernel/printctl.cpp`, `Kernel/prdlgctl`/`wxOil/prdlgctl.cpp`, `Kernel/optsprin.cpp` | Media | P2 |
| Impresión | Opciones de impresión (layout/general) | Pestañas de opciones de impresión. | `Kernel/optsprin.cpp`, `Kernel/prnprefs.cpp`, recursos `IDD_OPTSTAB_PRINTGENERAL/LAYOUT` | Media | P2 |
| Impresión | Marcas de impresión | Marcas de corte, registro, barra de color, barra de grises, información. | `Kernel/prnmks.cpp`, `Kernel/prnmkcom.cpp`, recursos `IDB_PRINTMARK_*` | Media | P3 |
| Impresión | Separación de color (CMYK + spot) | Previsualización por plancha y salida separada. | `Kernel/colplate.cpp`, `Kernel/xsepsops.cpp`, `Kernel/psrndrgn.cpp` | Alta | P3 |
| Impresión | Previsualización de planchas en pantalla | Composite / Cyan / Magenta / Yellow / Key / Spot 1-8 / Mono. | `Kernel/xsepsops.cpp` (`OPTOKEN_COMPOSITEPREVIEW`, `OPTOKEN_CYANPREVIEW`, …) | Media | P3 |
| Impresión | Sobreimpresión de línea y relleno | `AttrOverprintLine`, `AttrOverprintFill`, `AttrPrintOnAllPlates`. | `Kernel/isetattr.cpp`, `Kernel/xsepsops.cpp` | Baja | P3 |
| Impresión | Imprimir como formas (texto→curvas) | `OPTOKEN_TOGGLEPRINTASSHAPES`. | `Kernel/printing.cpp` | Baja | P3 |
| Impresión | Progreso de impresión | Barra de progreso dedicada. | `wxOil/printprg.cpp`, `wxOil/progbar.cpp` | Baja | P3 |
| Impresión | Generación PostScript | DC PostScript propio con prólogo y funciones de trama. | `wxOil/psdc.cpp`, `Kernel/psrndrgn.cpp`, `wxOil/xrc/prolog.ps`, `spotfunc.ps` | Alta | P3 |

### 1.16 Preferencias y unidades

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Preferencias | Sistema de preferencias declarativas | `DeclareSection` / `DeclarePref` con tipos y rangos; persistencia en perfil. | `Kernel/prefs.cpp`, `Kernel/appprefs.cpp`, `wxOil/oilprefs.cpp`, `wxOil/camprofile.cpp`, `wxOil/registry.cpp` | Media | P0 |
| Preferencias | Diálogo de opciones con pestañas (`Ctrl+Shift+O`) | 13 pestañas: General/Edit, Grid&Ruler, Internet, Misc, Page, Plug-ins, Pointers, Print General, Print Layout, Scale, Tune, Undo, Units, View. | `Kernel/optsedit.cpp`, `optsgrid.cpp`, `optsinet.cpp`, `optsmisc.cpp`, `optspage.cpp`, `optsplug.cpp`, `optspntr.cpp`, `optsprin.cpp`, `optsscal.cpp`, `optstune.cpp`, `optsundo.cpp`, `optsunit.cpp`, `optsview.cpp` | Alta | P1 |
| Preferencias | Pestaña Edit: duplicar offset, ángulo de constricción, nudge | Parámetros de edición. | `Kernel/optsedit.cpp` | Baja | P1 |
| Preferencias | Pestaña View: calidad por defecto, scroll, double-buffer | Opciones de visualización. | `Kernel/optsview.cpp` | Baja | P1 |
| Preferencias | Pestaña Undo: tamaño del buffer de deshacer | Límite de memoria del historial. | `Kernel/optsundo.cpp`, `Kernel/ophist.cpp` | Baja | P1 |
| Preferencias | Pestaña Tune: memoria, caché, rendimiento | Ajustes de caché de bitmaps y memoria. | `Kernel/optstune.cpp`, `wxOil/tunemem.cpp`, `wxOil/cammemory.cpp` | Media | P2 |
| Preferencias | Pestaña Pointers: cursores | Selección de estilo de puntero. | `Kernel/optspntr.cpp`, `wxOil/cursor.cpp` | Baja | P3 |
| Preferencias | Pestaña Scale: escala de dibujo | Escala cartográfica/arquitectónica (p.ej. 1:100). | `Kernel/optsscal.cpp`, `Kernel/scunit.cpp` | Media | P2 |
| Unidades | 12 tipos de unidad | Milímetros, centímetros, metros, pulgadas, pies, yardas, puntos, picas, millipuntos, millas, kilómetros, píxeles + "automático". Base interna: **millipuntos**. | `Kernel/units.cpp`, `Kernel/unittype.h`, `Kernel/unitres.h`, `Kernel/scunit.cpp` | Media | P0 |
| Unidades | Unidades definidas por el usuario | Crear unidades derivadas (numerador/denominador sobre una unidad base). | `Kernel/units.cpp`, `Kernel/optsunit.cpp` (`OPTOKEN_UNITPROPERTIESDLG`) | Media | P3 |
| Unidades | Unidades de página vs unidades de fuente | Dos conjuntos independientes (documento y tipografía). | `Kernel/units.cpp`, `Kernel/unitcomp.cpp` | Baja | P1 |
| Unidades | Parseo/formateo de medidas en campos | Aceptar "10mm", "1 in", "3p6" en cualquier campo numérico. | `Kernel/units.cpp`, `Kernel/usercord.cpp`, `Kernel/userrect.cpp` | Media | P0 |
| Preferencias | Diálogo de consejos al arrancar | "Tip of the day". | `Kernel/tipsdlg.cpp` | Baja | P3 |
| Preferencias | Hotkeys configurables | Tabla de atajos cargada de recurso, con modificadores y contexto. | `Kernel/hotkeys.cpp`, `wxOil/keypress.cpp`, `wxOil/wxkeymap.cpp`, `wxOil/xrc/STANDARD_HOTKEYS.res` | Media | P1 |

### 1.17 Undo / redo y sistema de operaciones

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Undo | Modelo de operaciones (`Operation`/`OpDescriptor`) | Toda acción es una `Operation` registrada con un token; permite menús, barras, macros y undo uniformes. **Pilar arquitectónico.** | `Kernel/ops.cpp`, `Kernel/opdesc.cpp`, `Kernel/opnode.cpp`, `Kernel/undoop.cpp` | Muy alta | P0 |
| Undo | Historial de deshacer/rehacer | Pila de acciones con descripción textual, límite por memoria. | `Kernel/ophist.cpp`, `Kernel/optsundo.cpp` | Alta | P0 |
| Undo | Acciones granulares (`Action`) | Cada operación se compone de acciones reversibles registradas. | `Kernel/undoop.cpp`, `Kernel/cpyact.cpp`, `Kernel/insertnd.cpp`, `Kernel/objchge.cpp` | Alta | P0 |
| Undo | Descripción legible en el menú | "Deshacer: Mover", etc. | `Kernel/opdesc.cpp`, `Kernel/ophist.cpp` | Baja | P0 |
| Undo | Operaciones no deshacibles | Cambios de vista, preferencias, zoom. | `Kernel/ops.cpp` | Baja | P0 |
| Undo | Undo de plug-ins y efectos | Envoltorio de undo para operaciones externas. | `Kernel/plugopun.cpp`, `Kernel/bfxopun.cpp` | Media | P3 |
| Undo | Mensajería interna (`Msg`/`MessageHandler`) | Difusión de eventos de selección, documento, atributos y preferencias. | `Kernel/camtypes.cpp`, `Kernel/msg.h`, `Kernel/optsmsgs.h`, `Kernel/objchge.cpp` | Alta | P0 |
| Undo | Registro de objetos y RTTI propio | `CC_DECLARE_DYNCREATE`, `CCRuntimeClass`, dump de memoria. | `Kernel/objreg.cpp`, `Kernel/ccobject`/`wxOil/ccobject.cpp`, `Kernel/camtypes.cpp` | Alta | P0 |

### 1.18 Snapping, guías, rejilla y reglas

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Snap | Snap a rejilla (`NumPad .`) | Ajuste a la rejilla activa, activable durante el arrastre. | `Kernel/snap.cpp`, `Kernel/snapops.cpp`, `Kernel/grid.cpp` | Media | P0 |
| Snap | Snap a guías (`NumPad 2`) | Ajuste a líneas guía. | `Kernel/snap.cpp`, `Kernel/guides.cpp` | Baja | P0 |
| Snap | Snap a objetos / "magnetic snap" (`NumPad *`) | Ajuste magnético a puntos y bordes de otros objetos, con radios configurables. **Diferenciador de Xara.** | `Kernel/snap.cpp`, `Kernel/snapops.cpp`, `Kernel/optsgrid.cpp`, cursor `IDC_SNAPPED.cur` | Alta | P1 |
| Rejilla | Rejillas rectangulares e isométricas | Tipo, espaciado, subdivisiones, color, por spread. | `Kernel/grid.cpp`, `Kernel/optsgrid.cpp`, `tools/gridtool.cpp` | Media | P1 |
| Rejilla | Herramienta de rejilla (`TOOL10`) | Crear/mover/redimensionar rejillas en el documento. | `tools/gridtool.cpp` (`OPTOKEN_GRIDRESIZE`, `OPTOKEN_GRIDNEWRESIZE`, `OPTOKEN_GRIDSELECTION`) | Media | P3 |
| Rejilla | Mostrar/ocultar rejilla (`#`) | Conmutador de visibilidad. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWGRID`) | Baja | P0 |
| Guías | Crear guía arrastrando desde la regla | Guías horizontales y verticales; cursores dedicados. | `Kernel/guides.cpp`, `wxOil/oilruler.cpp`, cursores `IDCSR_SEL_HGUIDE/VGUIDE.cur` | Media | P1 |
| Guías | Propiedades de guía / borrar guía / borrar todas | Diálogo de posición exacta. | `Kernel/guides.cpp` (`OPTOKEN_EDITGUIDELINEPROPDLG`, `OPTOKEN_DELETEGUIDELINE`, `OPTOKEN_DELETEALLGUIDELINES`) | Baja | P1 |
| Guías | Mostrar/ocultar guías (`NumPad 1`) | Conmutador. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWGUIDES`) | Baja | P1 |
| Reglas | Reglas con unidades y marcador de posición | `Ctrl+L`; origen movible; marcas del puntero. | `Kernel/rulers.cpp`, `wxOil/oilruler.cpp` | Media | P1 |
| Snap | Pull onto grid / arrange pull grid | Ajustar objetos existentes a la rejilla. | `Kernel/oppull.cpp` (`OPTOKEN_PULLONTOGRID`, `OPTOKEN_ARRANGEPULLGRID`) | Baja | P3 |

### 1.19 Vista, renderizado y calidad

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Render | Motor de render `RenderRegion` | Abstracción de destino de dibujo (pantalla, impresora, bitmap, PostScript, SWF) con pila de atributos. | `Kernel/rndrgn.cpp`, `Kernel/rndstack.cpp`, `Kernel/noderend.cpp`, `Kernel/rrcaps.cpp`, `Kernel/region.cpp`, `Kernel/rgnlist.cpp` | Muy alta | P0 |
| Render | Rasterizador CDraw/GDraw | Motor propietario de relleno, antialiasing y transparencias (binario en `libs/`). **Es la clave de la calidad y velocidad de Xara.** | `GDraw/gdraw.h`, `GDraw/gdraw2.h`, `Kernel/GDrawIntf.cpp`, `wxOil/gdrawcon.cpp`, `wxOil/grndrgn.cpp`, `libs/` | Muy alta | P0 (sustituir) |
| Render | Antialiasing de alta calidad | Antialias por defecto en pantalla, seleccionable. | `Kernel/quality.cpp`, `wxOil/grndrgn.cpp` | Alta | P0 |
| Render | Niveles de calidad: Antialiased / Normal / Simple / Outline | Degradación progresiva para velocidad. | `Kernel/quality.cpp`, `Kernel/qualops.cpp`, `wxOil/xrc/SHARED_MENU.res` | Media | P1 |
| Render | Redibujado incremental por regiones | Sólo se redibujan las regiones invalidadas. | `Kernel/invalid.cpp`, `Kernel/region.cpp`, `wxOil/rendwnd.cpp`, `wxOil/rendbits.cpp` | Alta | P0 |
| Render | Render offscreen y double-buffering | Buffer fuera de pantalla para evitar parpadeo. | `wxOil/offscrn.cpp`, `wxOil/osrndrgn.cpp`, `wxOil/grnddib.cpp` | Media | P0 |
| Render | XOR / EOR rendering para blobs y arrastres | Dibujo de manejadores y previsualizaciones sin redibujar el documento. **Clave de la sensación de velocidad.** | `Kernel/blobs.cpp`, `wxOil/grndrgn.cpp`, `wxOil/handles.cpp`, `tools/rendsel.cpp` | Alta | P0 |
| Vista | Zoom: herramienta (`TOOL4`), in/out, a página, a dibujo, a selección, 100 % | `Ctrl+NumPad+/-`, `Ctrl+Shift+P/J/Z`, `Ctrl+R` (zoom anterior). | `tools/zoomtool.cpp`, `tools/zoomops.cpp` | Media | P0 |
| Vista | Push tool / pan (`TOOL3`, `Alt+9`, barra espaciadora) | Desplazamiento del lienzo. | `tools/pushtool.cpp`, `tools/pushbase.cpp` | Baja | P0 |
| Vista | Scroll y barras de desplazamiento | Scroll con rueda, barras, scroll durante arrastre. | `wxOil/scroller.cpp`, `wxOil/scrlbutn.cpp`, `wxOil/scrlthmb.cpp`, `wxOil/scrvw.cpp` | Media | P0 |
| Vista | Múltiples vistas del mismo documento | `WindowNewView`, tile, cascade, arrange. | `Kernel/view.cpp`, `Kernel/docview.cpp`, `Kernel/scrnview.cpp`, `wxOil/camview.cpp`, `wxOil/scrcamvw.cpp` | Alta | P2 |
| Vista | Pantalla completa (`NumPad 8`) | Modo sin decoraciones. | `Kernel/viewmenu.cpp` (`OPTOKEN_VIEWFULLSCREEN`) | Baja | P2 |
| Vista | Barra de calidad deslizante | Slider de calidad en la barra de estado. | `Kernel/qualops.cpp` (`OPTOKEN_QUALITYSLIDER`), recursos `IDB_QUALITY*` | Baja | P3 |
| Vista | Medición de tiempo de redibujado | `TimeDraw` (herramienta de desarrollo). | `wxOil/speedtst.cpp`, `Kernel/viewmenu.cpp` | Baja | P3 |

### 1.20 Infraestructura, plataforma y utilidades

| Área | Funcionalidad | Descripción | Fichero(s) C++ de referencia | Complejidad | Prioridad |
|---|---|---|---|---|---|
| Infra | Capa OIL (abstracción de SO/toolkit) | Separación limpia Kernel ↔ OIL; permitió portar de MFC a wxWidgets. Modelo a replicar en Rust (core sin GUI). | `wxOil/` completo, `Kernel/` sin dependencias de toolkit | Alta | P0 |
| Infra | Aritmética de punto fijo | `FIXED16`, `FIXED24`, `XLONG` para coordenadas exactas en millipuntos. | `Kernel/fixed.cpp`, `Kernel/fixed16.cpp`, `Kernel/fixed24.cpp`, `Kernel/fix24.cpp`, `Kernel/xlong.cpp` | Media | P0 |
| Infra | Matrices y geometría | `Matrix`, `Trans2DMatrix`, `DocCoord`, `DocRect`, `WorkCoord`. | `Kernel/matrix.cpp`, `Kernel/xmatrix.cpp`, `Kernel/trans2d.cpp`, `Kernel/doccoord.cpp`, `Kernel/docrect.cpp`, `Kernel/wrkcoord.cpp`, `Kernel/coord.cpp`, `Kernel/vector.cpp` | Media | P0 |
| Infra | Sistema de cadenas de longitud fija | `String_8/16/32/64/128/256` — evitar en Rust (usar `String`). | `wxOil/fixstr*.cpp`, `wxOil/basestr.cpp`, `wxOil/varstr.cpp` | Baja | — |
| Infra | Gestión de errores y excepciones | `ERROR2`, `ENSURE`, cajas de error localizadas. | `wxOil/errors.cpp`, `wxOil/errorbox.cpp`, `wxOil/ensure.cpp`, `wxOil/exceptio.cpp` | Media | P0 |
| Infra | Gestión de recursos y localización | Recursos XRC + `.res`, catálogos `po/` (gettext), 152 ficheros XRC. | `wxOil/camresource.cpp`, `wxOil/resources.cpp`, `wxOil/xrc/`, `po/` | Media | P1 |
| Infra | Progreso y operaciones largas | Barra de progreso, cancelación, cursores de espera. | `wxOil/progress.cpp`, `wxOil/progbar.cpp`, `wxOil/oilprog.cpp` | Baja | P1 |
| Infra | Gestión de ficheros y rutas | `PathName`, ficheros con buffer, archivos comprimidos. | `wxOil/pathname.cpp`, `wxOil/pathnmex.cpp`, `wxOil/fileutil.cpp`, `Kernel/ccfile.cpp`, `Kernel/ccbuffil.cpp`, `Kernel/ccafile.cpp`, `Kernel/archive.cpp` | Media | P0 |
| Infra | Portapapeles nativo e interno | Formato interno + intercambio con el sistema. | `wxOil/natclipm.cpp`, `wxOil/clipext.cpp`, `wxOil/fuzzclip.cpp` | Media | P1 |
| Infra | Soporte de tableta gráfica | Presión y entrada de tableta. | `wxOil/tablet.cpp`, `Kernel/pressure.cpp` | Media | P3 |
| Infra | Red / descargas HTTP | Descarga de bibliotecas, ayuda, actualizaciones. | `wxOil/camnet.cpp`, `wxOil/lddirect.cpp`, `wxOil/htmldwnd.cpp`, `wxOil/helpdownload.cpp` | Media | P3 |
| Infra | Ayuda en línea y "¿Qué es esto?" | `F1`, `OPTOKEN_WHATSTHIS`. | `Kernel/opwhat.cpp`, `wxOil/helpuser.cpp`, `wxOil/helptabs.cpp` | Baja | P3 |
| Infra | Diálogo Acerca de / registro / actualización | `AboutDlg`, `Register`, `Update`. | `Kernel/aboutdlg.cpp`, `Kernel/release.cpp`, `Kernel/inetop.cpp` | Baja | P3 |
| Infra | Herramientas de depuración | Árbol de nodos, árbol Xar, árbol CMX, blobby dialogs, pruebas de render, crash tests. | `Kernel/dbugtree.cpp`, `Kernel/debugdlg.cpp`, `Kernel/cxftree.cpp`, `Kernel/cmxtree.cpp`, `Kernel/blobby.cpp`, `Kernel/renddlg.cpp`, `Kernel/rnddlgs.cpp`, `wxOil/diagnost.cpp` | Media | P2 |
| Infra | Módulos y carga dinámica | Sistema de módulos (`CreateModule`), herramientas como plug-ins. | `Kernel/module.cpp`, `Kernel/modlist.cpp`, `tools/camtools.cpp`, `tools/viewmod.cpp` | Media | P3 |
| Infra | Biblioteca `xarlib` independiente | Lectura/expansión de ficheros Xar fuera de la app. | `xarlib/`, `xarlib/ExpandXar` | Media | P2 |

---

**Recuento**: 20 áreas, **335 funcionalidades catalogadas** (filas de la tabla maestra §1.1–§1.20).

---

## 2. MVP (P0) de la primera versión Linux en Rust

### 2.1 Objetivo del MVP

> **Un editor vectorial que abra y guarde `.xar` y SVG, dibuje y edite formas y caminos con la *sensación Xara* (selector unificado, edición de rellenos en canvas, redibujado instantáneo y antialiasing de calidad), con capas, texto básico y exportación a PNG/JPEG/SVG.**

El criterio de corte no es "cuántas funciones", sino **preservar la identidad de interacción de Xara**. Es preferible un conjunto pequeño de herramientas que se sientan exactamente como Xara, que un clon amplio y torpe.

### 2.2 Qué ENTRA en el MVP (P0)

| Bloque | Contenido |
|---|---|
| **Núcleo** | Árbol de nodos + atributos como nodos; sistema de operaciones con undo/redo granular; mensajería; coordenadas en millipuntos (punto fijo o `i64`). |
| **Documento** | Nuevo/Abrir/Guardar/Guardar como; una página con tamaño configurable; capas con visibilidad/bloqueo y galería de capas. |
| **Render** | Rasterizador propio en Rust con antialiasing de calidad (candidatos: `tiny-skia`, `vello`, o motor propio); redibujado incremental por regiones; overlay de manejadores desacoplado del documento (equivalente al render XOR). |
| **Herramientas** | Selector unificado (`TOOL7`) completo con doble estado escala/rotación; Rectángulo; Elipse; Bézier/Forma; Pluma; Mano alzada; Relleno; Transparencia; Zoom; Push/Pan. **10 herramientas.** |
| **Edición de caminos** | Nodos y manejadores; añadir/borrar puntos; línea/curva; suave/cúspide; cerrar; seleccionar todos los puntos; winding rule. |
| **Transformación** | Mover, escalar, rotar, sesgar, voltear, copiar-y-transformar, nudge por teclado (24 variantes), infobar numérica X/Y/W/H/ángulo con 9 anclas y bloqueo de aspecto. |
| **Estructura** | Agrupar/desagrupar; orden Z completo; alineación y distribución; cortar/copiar/pegar/pegar en sitio; duplicar; clonar; borrar; seleccionar todo/ninguno. |
| **Color** | RGB + HSV + escala de grises; colores de documento con nombre; editor de color; barra de color en pantalla con arrastre a objetos; "sin color". |
| **Relleno** | Plano, lineal, radial/circular, con **manejadores en el lienzo** y arrastre de colores de la paleta a las paradas; rampas multi-parada. |
| **Transparencia** | Plana y lineal/radial, modo Mix, con manejadores en el lienzo. |
| **Línea** | Grosor, color, transparencia, caps, joins, mitre limit, winding rule. |
| **Texto** | Texto simple y en columna; tipografía (FreeType/fontconfig), tamaño, negrita/cursiva/subrayado, justificación, interlineado; conversión a formas. |
| **Bitmaps** | Importar PNG/JPEG e insertar como nodo bitmap; galería de bitmaps; mover/escalar. |
| **Snapping** | Snap a rejilla y a guías; rejilla rectangular; reglas; guías arrastrables. |
| **Vista** | Zoom (herramienta + comandos), pan, calidad antialias/outline, scroll. |
| **E/S** | **Importar**: `.xar` (lectura completa), SVG, PNG, JPEG. **Exportar**: `.xar`, SVG, PNG, JPEG. |
| **Preferencias** | Sistema de prefs persistidas + unidades (los 12 tipos, parseo "10mm"/"1in" en todos los campos). |
| **UI** | Infobar contextual por herramienta; menús y atajos compatibles con los originales; menús contextuales; barra de estado con coordenadas. |

### 2.3 Qué NO entra en el MVP, y por qué

| Excluido | Justificación |
|---|---|
| **Blend, Contour, Mould, Bevel** | Cuatro subsistemas de complejidad *Muy alta* cada uno (interpolación de formas y atributos, deformación de envolvente/perspectiva con rellenos, offset de caminos robusto, iluminación 3D). Cada uno vale por sí solo una fase completa. No bloquean el uso diario de un editor vectorial. |
| **Shadow y Feather** | Se marcan **P1 destacado**: son de los diferenciadores más visibles de Xara y deben entrar inmediatamente después del MVP, pero requieren un pipeline de desenfoque gaussiano y render offscreen que no conviene atar antes de estabilizar el render base. |
| **Live Effects / XPE / plug-ins de bitmap** | Dependen de un ecosistema de plug-ins externos (XPE, plug-ins tipo Photoshop) que hoy no existe en Linux. Valor bajo, coste muy alto. |
| **Combinar formas (booleanas)** | *Muy alta* complejidad y alto riesgo de bugs de robustez numérica. P1: se aborda con una crate de booleanas ya probada en cuanto el modelo de camino esté cerrado. |
| **Pinceles, stroke types, presión/tableta** | Subsistema grande (`brsh*`, `strk*`, `pp*`) con poca demanda en un editor de ilustración técnica/diagramas; P2. |
| **Rellenos fractal/ruido, 3-4 colores, cónico, cuadrado** | Cola larga de tipos de relleno; el 90 % del uso es plano/lineal/radial/bitmap. P1–P3. |
| **Modos de transparencia avanzados (Contrast, Hue, Luminosity…)** | Dependen de operaciones de composición del motor CDraw que hay que reimplementar. P2, tras el pipeline de composición. |
| **Texto en camino, tabuladores, regla de texto, kerning fino** | Tipografía avanzada; el MVP cubre el texto que la gente necesita para etiquetar diagramas. P1–P2. |
| **Animación, fotogramas, GIF animado, rollovers, imagemaps, image slicing, HTML/SWF** | Todo el bloque "web 2005" está obsoleto. P3, probablemente **no se reimplementa nunca**; si acaso, exportación a SVG animado. |
| **Impresión, marcas de impresión, separación de color, PostScript** | En Linux lo natural es exportar a PDF y delegar en CUPS. P2 (exportar PDF) / P3 (preprensa). |
| **CDR, CMX, WMF/EMF, Acorn Draw, EPS/AI** | Importadores legados, *Muy alta* complejidad. P2 para EPS/AI (todavía hay ficheros vivos), P3 para el resto. Alternativa pragmática: delegar en librerías externas (`librevenge`/`libcdr`, Ghostscript). |
| **Galerías de clipart/rellenos/bibliotecas web** | Dependían de servidores de Xara que ya no existen. P3. |
| **Trazador de bitmap (auto-trace)** | *Muy alta*; hay alternativas externas (potrace). P2 vía integración. |
| **Múltiples vistas del mismo documento, multipágina/spreads** | Útiles pero no críticos para la primera versión; añaden complejidad al modelo de vista. P1–P2. |
| **Colores CMYK/spot y gestión de color** | Sólo relevantes con preprensa. P3. |

### 2.4 Riesgos del MVP

1. **El rasterizador.** CDraw es binario y propietario; su calidad de antialiasing y su velocidad son *el* diferenciador. Es el único elemento del MVP que puede hundir la percepción del producto. Hay que prototipar y medir (`vello` en GPU vs `tiny-skia` en CPU) **antes** de construir encima.
2. **El formato `.xar`.** Sin lectura fiable de `.xar` no hay continuidad con los documentos existentes. Los *record handlers* (`Kernel/rech*.cpp`, `Kernel/cxf*.cpp`) son la especificación de facto; `xarlib/` da un punto de entrada más pequeño para empezar.
3. **Atributos como nodos.** Es un modelo poco habitual (los atributos son hermanos previos, no propiedades del objeto). Reproducirlo o sustituirlo por un modelo de propiedades explícitas es una decisión arquitectónica que condiciona el import/export de `.xar`.

---

## 3. Diferenciadores de Xara — lo que hay que preservar

Estos son los rasgos que hacían a Xara sentirse distinto (y mejor) que Illustrator, CorelDRAW o Inkscape de la época. **Perderlos equivale a no haber reimplementado Xara.**

| # | Diferenciador | En qué consiste | Dónde está en el código | Prioridad de preservación |
|---|---|---|---|---|
| 1 | **Herramienta de selección unificada** | Una sola herramienta selecciona, mueve, escala, rota, sesga y edita rellenos. El segundo clic conmuta escala ⇄ rotación. No hay que cambiar de herramienta para operaciones frecuentes. | `tools/selector.cpp`, `tools/selinfo.cpp` | **Crítica (P0)** |
| 2 | **Edición directa e interactiva de rellenos en el lienzo** | Flechas y blobs del degradado sobre el propio objeto; arrastrar un color de la paleta al extremo de la flecha cambia esa parada; nudge del relleno con teclado. Nadie más hacía esto en 1995. | `tools/filltool.cpp`, `Kernel/opgrad.cpp`, `Kernel/fillndge.cpp`, cursores `IDC_CANDROPONFILL*` | **Crítica (P0)** |
| 3 | **Velocidad de redibujado** | Redibujado incremental por regiones + caché por nodo + render XOR de los manejadores: el arrastre es instantáneo aunque el dibujo sea complejo. Xara presumía de redibujar más rápido que la competencia en un orden de magnitud. | `Kernel/invalid.cpp`, `Kernel/region.cpp`, `Kernel/nodecach.cpp`, `wxOil/grndrgn.cpp`, `wxOil/rendwnd.cpp` | **Crítica (P0)** |
| 4 | **Calidad de antialiasing** | El rasterizador CDraw producía bordes visiblemente más limpios que los competidores, con antialias activo por defecto en pantalla. | `GDraw/`, `libs/`, `Kernel/GDrawIntf.cpp`, `Kernel/quality.cpp` | **Crítica (P0)** |
| 5 | **Formas paramétricas vivas** | Rectángulos redondeados, elipses y QuickShapes siguen siendo editables como parámetros (radio, nº de lados, estelación, curvatura) indefinidamente; sólo se convierten a camino cuando el usuario lo pide. | `Kernel/noderect.cpp`, `Kernel/nodeelip.cpp`, `Kernel/nodershp.cpp` | Alta (P0/P1) |
| 6 | **Sombras suaves en tiempo real** | Sombra de pared/suelo/glow con penumbra arrastrable, recalculada en vivo mientras se arrastra. En 1999 esto era casi mágico. | `tools/shadtool.cpp`, `Kernel/nodeshad.cpp`, `Kernel/bshadow.cpp` | Alta (P1) |
| 7 | **Transparencias avanzadas** | Transparencia graduada con las mismas geometrías que los rellenos, más modos tipo *blend mode* (Stained Glass, Bleach, Contrast, Luminosity, Hue…). | `Kernel/fillattr.cpp`, `Kernel/fillval.h`, `GDraw/gdraw.h` | Alta (P1/P2) |
| 8 | **Blend vectorial de calidad** | Interpolación de forma, color, atributos y posición, con perfiles bias/gain independientes y blend sobre camino. | `Kernel/nodeblnd.cpp`, `Kernel/gblend.cpp`, `tools/blndtool.cpp` | Media-alta (P2) |
| 9 | **Moulds (envolvente y perspectiva)** | Deformación de cualquier objeto o grupo por envolvente Bézier o perspectiva, con el relleno deformándose coherentemente. | `Kernel/nodemold.cpp`, `Kernel/moldenv.cpp`, `Kernel/moldpers.cpp`, `Kernel/gmould.cpp` | Media-alta (P2) |
| 10 | **Colores con nombre y colores vinculados** | Editar un color del documento actualiza todos sus usos; los colores derivados (tinte/sombra/matiz) se recalculan en cascada. Diseño de paletas coherentes sin esfuerzo. | `Kernel/doccolor.cpp`, `Kernel/colcomp.cpp`, `Kernel/coldlog.cpp` | Alta (P1) |
| 11 | **Feather (desvanecido de bordes) sobre cualquier objeto** | Un atributo, no un efecto destructivo: se aplica a formas, grupos y texto, y sigue siendo editable. | `Kernel/opfeathr.cpp`, `Kernel/fthrattr.cpp` | Alta (P1) |
| 12 | **ClipView** | Recorte vivo de un grupo por la forma superior, sin destruir nada. | `Kernel/nodeclip.cpp`, `tools/opclip.cpp` | Media-alta (P1) |
| 13 | **Selección dentro de grupos sin desagrupar** | `Ctrl`+clic entra al objeto hoja; `Alt`+clic selecciona el objeto de debajo. Cursores distintos indican el modo. | `tools/selector.cpp`, `Kernel/hittest.cpp` | Alta (P0) |
| 14 | **Snap magnético a objetos** | Ajuste a puntos y bordes de otros objetos con realimentación de cursor, activable/desactivable en pleno arrastre con el teclado numérico. | `Kernel/snap.cpp`, `Kernel/snapops.cpp` | Alta (P1) |
| 15 | **Modificadores de teclado activos durante el arrastre** | `Ctrl` (constrain), `Shift` (adjust), `Alt` (alternative) cambian el comportamiento **mientras** se arrastra, no sólo al empezar. Incluye conmutar snap en medio de un arrastre (`WorksInDrag`). | `Kernel/input.cpp`, `wxOil/clikmods.cpp`, `wxOil/keypress.cpp`, `STANDARD_HOTKEYS.res` | **Crítica (P0)** |
| 16 | **Infobar contextual con campos numéricos que aceptan unidades** | Cualquier campo acepta "10mm", "1in", "3p6"; los botones de *bump* permiten incrementos finos sin teclear. | `Kernel/infobar.cpp`, `Kernel/units.cpp`, `tools/*info.cpp` | Alta (P0) |
| 17 | **Todo es una operación con nombre** | Cada acción tiene un `OPTOKEN`, descripción legible, undo granular y puede colgarse de menú, barra, atajo o menú contextual sin código extra. | `Kernel/ops.cpp`, `Kernel/opdesc.cpp` | **Crítica (P0)** |
| 18 | **Tamaño y arranque** | Xara era pequeño y arrancaba al instante frente a suites enormes. Un binario Rust compacto y de arranque rápido conserva esa ventaja competitiva. | (propiedad global del diseño) | Alta (P0) |

---

## 4. Atajos de teclado y modelo de interacción

Extraídos de `wxOil/xrc/STANDARD_HOTKEYS.res` (142 líneas) y `wxOil/xrc/SHARED_MENU.res`.

### 4.1 Modificadores (nomenclatura interna de Xara)

| Nombre Xara | Tecla | Semántica típica |
|---|---|---|
| `Constrain` | `Ctrl` | Restringir (cuadrado/círculo, ángulos múltiplos, proporción) y prefijo de comandos |
| `Adjust` | `Shift` | Ajustar / añadir a la selección / variante del comando |
| `Alternative` | `Alt` | Modo alternativo (seleccionar objeto de debajo, nudge en píxeles, cambio de herramienta) |
| `Extended` | — | Marca de tecla extendida del teclado |
| `WorksInDrag` | — | El atajo sigue activo **durante** un arrastre (snapping) |
| `CheckUnicode` | — | El atajo depende de la disposición de teclado |

### 4.2 Fichero, edición y documento

| Atajo | Comando | Atajo | Comando |
|---|---|---|---|
| `Ctrl+N` | Nuevo dibujo | `Ctrl+Shift+N` | Nueva animación |
| `Ctrl+O` | Abrir | `Ctrl+W` | Cerrar |
| `Ctrl+S` | Guardar | `Ctrl+P` | Imprimir |
| `Ctrl+Shift+I` | Importar | `Ctrl+Shift+E` | Exportar |
| `Ctrl+Shift+Y` | Importar desde URL | `Ctrl+Shift+O` | Opciones |
| `Ctrl+Z` / `<` | Deshacer | `Ctrl+Y` / `>` | Rehacer |
| `Ctrl+X` / `Backspace` / `Shift+Del` | Cortar | `Ctrl+C` / `Ctrl+Ins` | Copiar |
| `Ctrl+V` / `Ins` / `Shift+Ins` | Pegar | `Ctrl+Shift+V` | Pegar en la misma posición |
| `Ctrl+Shift+A` | Pegar atributos | `Del` | Borrar |
| `Ctrl+A` | Seleccionar todo | `Esc` | Deseleccionar todo |
| `Ctrl+D` | Duplicar | `Ctrl+K` | Clonar |
| `Return` / `Enter` | Editar selección | `Ctrl+Tab` / `Ctrl+Shift+Tab` | Documento siguiente / anterior |

### 4.3 Organizar (Arrange)

| Atajo | Comando |
|---|---|
| `Ctrl+F` | Traer al frente |
| `Ctrl+Shift+F` | Mover hacia delante |
| `Ctrl+Shift+B` | Mover hacia atrás |
| `Ctrl+B` | Enviar al fondo |
| `Ctrl+Shift+U` | Mover una capa arriba |
| `Ctrl+Shift+D` | Mover una capa abajo |
| `Ctrl+G` | Agrupar |
| `Ctrl+U` | Desagrupar |
| `Ctrl+Shift+L` | Diálogo de alineación |
| `Ctrl+1` / `Ctrl+2` / `Ctrl+3` / `Ctrl+4` | Combinar: Añadir / Restar / Intersectar / Cortar |
| `Ctrl+Shift+S` | Convertir a formas |
| `Ctrl+Shift+C` | Convertir a bitmap |
| `Ctrl+Shift+R` | Invertir camino del texto |

### 4.4 Vista, zoom, rejilla y snap

| Atajo | Comando |
|---|---|
| `Ctrl+NumPad +` / `Ctrl+NumPad −` | Zoom in / out |
| `Ctrl+Shift+P` | Zoom al spread |
| `Ctrl+Shift+J` | Zoom al dibujo |
| `Ctrl+Shift+Z` | Zoom a la selección |
| `Ctrl+R` | Zoom anterior |
| `Ctrl+L` | Mostrar/ocultar reglas |
| `#` | Mostrar/ocultar rejilla |
| `NumPad 1` | Mostrar/ocultar guías |
| `NumPad .` | Snap a rejilla *(activo durante el arrastre)* |
| `NumPad 2` | Snap a guías *(activo durante el arrastre)* |
| `NumPad *` | Snap a objetos *(activo durante el arrastre)* |
| `NumPad 8` | Pantalla completa |

### 4.5 Nudge (24 combinaciones)

| Modificador | Paso |
|---|---|
| *(ninguno)* | 1 unidad de nudge |
| `Ctrl` | ×5 |
| `Shift` | ×10 |
| `Ctrl+Shift` | 1/5 del paso |
| `Alt` | 1 píxel |
| `Alt+Shift` | 10 píxeles |

Cada uno en las 4 direcciones (`↑ ↓ ← →`). Existen tres familias paralelas de operaciones de nudge: objetos (`OPTOKEN_NUDGE*`), puntos de camino (`OPTOKEN_PATHNUDGE*`) y rellenos (`OPTOKEN_FILLNUDGE*`).

### 4.6 Herramientas

| Atajo | Herramienta | ID |
|---|---|---|
| `F2` / `Alt+S` / `Espacio` | Selector | `TOOL7` |
| `F3` | Mano alzada (Freehand) | `TOOL6` |
| `F4` | Bézier / Forma | `TOOL11` |
| `F5` / `Alt+1` | Relleno graduado | `TOOL13` |
| `F6` / `Alt+2` | Transparencia | `TOOL17` |
| `F7` / `Alt+6` | Blend | `TOOL16` |
| `F8` | Texto | `TOOL21` |
| `Shift+F2` | QuickShape | `TOOL18` |
| `Shift+F3` | Rectángulo | `TOOL5` |
| `Shift+F4` | Elipse | `TOOL12` |
| `Shift+F5` | Pluma | `TOOL14` |
| `Shift+F6` / `Alt+7` | Mould | `TOOL19` |
| `Shift+F7` / `Alt+0` / `Alt+Z` | Zoom | `TOOL4` |
| `Shift+F8` / `Alt+9` / `Alt+X` | Push (pan) | `TOOL3` |
| `Ctrl+F2` / `Alt+3` | Sombra | `TOOL22` |
| `Ctrl+F3` / `Alt+4` | Bisel | `TOOL23` |
| `Ctrl+F5` | Live Effects | `TOOL26` |
| `Ctrl+F7` / `Alt+5` | Contorno | `TOOL24` |
| `Ctrl+F8` / `Alt+8` | Slice | `TOOL25` |
| *(sin atajo)* | Rejilla | `TOOL10` |

> **`ToolSwitch`**: `Alt+X`, `Alt+Z`, `Alt+S` y la **barra espaciadora** son cambios *momentáneos* de herramienta — al soltar la tecla se vuelve a la anterior. La barra espaciadora activa el Selector temporalmente. Este patrón es parte esencial de la fluidez de Xara y debe reproducirse.

### 4.7 Galerías

| Atajo | Galería | Atajo | Galería |
|---|---|---|---|
| `F9` | Colores | `Shift+F9` | Fuentes |
| `F10` | Capas | `Shift+F10` | Clipart |
| `F11` | Bitmaps | `Shift+F11` | Rellenos |
| `F12` | Líneas | `Shift+F12` | Fotogramas |
| `Ctrl+Shift+F9` | Nombres | | |

### 4.8 Otros

| Atajo | Comando |
|---|---|
| `Ctrl+E` | Cuentagotas / selector de color |
| `Ctrl+I` | Image slice |
| `Ctrl+Shift+W` | Diálogo de dirección web |
| `F1` | Índice de ayuda |

### 4.9 Modelo de interacción principal

1. **Todo ocurre en el lienzo.** Los diálogos modales son la excepción: rellenos, transparencias, sombras, biseles, contornos y moulds se editan con manejadores sobre el objeto.
2. **La infobar es el panel de precisión.** Cada herramienta publica su barra con los mismos parámetros que sus manejadores, en forma numérica y con unidades.
3. **Herramienta ↔ objeto.** Doble clic sobre un objeto activa la herramienta que lo creó (un rectángulo abre la herramienta Rectángulo, un texto la de Texto). `Return` = "editar selección".
4. **Modificadores vivos.** `Ctrl`/`Shift`/`Alt` cambian el comportamiento durante el arrastre; el snap se conmuta sin soltar el ratón.
5. **Cambio momentáneo de herramienta.** Barra espaciadora (selector), `Alt+X` (push), `Alt+Z` (zoom).
6. **Arrastrar y soltar como verbo universal.** Colores de la paleta a objetos y a paradas de degradado; ítems de galería al lienzo; ficheros al lienzo.
7. **Todo es deshacible y tiene nombre.** El menú Deshacer describe la última acción con su nombre real.

---

## 5. Formatos de importación y exportación con prioridad

### 5.1 Importación

| Formato | Ext. | Vector/Bitmap | Implementación original | Prioridad | Estrategia recomendada en Rust |
|---|---|---|---|---|---|
| Xara nativo v2 | `.xar` | Vector | `Kernel/native.cpp`, `cxf*.cpp`, `rech*.cpp` | **P0** | Parser propio de records + `flate2`; `xarlib/` como referencia mínima |
| SVG | `.svg`, `.svgz` | Vector | `filters/SVGFilter/svgimporter.cpp` | **P0** | `usvg`/`resvg` como base, adaptando al modelo de nodos |
| PNG | `.png` | Bitmap | `wxOil/pngfiltr.cpp` | **P0** | `image` / `png` crate |
| JPEG | `.jpg`, `.jpeg` | Bitmap | `Kernel/imjpeg.cpp` | **P0** | `image` / `jpeg-decoder` |
| Xara web | `.web` | Vector | `Kernel/webfiltr.cpp` | P2 | Variante del parser `.xar` |
| GIF (incl. animado) | `.gif` | Bitmap | `wxOil/giffiltr.cpp` | P1 | `image` / `gif` crate |
| BMP / DIB | `.bmp`, `.dib` | Bitmap | `wxOil/imgmgkft.cpp`, `Kernel/bitfilt.cpp` | P1 | `image` |
| TIFF | `.tif`, `.tiff` | Bitmap | ImageMagick externo | P2 | `image` (feature tiff) |
| WebP / AVIF | — | Bitmap | *(no existía)* | P1 | `image` — **añadido moderno recomendado** |
| PDF | `.pdf` | Vector/Bitmap | ImageMagick (rasterizado) | P2 | `pdfium`/`mupdf` binding, o rasterizado |
| PSD | `.psd` | Bitmap | ImageMagick externo | P3 | Opcional |
| PNM / PPM / PGM / PBM | `.ppm`… | Bitmap | `wxOil/ppmfiltr.cpp`, ImageMagick | P3 | `image` |
| XPM / ICO / PCD / PICT | varios | Bitmap | ImageMagick externo | P3 | `image` donde exista |
| EPS genérico | `.eps` | Vector | `Kernel/epsfiltr.cpp` | P2 | Delegar en Ghostscript, o intérprete parcial |
| Adobe Illustrator (AI/AI5/AI8) | `.ai` | Vector | `Kernel/ai_eps.cpp`, `ai5_eps.cpp`, `ai8_eps.cpp` | P2 | AI moderno = PDF ⇒ vía ruta PDF |
| EPS de Photoshop / FreeHand / ArtWorks | `.eps` | Vector | `Kernel/coreleps.cpp`, `freeeps.cpp`, `aw_eps.cpp` | P3 | Descartable |
| CorelDRAW | `.cdr` | Vector | `Kernel/cdrfiltr.cpp` + `cdr*.cpp` | P3 | `libcdr` vía FFI si se pide |
| Corel CMX 16/32 | `.cmx` | Vector | `Kernel/cmx*.cpp` (9 ficheros) | P3 | Descartable |
| WMF / EMF | `.wmf`, `.emf` | Vector | `wxOil/metafilt.cpp` | P3 | `libwmf` vía FFI si se pide |
| Acorn Draw / Sprite | `.aff` | Vector/Bitmap | `Kernel/drawfltr.cpp` | P3 | Descartable |
| Paletas (MS, PSP, Corel, ACO/ACT, JCW) | varios | Paleta | `Kernel/impcol.cpp` | P3 | `.ase`/`.gpl` modernos en su lugar |
| Texto ANSI / Unicode / RTF | `.txt`, `.rtf` | Texto | `Kernel/textfltr.cpp` | P3 | Sólo texto plano (P2) |
| Importación desde URL | — | Cualquiera | `Kernel/urlimp.cpp` | P3 | Descartable (drag&drop del navegador basta) |

### 5.2 Exportación

| Formato | Ext. | Opciones del original | Prioridad | Estrategia recomendada en Rust |
|---|---|---|---|---|
| Xara nativo v2 | `.xar` | Compresión, miniatura incrustada, versión | **P0** | Escritor de records + `flate2` |
| SVG | `.svg` | Diálogo de opciones propio | **P0** | Serializador propio (control total sobre fidelidad) |
| PNG | `.png` | Profundidad, alfa, entrelazado, paleta, DPI, antialias, ámbito | **P0** | `png` crate |
| JPEG | `.jpg` | Calidad, progresivo, DPI | **P0** | `jpeg-encoder` |
| PDF | `.pdf` | *(no existía nativamente)* | **P1** | **Añadido moderno prioritario**: sustituye a EPS/PostScript y a la impresión |
| GIF | `.gif` | Paleta, dithering, transparencia, entrelazado | P1 | `gif` crate + cuantización |
| BMP / DIB | `.bmp` | Profundidad | P2 | `image` |
| TIFF | `.tif` | Vía ImageMagick | P2 | `image` |
| WebP | `.webp` | *(no existía)* | P2 | **Añadido moderno** |
| Xara web | `.web` | Formato reducido | P3 | Descartable |
| EPS / PostScript | `.eps`, `.ps` | Prólogo PS, separación, marcas | P3 | Sustituido por PDF |
| Flash / SWF | `.swf` | Formas, texto, bitmaps, sprites, botones | P3 | **No reimplementar** (formato muerto) |
| HTML + imagemap | `.html` | `<map>` con áreas, URLs por objeto | P3 | **No reimplementar** |
| Image slicing (HTML + trozos) | — | Cortes por objetos nombrados + rollovers | P3 | **No reimplementar** |
| GIF animado | `.gif` | Retardos, bucle, disposal, paleta común | P3 | Considerar SVG/APNG animado en su lugar |
| Paleta de colores | varios | Volcado de la paleta del documento | P3 | `.gpl` / `.ase` |

### 5.3 Resumen numérico de formatos

- **Importación**: **24 familias de formato** catalogadas (4 en P0, 3 en P1, 6 en P2, 11 en P3). Contando las variantes concretas de bitmap que el original enchufaba vía ImageMagick, el catálogo nominal supera los **50 formatos** (`FILTERID_*` define 130+ identificadores, la mayoría comentados en el build de Linux).
- **Exportación**: **16 familias de formato** (4 en P0, 2 en P1, 3 en P2, 7 en P3), más 2 añadidos modernos recomendados (PDF, WebP).

---

## 6. Recuento de herramientas

El original define **26 slots de herramienta** (`TOOLID_1..26` en `Kernel/tool.h`), de los cuales **20 están implementados** en `tools/`:

| # | Herramienta | ID | Fichero | Prioridad |
|---|---|---|---|---|
| 1 | Selector | `TOOL7` | `tools/selector.cpp` | P0 |
| 2 | Rectángulo | `TOOL5` | `tools/rectangl.cpp` | P0 |
| 3 | Elipse | `TOOL12` | `tools/eliptool.cpp` | P0 |
| 4 | QuickShape (polígono/estrella) | `TOOL18` | `tools/regshape.cpp` | P1 |
| 5 | Bézier / Forma | `TOOL11` | `tools/beztool.cpp` | P0 |
| 6 | Pluma | `TOOL14` | `tools/pentool.cpp` | P0 |
| 7 | Mano alzada | `TOOL6` | `tools/freehand.cpp` | P0 |
| 8 | Texto | `TOOL21` | `tools/texttool.cpp` | P0 |
| 9 | Relleno graduado | `TOOL13` | `tools/filltool.cpp` (`GradFillTool`) | P0 |
| 10 | Transparencia | `TOOL17` | `tools/filltool.cpp` (`TranspTool`) | P0 |
| 11 | Blend | `TOOL16` | `tools/blndtool.cpp` | P2 |
| 12 | Mould | `TOOL19` | `tools/moldtool.cpp` | P2 |
| 13 | Contorno | `TOOL24` | `tools/cntrtool.cpp` | P2 |
| 14 | Sombra | `TOOL22` | `tools/shadtool.cpp` | P1 |
| 15 | Bisel | `TOOL23` | `tools/bevtool.cpp` | P2 |
| 16 | Live Effects | `TOOL26` | `tools/liveeffectstool.cpp` | P3 |
| 17 | Slice (web) | `TOOL25` | `tools/slicetool.cpp` | P3 |
| 18 | Zoom | `TOOL4` | `tools/zoomtool.cpp` | P0 |
| 19 | Push / Pan | `TOOL3` | `tools/pushtool.cpp` | P0 |
| 20 | Rejilla | `TOOL10` | `tools/gridtool.cpp` | P3 |
| — | Blank (plantilla de desarrollo) | `TOOL15` | `tools/blnktool.cpp` | — |
| — | Test / Rect / Rotate / Accusoft (slots históricos sin implementar en LX) | `TOOL2/8/9/20` | — | — |

Funciones que en Xara **no son herramientas** pero que otros programas sí exponen como tales, y que hay que colocar en la UI de la reimplementación:

- **Cuentagotas** → comando `Ctrl+E` + cursores dedicados (`wxOil/dragpick.cpp`).
- **Borrador** → modo *rub-out* dentro de la herramienta de mano alzada (`tools/freehand.cpp`).
- **Pincel** → atributo de línea (galería de líneas + `Kernel/brshattr.cpp`), no herramienta separada.
- **Recorte / clip** → operación ClipView (`tools/opclip.cpp`), no herramienta.
- **Cotas / medición** → no existe en Xara LX; las reglas, guías y la infobar numérica cubren el caso.

---

## 7. Notas para la planificación

1. **Orden de ataque sugerido**: (a) modelo de nodos + operaciones + undo; (b) render y overlay de manejadores; (c) selector + formas + caminos; (d) rellenos/transparencias con edición en canvas; (e) `.xar` lectura, luego escritura; (f) SVG; (g) texto; (h) capas y galerías; (i) sombra y feather; (j) el resto por prioridad.
2. **La capa OIL es un buen precedente**: mantener el *core* en Rust puro sin dependencias de GUI permite tests headless, renderizado a fichero y, más adelante, otros frontends.
3. **`Kernel/rech*.cpp` es la documentación del formato `.xar`**: cada fichero es el *record handler* de una familia de records (`rechrect`, `rechellp`, `rechtext`, `rechbmp`, `rechcol`, `rechattr`, `rechpoly`, `rechrshp`, `rechprnt`, `rechunit`, `rechdoc`, `rechinfo`, `rechsmth`, `rechcomp`). Son el punto de partida para escribir la especificación del parser en Rust.
4. **No reimplementar**: SWF, HTML/imagemap, image slicing, rollovers, GIF animado, CMX, CDR, Acorn Draw, PostScript/preprensa, plug-ins XPE. Son ~15 % del código original y ~0 % del valor actual.
