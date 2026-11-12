# Sistema de animación

GlassEnginer reproduce animaciones esqueléticas importadas desde modelos `.glb` y `.blend`. La evaluación de poses se realiza en CPU y admite reproducción directa, fundidos entre clips, controladores declarativos y blend trees 1D.

## Clips y piel en una entidad

Un modelo importado puede incluir clips de animación y una piel (*skin*) con articulaciones. Para evaluar la pose, el motor muestrea la transformación local de cada nodo:

- Traslación y escala: interpolación lineal (`lerp`).
- Rotación: interpolación esférica (`slerp`).
- Resultado: composición de las matrices de piel.

Una entidad con el componente `Animator` puede reproducir directamente un clip mediante los campos `clip`, `speed` y `playing`. Sin controlador también es posible solicitar un `CrossFade` entre clips desde código.

Si el campo `controller` contiene un nombre, la entidad utiliza el controlador situado en:

`assets/animators/<nombre>.animator.json`

## Controladores

Los controladores son archivos JSON inspirados en una versión reducida del Animator de Unity. Declaran:

- `parameters`: parámetros `bool`, `float` o `trigger`, con valor por defecto.
- `states`: estados con nombre, clip, velocidad y repetición.
- `transitions`: transiciones condicionadas entre estados.
- `default_state`: estado inicial.

```jsonc
{
  "parameters": {
    "walk": { "type": "bool", "default": false },
    "Attack": { "type": "trigger", "default": false }
  },
  "default_state": "Idle",
  "states": [
    // Un estado normal reproduce un único clip.
    { "name": "Idle", "clip": "Idle", "speed": 1.0, "loop": true },
    { "name": "Walk", "clip": "Walk", "speed": 1.0, "loop": true }
  ],
  "transitions": [
    {
      "from": "Idle",          // "*" equivale a AnyState.
      "to": "Walk",
      "conditions": [
        { "param": "walk", "op": "eq", "value": true }
      ],
      "fade": 0.3,             // Segundos de fundido.
      "exit_time": 0.9         // Opcional; fracción 0..1 del clip origen.
    }
  ]
}
```

Las condiciones admiten los operadores `eq`, `not_eq`, `greater` y `less`. El `exit_time`, cuando está presente, se expresa como fracción de la duración del clip del estado origen.

## Transiciones y cross-fade

Cuando se cumplen las condiciones de una transición, el motor cambia al estado destino y mezcla durante `fade` segundos el clip saliente con el entrante. El peso avanza linealmente con el tiempo; traslación y escala usan `lerp`, y la rotación usa `slerp` por el camino corto.

Los `trigger` se consumen al disparar la transición que los utiliza. Desde código también existen `Play` y `CrossFade` programáticos por nombre de estado: quedan pendientes (`pending`) y pueden especificar su propio fundido.

La mezcla general de N poses reside en el renderer:

`evaluate_skin_matrices_multi(skin, samples)`

Cada sample contiene `(clip, tiempo, peso)`. Los pesos se normalizan por su suma y las rotaciones se acumulan mediante `slerp` iterativo por el camino corto. `evaluate_skin_matrices_blended`, para dos clips, es el caso particular histórico.

## Blend tree 1D

Un estado puede declarar `blend` en lugar de utilizar directamente su clip. El parámetro indicado determina el peso de cada hijo según la distancia a su umbral:

- En su umbral, un hijo pesa 1.
- Entre dos umbrales, el reparto es lineal.
- Fuera del rango, domina el hijo extremo.
- Cada hijo mantiene su propio reloj, que avanza con su `speed` multiplicada por la del estado.

El ejemplo real `assets/animators/locomocion.animator.json` define `Idle` y `Locomocion`; este último mezcla locomoción mediante `Speed`:

```json
{
  "name": "Locomocion",
  "speed": 1.0,
  "loop": true,
  "blend": {
    "param": "Speed",
    "children": [
      { "clip": "Idle", "threshold": 0.0, "speed": 1.0 },
      { "clip": "Walk", "threshold": 2.5 },
      { "clip": "Run",  "threshold": 5.0 }
    ]
  }
}
```

Las transiciones entre `Idle` y `Locomocion` usan el booleano `walk` y un fundido de `0,3` segundos. Un fundido puede entrar o salir de un estado blend: la mezcla del frame combina los samples de ambos lados con sus pesos correspondientes.

## Parámetros desde código y MCP

Los parámetros pueden modificarse desde C#. Los cambios afectan a la evaluación de transiciones y al blend tree.

También están disponibles las tools MCP:

- `glass_animator_state`: consulta el estado actual, el fundido, los parámetros y los pesos del blend.
- `glass_animator_set_param`: fija un parámetro; las transiciones y el blend reaccionan en el siguiente frame.

## Límites conocidos

- No hay capas de animación ni máscaras de esqueleto.
- No hay IK.
- No hay eventos de animación por ahora.
- En un estado blend, `exit_time` usa como referencia la duración del hijo de menor umbral.