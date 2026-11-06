#version 450
// Bright-pass del bloom: extrae de la escena HDR lo que supera el
// umbral, con rodilla suave para que el bloom no parpadee al cruzar el
// umbral. Trabaja en lineal HDR, ANTES del ACES: un umbral de 1,0 solo
// deja pasar especulares intensos y emisivos por encima del blanco.
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D scene_tex;
layout(set = 0, binding = 1) uniform sampler2D aux_tex; // sin uso en esta pasada

layout(push_constant) uniform PostPush {
    vec4 params0; // x = umbral, y = rodilla, z = intensidad, w = viñeta
    vec4 params1; // x = dirección blur x, y = dirección blur y, z = exposición, w = activo
} u_post;

void main() {
    vec3 hdr = texture(scene_tex, v_uv).rgb;
    float threshold = u_post.params0.x;
    float knee = u_post.params0.y;
    float luma = dot(hdr, vec3(0.2126, 0.7152, 0.0722));
    float soft = clamp(luma - threshold + knee, 0.0, 2.0 * knee);
    soft = (soft * soft) / (4.0 * knee + 0.0001);
    float contribution = max(soft, luma - threshold);
    vec3 bright = hdr * (contribution / max(luma, 0.0001));
    out_color = vec4(bright, 1.0);
}
