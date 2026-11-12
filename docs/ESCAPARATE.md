# Escena escaparate (Fase 9)

Escena de demostración del motor **GlassEnginer**, preparada para mostrar materiales, efectos, partículas, portales y cambios de iluminación funcionando conjuntamente.

## Abrir la escena

Abre `assets/scenes/Fase9Escaparate.scene.json` en modo **standalone** o desde el **editor**. No necesita pasos previos. El script `EscaparateControl` gestiona automáticamente la actividad de la escena.

## Recorrido por la sala

La sala mide **34 × 26 m** y organiza la avenida de materiales en dos filas:

- **Izquierda — PBR:** cuatro esferas de colores —oro, rojo, azul y gris— sobre pedestales, junto a un cubo que utiliza el grafo de tinte con textura.
- **Derecha — Toon/cel shading:** una esfera, un cubo, una cápsula y un cilindro con el grafo `toon_demo`, incluido su contorno.

En la zona media encontrarás un plano de **agua animada** con el grafo `agua`, una esquina de **fuego** con partículas y luz cálida, y un **haz láser** con el grafo `laser` entre dos postes.

Al fondo, un muro divisorio contiene un **portal en vivo** enlazado con otro portal. El script de control lanza automáticamente una bola naranja que lo cruza repetidamente conservando el impulso.

## Clima automático

Sin intervención, la escena alterna cada **24 segundos** entre:

- **LLUVIA:** un emisor cubre toda la sala y el sol permanece a plena intensidad.
- **TORMENTA DE ARENA:** partículas ocres atraviesan la sala impulsadas por el viento, aparece un muro de polvo con el grafo `tormenta_arena` al fondo y el sol se atenúa a **0,45**.

`EscaparateControl` gobierna el ciclo. Cada cambio se anuncia en el registro de la aplicación con el prefijo `[ESCAPARATE]`.

## Qué mirar en una demo

- El contraste entre PBR y toon bajo la misma luz.
- La animación del agua.
- El cruce del portal sin corte de cámara.
- El cambio de clima sin recargar la escena.

## Nota técnica

La escena no incorpora código de motor nuevo para los efectos: combina grafos de shaders de la Fase 2S, partículas CPU de la Fase 8P, portales y luces de la Fase 2. También sirve como **regresión visual** de todos esos sistemas.
