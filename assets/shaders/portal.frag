#version 450
// Muestra la vista renderizada desde el portal enlazado
// (textura HDR en el set 4, el mismo layout que la textura
// del lightmap) con muestreo proyectivo, y tiñe el borde del
// rectángulo con el color del portal (v_rim). El borde se
// mide sobre la posición local normalizada (v_local en
// [-1, 1] sobre la cara), no sobre el UV de la malla.
layout(set = 4, binding = 0) uniform sampler2D portalTex;

layout(location = 0) in vec4 v_clip_virtual;
layout(location = 1) in vec2 v_local;
layout(location = 2) in vec4 v_rim;

layout(location = 0) out vec4 outColor;

void main() {
    vec3 col;
    if (v_clip_virtual.w <= 0.0) {
        col = vec3(0.0);
    } else {
        vec2 uv = v_clip_virtual.xy / v_clip_virtual.w * 0.5 + 0.5;
        col = texture(portalTex, uv).rgb;
    }
    // Anillo fino en el borde exterior del rectángulo.
    // (Uno grueso con brillo HDR se quemaba a crema en el
    // post-procesado y tapaba la vista: fino y apenas
    // sobreiluminado se lee como el marco del portal.)
    float d = max(abs(v_local.x), abs(v_local.y));
    float borde = smoothstep(0.93, 0.995, d);
    col = mix(col, v_rim.rgb * 1.15, borde * 0.9);
    outColor = vec4(col, 1.0);
}
