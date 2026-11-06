#version 450
// Pasada final del post-procesado: compone la escena HDR con el bloom
// (suma en lineal, ANTES de mapear), aplica el ACES de Narkowicz con la
// exposición 1,5 que antes vivía en pbr.frag, y una viñeta suave DESPUÉS
// del mapeo. Escribe en el objetivo LDR (R8G8B8A8_UNORM) de siempre: la
// UI 2D, egui y el blit del standalone consumen ese objetivo sin saber
// que el post existe. La gamma sigue siendo única: la de presentación.
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D scene_tex;
layout(set = 0, binding = 1) uniform sampler2D bloom_tex;

layout(push_constant) uniform PostPush {
    vec4 params0; // x = umbral, y = rodilla, z = intensidad bloom, w = intensidad viñeta
    vec4 params1; // x/y = dirección blur (sin uso aquí), z = exposición, w = activo
} u_post;

vec3 aces_narkowicz(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

void main() {
    vec3 scene = texture(scene_tex, v_uv).rgb;
    vec3 bloom = texture(bloom_tex, v_uv).rgb;
    vec3 combined = scene + bloom * u_post.params0.z;
    vec3 ldr = aces_narkowicz(combined * u_post.params1.z);
    // Viñeta: oscurece hacia las esquinas tras el mapeo (espacio LDR).
    float vignette = u_post.params0.w;
    if (vignette > 0.0) {
        vec2 centered = v_uv * 2.0 - 1.0;
        float dist = length(centered);
        ldr *= 1.0 - smoothstep(0.6, 1.45, dist) * vignette;
    }
    out_color = vec4(ldr, 1.0);
}
