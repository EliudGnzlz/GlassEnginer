#version 450

// Vértice de las líneas del editor (rejilla de la vista de escena):
// posición en mundo y color por vértice (el desvanecido con la
// distancia ya viene aplicado en el alfa). Solo se usa el MVP del
// bloque de push constants de siempre; el pipeline es LINE_LIST
// con test de profundidad, para que los modelos oculten la rejilla.

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec4 in_color;

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
    v_color = in_color;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
