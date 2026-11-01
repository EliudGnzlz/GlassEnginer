#version 450

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;

layout(location = 0) out vec4 out_color;

void main() {
    vec3 light_dir = normalize(vec3(0.5, 1.0, 0.7));
    float diff = max(dot(normalize(v_normal), light_dir), 0.25);
    vec3 base_color = vec3(0.18, 0.55, 0.92); // Azul distintivo GlassEngine
    out_color = vec4(base_color * diff, 1.0);
}
