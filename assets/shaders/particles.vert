#version 450

// Vértice de partículas (Fase 8P): la CPU ya expandió el billboard
// en espacio de mundo (posición, color y uv por esquina); aquí
// solo se aplica el MVP del bloque de push constants de siempre.

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec4 in_color;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec4 v_color;
layout(location = 1) out vec2 v_uv;

layout(push_constant) uniform PushConstants {
    mat4 mvp;
    mat4 model;
    vec4 light_dir;
    vec4 light_color;
    vec4 material_params;
    vec4 object_color;
    vec4 camera_pos;
    vec4 lm_params;
} u_push;

void main() {
    v_color = in_color;
    v_uv = in_uv;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
