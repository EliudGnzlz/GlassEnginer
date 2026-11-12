#version 450
// Fragmento de profundidad con recorte alfa para el pase de sombras de
// los grafos (Fase 2S, S3): acompaña a la etapa de vértices del grafo
// cuando este declara la propiedad especial `alpha_clip`. El umbral
// llega en material_params.x de los push constants (lo resuelve la app
// desde el esquema del grafo y los overrides de la entidad); la textura
// es la base del material (set 0, binding 0, el mismo set que el pase
// forward del grafo).
layout(location = 2) in vec2 v_uv;
layout(set = 0, binding = 0) uniform sampler2D base_color_tex;
layout(push_constant) uniform PushConstants {
    mat4 mvp;
    mat4 model;
    vec4 light_dir;
    vec4 light_color;
    vec4 material_params;
    vec4 object_color;
    vec4 camera_pos;
} u_push;

void main() {
    if (texture(base_color_tex, v_uv).a < u_push.material_params.x) {
        discard;
    }
}
