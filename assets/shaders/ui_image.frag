#version 450
// Fragmento UI para imágenes. Misma lógica que ui.frag; lo único que cambia
// entre ambos pipelines es la textura enlazada en binding 0.
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
layout(location = 10) in vec4 v_clip_radios; // M3 (V5c): radios del clip

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
        base = texture(atlas_tex, v_uv) * v_color;
    } else if (modo > 1.5 && modo < 2.5) {
        // 3. Texto/SDF: cobertura en alfa; modula color base (o degradado).
        float cobertura = texture(atlas_tex, v_uv).a;
        base = vec4(base.rgb, base.a * cobertura);
    }
    // modo 3 (sombra) y modo 0 (color) no tocan base aquí.

    // 4. SDF de rect redondeado. dist se calcula una sola vez y se reutiliza.
    // PREMULTIPLICADO: se escala `base` ENTERO (rgb + alfa), no solo el
    // alfa — con la mezcla ONE/ONE_MINUS_SRC_ALPHA, bajar solo el alfa
    // no recorta nada: el rgb seguiría sumándose a plena intensidad
    // (cazado por el spike M3, V5c; el borde 5b y el texto ya lo hacían así).
    float rad = (v_local.x < v_tamano.x * 0.5)
        ? (v_local.y < v_tamano.y * 0.5 ? v_radios.x : v_radios.w)
        : (v_local.y < v_tamano.y * 0.5 ? v_radios.y : v_radios.z);
    vec2 q = abs(v_local - v_tamano * 0.5) - (v_tamano * 0.5 - vec2(rad));
    float dist = (rad > 0.0)
        ? length(max(q, 0.0)) - rad
        : length(max(abs(v_local - v_tamano * 0.5) - v_tamano * 0.5, 0.0));
    if (rad > 0.0) {
        base *= 1.0 - smoothstep(0.0, 1.0, dist);
    }

    // 5. Sombra: mismo SDF difuminado por el radio de desenfoque.
    if (modo > 2.5) {
        float b = max(v_pack2.y, 1.0);
        base *= 1.0 - smoothstep(0.0, b, dist);
    }

    // 6. Clip. NOTA: v_clip llega en coordenadas LOCALES al rect del nodo
    // (el conversor de P3b2 resta la posición del nodo); no en pantalla.
    // M3 (V5c): el clip puede ser redondeado (v_clip_radios del ancestro
    // que recorta). SDF con signo del rect redondeado; con radios 0 es el
    // rect de antes, bit a bit (max(dclip, 0) conserva el borde suave
    // entero fuera del rect, como el código anterior). Se escala `base`
    // ENTERO (ver paso 4): bajar solo el alfa no recorta nada con la
    // mezcla ONE/ONE_MINUS_SRC_ALPHA.
    if (v_clip.z > 0.0) {
        float crad = (v_local.x < v_clip.x + v_clip.z * 0.5)
            ? (v_local.y < v_clip.y + v_clip.w * 0.5 ? v_clip_radios.x : v_clip_radios.w)
            : (v_local.y < v_clip.y + v_clip.w * 0.5 ? v_clip_radios.y : v_clip_radios.z);
        vec2 cc = v_clip.xy + v_clip.zw * 0.5;
        vec2 qc = abs(v_local - cc) - (v_clip.zw * 0.5 - vec2(crad));
        float dclip = (crad > 0.0)
            ? length(max(qc, 0.0)) + min(max(qc.x, qc.y), 0.0) - crad
            : length(max(max(v_clip.xy - v_local, v_local - (v_clip.xy + v_clip.zw)), vec2(0.0)));
        float s = max(v_pack1.x, 0.0);
        base *= 1.0 - smoothstep(0.0, max(s, 0.001), max(dclip, 0.0));
    }

    // 7. Ya premultiplicado: la mezcla ONE/ONE_MINUS_SRC_ALPHA hace el resto.
    out_color = base;
}
