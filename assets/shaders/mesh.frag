#version 450

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;

layout(location = 0) out vec4 out_color;

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
    vec3 N = normalize(v_normal);
    vec3 L = normalize(u_push.light_dir.xyz);
    float ambient = u_push.light_dir.w;
    // has_light vive en material_params.w (convención PBR). Antes se leía de
    // light_color.w, que duplicaba el mismo bit: dos fuentes para un dato es
    // una desincronización esperando a ocurrir.
    float has_light = u_push.material_params.w;

    float diff = max(dot(N, L), 0.0) * has_light;
    vec3 light_contrib = u_push.light_color.xyz * diff;
    vec3 final_color = u_push.object_color.xyz * (ambient + light_contrib);

    out_color = vec4(final_color, 1.0);
}
