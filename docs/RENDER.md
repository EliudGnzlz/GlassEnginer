# Render de GlassEnginer (Fase 2)

## Frustum culling

El culling se aplica por vista antes de emitir los comandos de geometría.
Se conserva el AABB local de cada geometría y se transforma mediante su matriz de mundo.
El AABB resultante se comprueba contra los seis planos del frustum.
Los planos se extraen de la matriz view-projection (VP) de la vista activa.
Las entidades completamente exteriores se descartan sin enviar su draw call.
Las intersectantes o interiores se mantienen.
Las listas de sombras no se filtran con este culling de la vista principal.
Cada pase de sombra conserva su propia selección de casters.
La tool `glass_render_stats` expone los totales y los descartes por vista.
Permite contrastar las entidades presentes en escena con las realmente dibujadas.

## Compresión BC de texturas

La compresión requiere la feature `textureCompressionBC` del dispositivo.
Las texturas con alfa utilizan BC3 en formato SRGB.
Las texturas sin alfa utilizan BC1 en formato SRGB.
Los mips se generan en CPU mediante downsample de caja.
Cada nivel comprimido se sube individualmente al recurso GPU.
Sin la feature, se emplea el respaldo RGBA8.
En ese respaldo, los mips se generan mediante blits en GPU.
La selección de ruta no cambia el uso de la textura por los materiales.
`glass_texture_info` expone los metadatos de cada textura.
Incluye su formato, dimensiones, niveles mip y ruta de compresión seleccionada.

## Niebla analítica

La niebla se declara en el bloque `fog` de una entidad.
Contiene `color` en espacio lineal y las distancias `near` y `far` en metros.
La mezcla es lineal según la distancia entre el fragmento y la cámara.
Antes de `near` se conserva el color original; después de `far` domina la niebla.
El cálculo está integrado en el shader PBR y en su gemelo legacy.
Si varias entidades contienen el bloque, manda la primera que lo lleve.
La escena `Fase2Demo.scene.json` lo declara en la cámara principal.

## Sombras de luces puntuales

Una puntual solicita sombras con `cast_shadow: true` en su bloque `light`.
El renderer dispone de un atlas compartido de cuatro cubos D32 de 512².
Cada cubo representa las seis caras necesarias para una puntual.
Los cubos se asignan por cercanía al ojo entre las luces solicitantes.
Como máximo cuatro puntuales proyectan sombra simultáneamente.
Las puntuales restantes siguen iluminando, pero sin sombra.
Cada luz con sombra ejecuta seis pases de profundidad por frame.
El coste aumenta linealmente con luces asignadas y casters visibles.

## Escena de prueba

`assets/scenes/Fase2Demo.scene.json` se genera con `scripts/gen_escena_fase2.py`.
La zona A queda delante de la cámara; B y C sirven para comprobar descartes.
L1-L4 solicitan sombras; L5-L8 iluminan sin ellas.
Ocho cubos flotantes aportan casters bajo las cuatro luces con sombra.

## Tools MCP de la fase

- `glass_render_stats`: draws, geometría retenida y descartes de culling.
- `glass_texture_info`: formato, compresión y mips por textura.
- `glass_frame_timing`: tiempos de frame y pases, disponible desde la Fase 0.
- `glass_memory_stats`: consumo de memoria, disponible desde la Fase 0.
