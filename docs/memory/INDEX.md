# Índice de memoria del proyecto

Notas persistentes por subsistema. Cada agente que trabaje en un subsistema
**debe** leer su nota antes y actualizarla después.

| Nota | Subsistema | Cubre |
|---|---|---|
| `xar-import.md` | Importador `.xar` | Tags implementados, rarezas del formato, ficheros de prueba que fallan |
| `document-model.md` | Modelo de documento | Arena de nodos, atributos, invariantes, undo |
| `render.md` | Motor de render | Pipeline, blend modes, caché, decisiones GPU/CPU |
| `xarast-format.md` | Formato nativo | Versionado, round-trip, extensiones SVG |
| `ui.md` | Interfaz | Toolkit, layout de paneles, atajos, Wayland |
| `packaging.md` | Empaquetado | AppImage, CI, compatibilidad glibc |
| `text.md` | Texto | Shaping, fuentes, texto en path |
| `perf.md` | Rendimiento | Benchmarks, presupuestos, regresiones |

## Estado

Las notas se crean conforme arrancan las fases. Si una nota no existe todavía,
créala con la plantilla:

```markdown
# <subsistema>

## Estado actual
## Decisiones tomadas (y por qué)
## Invariantes que NO se pueden romper
## Callejones sin salida (no volver a intentar)
## Pendiente / TODO
```
