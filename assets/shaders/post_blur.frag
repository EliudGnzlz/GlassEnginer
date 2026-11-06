#version 450
// Desenfoque gaussiano separable de 9 muestras (sigma ~2 téxeles en la
// resolución de trabajo, media). La dirección llega por push constants:
// (1,0) horizontal, (0,1) vertical; dos pasadas componen el blur 2D.
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D scene_tex;
layout(set = 0, binding = 1) uniform sampler2D aux_tex; // sin uso en esta pasada

layout(push_constant) uniform PostPush {
    vec4 params0; // x = umbral, y = rodilla, z = intensidad, w = viñeta
    vec4 params1; // x = dirección blur x, y = dirección blur y, z = exposición, w = activo
} u_post;

void main() {
    vec2 texel = 1.0 / vec2(textureSize(scene_tex, 0));
    vec2 dir = u_post.params1.xy * texel;
    vec3 sum = texture(scene_tex, v_uv).rgb * 0.227027;
    sum += texture(scene_tex, v_uv + dir * 1.0).rgb * 0.194595;
    sum += texture(scene_tex, v_uv - dir * 1.0).rgb * 0.194595;
    sum += texture(scene_tex, v_uv + dir * 2.0).rgb * 0.121622;
    sum += texture(scene_tex, v_uv - dir * 2.0).rgb * 0.121622;
    sum += texture(scene_tex, v_uv + dir * 3.0).rgb * 0.054054;
    sum += texture(scene_tex, v_uv - dir * 3.0).rgb * 0.054054;
    sum += texture(scene_tex, v_uv + dir * 4.0).rgb * 0.016216;
    sum += texture(scene_tex, v_uv - dir * 4.0).rgb * 0.016216;
    out_color = vec4(sum, 1.0);
}
