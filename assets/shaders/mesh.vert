#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;

layout(push_constant) uniform PushConstants {
    mat4 mvp;                // offset 0
    mat4 model;              // offset 64
    vec4 light_dir;          // offset 128
    vec4 light_color;        // offset 144
    vec4 material_params;    // offset 160
    vec4 object_color;       // offset 176
    vec4 camera_pos;         // offset 192
} u_push;

void main() {
    // La normal viaja a espacio de mundo: el fragment shader la compara con una
    // dirección de luz que ya está en mundo.
    v_normal = transpose(inverse(mat3(u_push.model))) * in_normal;
    v_uv = in_uv;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
