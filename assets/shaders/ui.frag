#version 450
// Fragmento UI (texto/SDF y color plano). Todos los colores premultiplicados.
layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;
layout(location = 2) in vec2 v_local;
layout(location = 3) in vec2 v_tamano;
layout(location = 4) in vec4 v_radios;
layout(location = 5) in vec4 v_clip;
layout(location = 6) in vec4 v_pack1;
layout(location = 7) in vec4 v_pack2;
layout(location = 8) in vec4 v_grad_c0;
layout(location = 9) in vec4 v_grad_c1;

layout(set = 0, binding = 0) uniform sampler2D atlas_tex;

layout(location = 0) out vec4 out_color;

void main() {
    vec4 base = v_color;
    float modo = v_pack1.y;
    bool hay_grad = v_pack2.x > v_pack1.w; // grad_t1 > grad_t0

    // 2. Degradado: sustituye base antes de aplicar modo (manda sobre imagen/texto).
    if (hay_grad) {
        float ang = radians(v_pack1.z);
        // Y crece hacia abajo: 0° = izq->der, 90° = arriba->abajo.
        vec2 d = vec2(cos(ang), sin(ang));
        float pmax = v_tamano.x * abs(d.x) + v_tamano.y * abs(d.y);
        float t = dot(v_local, d) / max(pmax, 0.0001);
        if (t < v_pack1.w || t > v_pack2.x) discard;
        base = mix(v_grad_c0, v_grad_c1, (t - v_pack1.w) / max(v_pack2.x - v_pack1.w, 0.0001));
    } else if (modo > 0.5 && modo < 1.5) {
        // 3. Imagen: la textura aporta el color, el vértice tiñe.
        // La mezcla es PREMULTIPLICADA (ONE / ONE_MINUS_SRC_ALPHA):
        // el texel también debe premultiplicarse por su propio alfa,
        // o un texel semitransparente filtraría su RGB a plena
        // intensidad (en P4c el texto blanco sobre negro salía como
        // bloques sólidos por el mismo motivo en la rama de texto).
        vec4 tex = texture(atlas_tex, v_uv);
        base = vec4(tex.rgb * tex.a * v_color.rgb, tex.a * v_color.a);
    } else if (modo > 1.5 && modo < 2.5) {
        // 3. Texto/SDF: evaluar distancia con signo y anti-alias en pantalla.
        const float SPREAD_PX = 4.0; // rango de distancia codificado en el atlas
        float a = texture(atlas_tex, v_uv).a;
        float dist = (a - 0.5) * 2.0 * SPREAD_PX; // distancia en texeles: >0 dentro, <0 fuera
        float w = fwidth(dist); // texeles por píxel de pantalla
        float cobertura = smoothstep(-w * 0.5, w * 0.5, dist);
        // Premultiplicado (P4c): RGB y alfa modulados por cobertura.
        base = vec4(base.rgb * cobertura, base.a * cobertura);
    }
    // modo 3 (sombra) y modo 0 (color) no tocan base aquí.

    // 4. SDF de rect redondeado. dist se calcula una sola vez y se reutiliza.
    float rad = (v_local.x < v_tamano.x * 0.5)
        ? (v_local.y < v_tamano.y * 0.5 ? v_radios.x : v_radios.w)
        : (v_local.y < v_tamano.y * 0.5 ? v_radios.y : v_radios.z);
    vec2 q = abs(v_local - v_tamano * 0.5) - (v_tamano * 0.5 - vec2(rad));
    float dist = (rad > 0.0)
        ? length(max(q, 0.0)) - rad
        : length(max(abs(v_local - v_tamano * 0.5) - v_tamano * 0.5, 0.0));
    if (rad > 0.0) {
        base.a *= 1.0 - smoothstep(0.0, 1.0, dist);
    }

    // 5. Sombra (modo 3): mismo SDF difuminado por el radio de desenfoque.
    if (modo > 2.5 && modo < 3.5) {
        float b = max(v_pack2.y, 1.0);
        base.a *= 1.0 - smoothstep(0.0, b, dist);
    }

    // 5b. Borde (V4/Q3, modo 4): banda de `ancho` px hacia dentro del
    // rect redondeado. El ancho viaja en v_pack2.y (el campo de
    // desenfoque del segmento). Se usa el SDF con signo propio
    // (negativo dentro, positivo fuera) porque el `dist` del paso 4
    // no tiene signo definido con radios 0.
    // Premultiplicado (P4c): RGB y alfa se modulan JUNTOS; si solo
    // bajara el alfa, la mezcla ONE/ONE_MINUS_SRC_ALPHA seguiría
    // sumando el RGB a plena intensidad y el borde cubriría el rect.
    if (modo > 3.5) {
        float ancho = max(v_pack2.y, 0.0);
        vec2 qb = abs(v_local - v_tamano * 0.5) - (v_tamano * 0.5 - vec2(rad));
        float db = length(max(qb, 0.0)) + min(max(qb.x, qb.y), 0.0) - rad;
        float w = fwidth(db);
        float banda = smoothstep(-ancho - w * 0.5, -ancho + w * 0.5, db)
            * (1.0 - smoothstep(-w * 0.5, w * 0.5, db));
        base *= banda;
    }

    // 6. Clip. NOTA: v_clip llega en coordenadas LOCALES al rect del nodo
    // (el conversor de P3b2 resta la posición del nodo); no en pantalla.
    if (v_clip.z > 0.0) {
        vec2 fuera = max(max(v_clip.xy - v_local, v_local - (v_clip.xy + v_clip.zw)), vec2(0.0));
        float dclip = length(fuera);
        float s = max(v_pack1.x, 0.0);
        base.a *= 1.0 - smoothstep(0.0, max(s, 0.001), dclip);
    }

    // 7. Ya premultiplicado: la mezcla ONE/ONE_MINUS_SRC_ALPHA hace el resto.
    out_color = base;
}
