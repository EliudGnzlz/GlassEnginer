# REGISTRO — Fase 9: escena escaparate (PBR + toon, lluvia, tormenta de arena, portal en vivo)

**Cerrada el 2026-11-12.** Última fase del plan de 12 del `ANALISIS_CONSOLIDADO.md` que quedaba por ejecutar (las fases 10 y 11 siguen aparcadas por orden de D3m3tr10). Sin tools MCP propias (así lo fija el anexo §8 del consolidado).

## Entrega

- **Escena `assets/scenes/Fase9Escaparate.scene.json`** (39 entidades): sala de 34 × 26 m con avenida de materiales en dos estilos — fila **PBR** (4 esferas de colores sobre pedestales + cubo con `graph:tinte_demo`) y fila **toon** (esfera, cubo, cápsula y cilindro con `graph:toon_demo` y su pase Outline) —, plano de agua (`graph:agua`), rincón de fuego (partículas 8P + luz cálida), haz láser (`graph:laser`) entre postes, 3 luces puntuales, sol con sombra y niebla en la cámara.
- **Clima automático** (script `EscaparateControl.cs`, paquete DeepSeek): ciclo LLUVIA 24 s ↔ TORMENTA DE ARENA 24 s de tiempo de script, sin recargar la escena. LLUVIA: emisor de caja sobre la sala (2.600/s, gotas estiradas). TORMENTA: emisor lateral con viento +X, muro de polvo con `graph:tormenta_arena` y sol atenuado a 0,45.
- **Portal en vivo**: par «escaparate» en el muro divisorio; la bola sonda naranja se relanza sola hacia el portal y el script anuncia cada cruce (`[ESCAPARATE] sonda cruzó el portal`).
- **Documentación**: `docs/ESCAPARATE.md` (paquete Kimi).
- **Decisión de construcción**: los efectos NO necesitaron código de motor nuevo — son datos del sistema de partículas de la 8P y grafos de la 2S. Inventar nodos nuevos habría sido sobreingeniería.

## Dos defectos de motor descubiertos por la fase (y arreglados)

1. **Los emisores desactivados quedaban congelados en el aire.** En `avanzar_particulas` (glass_app), un emisor cuya entidad se desactivaba dejaba de recibir `update()` pero su pool seguía dibujándose para siempre: al cambiar a tormenta, la cortina de lluvia permanecía visible minutos después. **Arreglo**: los emisores ausentes de la lista activa se detienen (`stop()`) y siguen envejeciendo hasta agotar su pool; entonces se retiran del runtime. Al reactivarse a mitad de desvanecido, reanudan (`play()`). Test nuevo: `particle_fade_tests::emisor_desactivado_se_desvanece_y_se_retira` (ojo al escribir tests: `SceneState::default()` trae 4 entidades semilla; hay que localizar por id, no por posición).
2. **Los scripts no podían mover cuerpos creados por la escena.** Las operaciones de velocidad de física van por **handle** de cuerpo, pero C# no tenía forma de resolver entidad → handle (pasar el id devolvía `true` contra otro cuerpo o ninguno: la bola sonda no se movía). **Arreglo** (patrón de la auditoría C5, campos AL FINAL del puente): FFI `glass_physics_body_handle`, callback `get_body_handle` en `PhysicsBridgeCallbacks`, y `RuntimeBridge.GetBodyHandle(entityId)` en C#. Verificado con micro-sonda antes de la corrida de cierre (`handle(9)=2`, la bola se movió y cruzó el plano del portal). Ajuste derivado: la velocidad de lanzamiento de la sonda subió de 6 a 8 m/s — con el damping del motor, a 6 m/s llegaba al plano casi parada (medido).

## Test físico de cierre (una sola sesión, sin reinicio)

Driver `fase1/driver_f9.py`, corrida 3 (2026-11-12, log `test-fisico9/app_f9.log`):

- **LLUVIA** (09:59 UTC): partículas vivas 3.240, draws 2, emisores 2; captura `captura_f9_lluvia.png`.
- **Cruce del portal**: 3 cruces registrados (10:06, 10:12 y 10:19 UTC, ida y vuelta), captura `captura_f9_portal.png`.
- **TORMENTA** (10:21 UTC): vivas 3.869, emisores 3 (arena + fuego + lluvia desvaneciéndose), captura `captura_f9_tormenta.png` — en ella la lluvia ya es solo un residuo escaso, no la cortina plena.
- **Frametime estable en la transición** (lavapipe, ~1,1 fps de VM): total medio lluvia 938 ms, transición 873 ms, tormenta 867 ms; sin picos al cambiar de clima. `last_gpu_age_frames: 0` en las tres lecturas.
- Proceso vivo al cierre del test.

Nota de entorno: la corrida se retrasó por dos reemplazos de VM (faltaban `libxkbcommon-x11-0` y las listas de apt; el `apt-get update` del sistema quedó colgado en el mirror de Cogent y la instalación se hizo con un estado apt privado usando solo el mirror de Azure).

## Verificación

- `cargo test --workspace` con `RUSTFLAGS="-D warnings"`: **382 tests, 0 fallos** (381 de la pasada de medios + el nuevo de partículas).
- `glass_app` recompilado; proyecto gestionado con **0 errores**. `scene_assets` en verde con la escena nueva.
- Artefactos de diagnóstico retirados al cierre (`SondaProbe.cs`, `SondaTest.scene.json`).

## Consumo por proveedor

- DeepSeek (`deepseek-flash`): **4.442** tokens (script `EscaparateControl`).
- Kimi (`kimi-k3.1-pro`): **1.846** tokens (documentación de la escena).
- Claude: 0. Integración, diagnóstico de los dos defectos y sus arreglos: trabajo quirúrgico de Rayo.
