#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec3 v_world_pos;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec2 v_uv;

layout(push_constant) uniform PushConstants {
    mat4 mvp;                // 64 bytes (offset 0)
    vec4 light_dir;          // 16 bytes (offset 64): xyz = light_dir, w = light_intensity
    vec4 light_color;        // 16 bytes (offset 80): xyz = light_color, w = ambient_intensity
    vec4 material_params;    // 16 bytes (offset 96): x = metallic, y = roughness, z = ao, w = has_light
    vec4 object_color;       // 16 bytes (offset 112): rgb = albedo, a = opacity
} u_push;

void main() {
    v_world_pos = in_position; // En coordenadas locales/modelo
    v_normal = in_normal;
    v_uv = in_uv;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
