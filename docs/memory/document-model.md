# document-model

Nota de memoria del subsistema **modelo de documento**.
Investigación completa en [`../research/02-modelo-documento.md`](../research/02-modelo-documento.md).

## Estado actual

- Investigación del modelo de Xara LX **terminada**: jerarquía de nodos, árbol
  documento/capítulo/spread/página/capa, sistema de atributos, rellenos,
  compuestos «live», texto, bitmaps, selección y undo.
- **No hay código Rust escrito todavía.** La propuesta de diseño (§10 del
  documento de investigación) está lista para implementarse.

## Decisiones tomadas (y por qué)

1. **Arena `slotmap::SlotMap<NodeId, NodeData>` + `enum NodeKind`**, no herencia,
   no trait objects, no ECS. El documento es un árbol jerárquico con orden de
   pintado estricto; el ECS no encaja con el ámbito léxico de atributos ni con
   el orden. Los ~60 predicados `IsXxx()` virtuales de `node.h:460-504` son la
   prueba de que la jerarquía estaba supliendo la falta de *sum types*.
2. **Lista enlazada de hermanos** (`parent/prev/next/first_child/last_child`),
   no `Vec<NodeId>`: insertar/borrar/mover/reordenar son las operaciones
   dominantes en un editor vectorial y son O(1). Añadimos `last_child`, que
   Xara no tiene (recorre), porque el importador hace append masivo.
3. **`NodeHidden` desaparece.** Borrar = `detach` + flag `DETACHED`; el nodo
   sigue vivo en la arena y lo retiene la `Transaction` del historial. Esto
   elimina `HiddenRefCnt`, `FindNextNonHidden`, `IsOrHidesAnAttribute`,
   `HidingNode/ShowingNode/ComplexHide` y el
   `KernelBitmapRef::RemoveFromTree/AddtoTree`.
4. **Los atributos SIGUEN SIENDO NODOS** (`NodeKind::Attr`). Decisión discutida
   y firme: es la única representación que preserva el ámbito léxico
   («este atributo afecta a los hermanos siguientes y a sus subárboles»), que es
   exactamente la semántica de `.xar` **y** de SVG (`<g>` con propiedades de
   presentación). Un mapa de atributos por nodo rompería el round-trip.
   Encima se pone un `AttrResolver` con caché, que da consultas O(1).
5. **`AttrStack` = réplica exacta de `CurrentAttrs` + `RenderStack`**: tabla
   densa indexada por `AttrSlot`, undo-log de `(slot, valor_anterior)` y marcas
   de nivel. `push_scope()`/`pop_scope()` al bajar/subir de una lista de hijos.
   ~60 líneas sustituyen a `rndrgn.cpp:7000-7150` + `rndstack.cpp`.
6. **Un solo `FillGeometry<S: Stop>` genérico** en vez de la duplicación
   color/transparencia (≈40 clases en `fillattr2.h` + 20 en `fillval.h`).
   `Perspective` es `Option<…>`, no dos puntos + un `BOOL IsPersp`.
7. **Undo = log de acciones inversas** (modelo de Xara), NO estructura
   persistente como almacén vivo. Ver «discrepancia» abajo.
8. **La selección sale de los nodos**: `IndexSet<NodeId>` en `EditState`, no un
   bit en `NodeFlags`. Igual el cursor de texto (`CaretNode` fuera del árbol) y
   el punto de inserción (`InsertionNode` fuera del árbol).
9. **La selección de puntos de control sale del `PathData`** a un overlay
   `HashMap<NodeId, BitVec>`: así la geometría es comparable con `==` y el COW
   del `Arc<PathData>` no se rompe al seleccionar un punto.
10. **Cachés unificadas bajo «clave derivada del estado»**: `BoundsCache` con
    epoch por nodo y propagación hacia arriba con corte temprano; `RasterKey`
    con `state_hash` en vez de los `m_Last*` de `NodeShadow`/`NodeBevel`.
11. **`xarast-model` sin dependencias gráficas.** Requisito para testear sin GPU
    y para el fuzzer del importador en CI.

## Discrepancia abierta con `research/05-stack-tecnologico.md §9`

