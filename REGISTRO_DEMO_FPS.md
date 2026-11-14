# REGISTRO — Demo FPS POLYGON (cerrada el 2026-11-14)

Escena `assets/scenes/DemoFpsPolygon.scene.json` (542 entidades,
generador `fase1/gen_demo_fps.py`): el campo de la Fase 10 vestido
con el pack Synty POLYGON Prototype (26 props/edificio convertidos a
.glb), un edificio de dos plantas con puerta y escalera TriMesh y 3
luces de perfil Vela, y dos X Bot de Mixamo: el jugador y el enemigo.

## Qué demuestra

- **Jugador** (`JugadorPersonaje.cs`, entidad 200): cuerpo X Bot
  (`xbot_demo.glb`, 11 clips) movido por el **CharacterController**
  (andar/correr/agacharse/saltar, plataformas nativas), cámara en
  1ª/3ª persona con la tecla V, ratón con yaw/pitch, armas por datos
  de la Fase 10 (teclas 1–4, R recarga, clic derecho = mira por FOV).
- **Enemigo** (`EnemigoSimple.cs`, entidad 210): X Bot teñido de
  rojo, 100 de vida, **barra de vida** flotante (entidad 211) que se
  acorta con cada impacto y **animación de muerte** (clip «Falling
  Back Death» de Mixamo, estado `Muerte` del controlador
  `polygon_locomocion`).
- **Verificación automática** (`DemoVerif.scene.json` +
  `VerificacionDemo.cs`): sin input humano, el jugador dispara con
  la pistola hasta matar al enemigo. Resultado del 2026-11-14:
  **10 disparos, 10 impactos, vida 100→0, «ENEMIGO MUERTO»**.
  Con input sintético X11 (driver_demo.py): 3 clics muestreados,
  3 impactos; el CharacterController anda y queda grounded
  (z 6,0 → 5,57).

## Los dos defectos que destapó (arreglados en el motor)

1. **Segfault al arrancar la demo.** `GameObject.GetComponent<T>()`
   solo busca en la lista del wrapper C#: para un componente que
   existe en la escena pero nunca se añadió desde C# devuelve
   **null**, y desreferenciar ese null en este host mata el proceso
   con SIGSEGV sin pasar por los try/catch (las excepciones por AV
   de hardware no se convierten en excepción gestionada aquí).
   Patrón obligatorio desde entonces:
   `GetComponent<T>() ?? AddComponent<T>()`.
   Repro conservada: escena `MicroAnim` + `MicroAnimTest.cs`.
   Además a `JugadorPersonaje.OnAnimationEvent` le faltaba el
   `override` (el método se ocultaba y los eventos Paso no llegaban).
2. **El jugador se disparaba a sí mismo.** La cápsula del
   CharacterController vivía en el mundo físico como un collider
   más: el ojo de la cámara está DENTRO de la cápsula, así que todo
   raycast hitscan impactaba en la propia cápsula a distancia 0
   (entidad 200) y los proyectiles nacían dentro de ella y se
   quedaban/desviaban. 18+ disparos, 0 impactos, con la geometría
   perfecta (la sonda MCP `glass_physics_cast` veía al enemigo a
   19,31 m). **Arreglo:** la cápsula del personaje se crea con
   grupos de colisión (GROUP_32, filtro NONE) — invisible para el
   solver y para las consultas del mundo; el controlador ya se
   resolvía con sus propias sondas (que excluyen su collider), así
   que nada pierde y nadie debe chocar con ella. Regresión:
   `crates/glass_physics/tests/character_invisible.rs` (2 tests:
   el raycast desde el ojo llega a la pared; un dinámico atraviesa
   la cápsula sin chocar). Limitación conocida: dos personajes ya no
   se empujan entre sí por el solver (sus controladores no se ven).

## Arreglos menores del bloque

- La precarga de modelos de `glass_app` ahora es **recursiva**:
  los 26 .glb de `assets/models/polygon/` nunca se registraban.
- `ProyectilF10`/`ProyectilRpg` activan **CCD**
  (`CollisionDetectionMode.Continuous`) al crearse: a 120 m/s el
  paso fijo avanza 2 m y tunelizaba blancos finos.
- De la escena salieron la Diana10/Poste10 de la Fase 10 (estaban
  en plena línea de tiro jugador→enemigo y se comían los disparos)
  y el Canvas/UI heredado de la Fase 10 (110–116).

## Consumo de APIs del bloque

- Claude Sonnet 5: 17.069 tokens (scripts Jugador/Enemigo).
- Opus, DeepSeek, Kimi: 0 (el diagnóstico de los dos defectos y los
  arreglos del motor fueron trabajo directo con bisección y sondas).
