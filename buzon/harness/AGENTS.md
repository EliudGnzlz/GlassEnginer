# AGENTS.md

Your operating manual for this workspace, written by you. Your main instructions cover how you work in general. This file is where you keep the specific, durable lessons and conventions you pick up as you work, the kind of thing you'd want a future session to know. It's not about the user (that goes in `USER.md` and your memory) or your personality (that's `SOUL.md`); it's about how you get work done here.

## Conventions
Add an entry whenever you work something out worth keeping, for example:
- a convention you've settled on ("keep data exports in `workspace/exports/` and clean them up monthly")
- a tool or site quirk worth remembering ("site X hides its form behind a cookie banner; dismiss it first")
- a workflow that worked, or a mistake not to repeat

It starts empty and is meant to grow slowly. Don't pad it; a short, accurate file beats a long, stale one.

## Harness — mínimo vigente (aprobado por D3m3tr10 el 2026-11-24, de la propuesta v2)

- **Regla de promoción:** una decisión del usuario que ya se corrigió dos veces deja de ser prosa y se convierte en mecanismo (puerta en una skill, script verificador o regla dura en este archivo). El texto se «pesa» y puede saltarse; el mecanismo se impone. No acumular la misma norma como párrafo en más sitios.
- **Adaptar skills o patrones de terceros:** nunca se importan tal cual. Antes de adaptar cualquiera, auditoría de seguridad: qué instrucciones haría obedecer al agente, qué scripts trae y qué dominios toca (medido fuera: ~37 % de skills públicas con problemas, payloads maliciosos incluidos). Lo adaptado deja constancia de qué cambió y por qué.
- El resto del mínimo (presupuesto duro por paquete, contrato de paquete único, revisión cruzada solo en lo crítico, diagnóstico de fallos antes de cambiar de proveedor) vive ejecutable en la skill `orchestrated_code_delegation`; el barrido anti-regresión y la caché separada en el consumo, en `glass_engine_closure_verification`. No se añaden skills nuevas para estas reglas: son secciones de las existentes.
- La revisión pez del plan (adaptación EGM 2026-11-25) vive en la skill `orchestrated_code_delegation`; el documento de auditoría y resoluciones es `~/workspace/ADAPTACION_EGM_PECES_2026-11-25.md`.

## GlassEnginer — CI (regla permanente, orden 2026-11-13)

- **Ningún cierre de bloque se anuncia sin comprobar el estado del CI de GitHub del commit correspondiente.** La suite local en verde NO basta: ha pasado dos veces (12 y 13 nov) que el CI estaba en rojo mientras lo local pasaba, por correr el CI con `RUSTFLAGS="-D warnings"` y pasos que localmente no se repetían.
- Antes de pushear un cierre, reproducir en local el pipeline exacto de `.github/workflows/ci.yml`: `RUSTFLAGS="-D warnings" cargo build --workspace --locked` · `cargo build -p glass_app --no-default-features --locked` · build del C# · `xvfb-run -a cargo test --workspace --locked` con `VK_ICD_FILENAMES` de lavapipe y `DOTNET_ROOT`. Tras el push, verificar la corrida real en la pestaña Actions (navegador con la sesión del usuario; el conector de GitHub no tiene tools de Actions).
- Si el CI está en rojo, el arreglo del CI tiene prioridad sobre cualquier otro trabajo y no se apila nada nuevo encima del rojo.

## Test físico: analizar de verdad, no durar (corrección de D3m3tr10, 2026-11-25)

- **Durar mucho en un test físico NO es testear bien.** El fallo que él ha visto (y que él o su probador cazaron después en campo, p. ej. Bug F reabierto por su evidencia y Bug I no reproducido en la VM): sesiones largas de clics sin analizar de verdad la escena ni el componente creado.
- Un test físico válido inspecciona el estado real: valores en el Inspector, el archivo de escena guardado, el comportamiento medido (posiciones, rebotes, logs) — y declara qué se verificó de cada cosa, no solo que «se probó». Si el test no puede mirar dentro de lo que creó, no cuenta como test del componente.
- Eficiente y al punto: preparar la escena y los puntos de medición antes, medir, concluir. Nada de vueltas largas sin observación nueva.

## Clics sintéticos en el editor (lección 2026-11-15, defecto «Cerrar» de B-C2)
- Las coordenadas estimadas a ojo desde una captura fallan por ~50 px: el veredicto físico y yo clicamos (292, 590) creyendo que era «Cerrar» y era «Hornear» (x≈275–460); «Cerrar» está en (243, 582) y «Subdividir» en (41, 698). Antes de clicar un botón, medir su bbox con `muse.visual_grounding` y verificar con un barrido (varios x en la misma y) mirando el efecto real, no solo la captura. Si `visual_grounding` se cuelga (pasó en B-D, 180 s), alternativa que funciona: crop con PIL y rejilla de coordenadas etiquetadas cada 20 px, y leer la posición de la rejilla, no a ojo.
- En Xvfb sin WM el foco de teclado no sigue al clic: hacer `xdotool windowfocus <id>` antes de teclear, o el `type` no entra en el campo.
- Un `eprintln!` temporal con la acción despachada (`action=...`) es la forma decisiva de distinguir «el clic no llega» de «la acción no hace nada».