05 propone que el documento **vivo** sea la estructura persistente (`imbl`).
Aquí se propone lo inverso: **arena viva + snapshot persistente periódico**.

- Motivo: el recorrido de render/hit-test/formateo accede por ID millones de
  veces por frame; `SlotMap` es indexación directa, un HAMT son 2–5 saltos con
  fallos de caché.
- Los objetivos de 05 (undo O(1), ramas de historial, «deshacer tras reabrir»)
  se cumplen con checkpoints `imbl::HashMap<NodeId, Arc<NodeData>>` cada N
  transacciones + log de acciones entre ellos, que además permite presupuestar
  memoria en bytes (como `OperationHistory::MaxSize`).
- **Acción pendiente:** microbenchmark de recorrido completo de un documento de
  100 000 nodos en ambas representaciones antes de cerrarlo en
  `docs/10-arquitectura.md`.

## Invariantes que NO se pueden romper

1. Árbol acíclico; enlaces `next`/`prev` recíprocos; todos los hijos apuntan al
   mismo padre; `first_child` sin `prev` y `last_child` sin `next`.
2. `DETACHED` es transitivo hacia abajo (nada bajo un nodo desvinculado es
   alcanzable desde la raíz).
3. Todo nodo `LiveRole::Generated` tiene un ancestro `LiveRole::Controller` del
   mismo `LiveKind`, y cada controlador tiene exactamente un subárbol `Source`.
4. Bloque de atributos antes del primer nodo ink dentro de una lista de hijos
   (*deseable*, no obligatorio: **el importador debe aceptar ficheros que lo
   violen** — hay `.xar` reales así).
5. Un spread, exactamente una capa activa.
6. `Tag` único y estable por documento; `by_tag` biyectivo con los nodos vivos.
7. Si la caja de un nodo es inválida, la de todos sus ancestros también.
8. Todo `BitmapId`/`PaletteId`/`BrushId` referenciado existe en
   `DocumentResources`.
9. La selección solo contiene nodos alcanzables desde la raíz.
10. `TextItem` solo bajo `TextLine`; `TextLine` solo bajo `TextStory`.
11. Coordenadas siempre en millipoints `i32`, nunca `f64`, en el modelo. El
    único `f32` admitido es el valor de color y la posición `0..1` de las
    paradas de rampa.

## Callejones sin salida (no volver a intentar)

- **Traducir la jerarquía con `Box<dyn Node>` + `Any`**: reproduce el
  downcasting constante del original y hace imposible el `match` exhaustivo.
- **`Rc<RefCell<Node>>`**: pánicos por `BorrowMut` en los recorridos que suben
  y bajan (el render sube al padre después de los hijos), ciclos padre↔hijo,
  no `Send`.
- **Mapa de atributos por nodo sin nodos de atributo**: pierde el ámbito de
  lista y rompe el round-trip con `.xar` y con SVG. Descartado tras análisis.
- **`Epoch` global para invalidar cajas**: invalida todo en cada edición, peor
  que el `InvalidateBoundingRect` dirigido de Xara. Usar epoch por nodo con
  propagación hacia arriba y corte temprano.
- **Meter el cursor de texto, la selección o el punto de inserción en el
  árbol** (como hace Xara con `CaretNode`, `NodeFlags::Selected` y
  `InsertionNode`): contamina undo, serialización, copia y recorridos.

## Pendiente / TODO

- [ ] Microbenchmark arena vs. `imbl` (ver «discrepancia»).
- [ ] Cerrar el conjunto exacto de `AttrSlot` cotejándolo con los 211 tags de
      `Kernel/cxftags.h` (ver `research/01-formato-xar.md`).
- [ ] Decidir si los pasos intermedios de un blend se materializan como nodos
      o se generan en el render (Xara hace lo segundo; afecta al hit-test).
- [ ] Definir `ProceduralSource` (fractal/noise) y su hash de caché.
- [ ] `Tree::validate()` + property tests con `proptest` sobre secuencias de
      attach/detach/move — antes de escribir el importador.
- [ ] Test de tamaño: `size_of::<NodeData>() <= 64`.
- [ ] Decidir el modelo de `MouldGeometry` (trait vs. enum) al implementar
      `xarast-live`.
