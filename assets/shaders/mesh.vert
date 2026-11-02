#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;

layout(push_constant) uniform PushConstants {
    mat4 mvp;
    vec4 light_dir;
    vec4 light_color;
    vec4 material_params;
    vec4 object_color;
} u_push;

void main() {
    v_normal = in_normal;
    v_uv = in_uv;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
