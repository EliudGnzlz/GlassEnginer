#version 450

// Fragmento de partículas (Fase 8P): color por vértice con una
// caída radial suave (disco blando) para que fuego y humo se lean
// sin necesidad de texturas.

layout(location = 0) in vec4 v_color;
layout(location = 1) in vec2 v_uv;

layout(location = 0) out vec4 out_color;

void main() {
    vec2 c = v_uv - 0.5;
    float d = length(c) * 2.0;
    float fall = smoothstep(1.0, 0.25, d);
    out_color = vec4(v_color.rgb, v_color.a * fall);
    if (out_color.a < 0.003) {
        discard;
    }
}
