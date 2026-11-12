#version 450

// Vértice del pase de contorno toon (Fase 2S, S5): expande la malla a lo
// largo de su normal en espacio de objeto por el grosor que llega en
// material_params.x y deja el color plano del contorno (object_color)
// para el fragmento. Mismo bloque de push constants que el PBR.

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec4 v_color;

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
    vec3 displaced = in_position + in_normal * u_push.material_params.x;
    v_color = u_push.object_color;
    gl_Position = u_push.mvp * vec4(displaced, 1.0);
}
