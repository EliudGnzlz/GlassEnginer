#version 450

// Por vértice (binding 0): el formato de siempre de crate::mesh::Vertex.
layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

// Por instancia (binding 1, input rate INSTANCE, stride 96 B): InstanceData.
// El mat4 ocupa las locations 3..6 (una columna por location).
layout(location = 3) in mat4 in_model;        // offset 0
layout(location = 7) in vec4 in_object_color; // offset 64: rgb = albedo, a = opacity
layout(location = 8) in vec4 in_lm_params;    // offset 80: x = índice lightmap (-1 = sin bake), y = tamaño del atlas px

layout(location = 0) out vec3 v_world_pos;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec2 v_uv;
// Constantes en toda la instancia: `flat` evita interpolarlas (el índice
// del lightmap llega exacto y el alfa sin error de interpolación).
layout(location = 3) flat out vec4 v_object_color;
layout(location = 4) flat out vec4 v_lm_params;

// Push constants del camino instanciado (160 B): solo campos por vista y
// material. El vertex solo lee vp; el bloque se declara entero para que
// coincida con el del fragment.
layout(push_constant) uniform PushConstants {
    mat4 vp;                 // 64 bytes (offset 0): proyección * vista
    vec4 light_dir;          // 16 bytes (offset 64)
    vec4 light_color;        // 16 bytes (offset 80)
    vec4 material_params;    // 16 bytes (offset 96)
    vec4 camera_pos;         // 16 bytes (offset 112)
    vec4 emissive;           // 16 bytes (offset 128)
    vec4 uv_transform;       // 16 bytes (offset 144)
} u_push;

void main() {
    // Posición en espacio de mundo con la matriz de modelo de la instancia.
    vec4 world = in_model * vec4(in_position, 1.0);
    v_world_pos = world.xyz;

    // Normal a espacio de mundo con la inversa traspuesta de la 3x3, igual
    // que pbr.vert: mat3(model) a secas sesga las normales con escala no
    // uniforme.
    mat3 normal_matrix = transpose(inverse(mat3(in_model)));
    v_normal = normal_matrix * in_normal;

    v_uv = in_uv;
    v_object_color = in_object_color;
    v_lm_params = in_lm_params;

    gl_Position = u_push.vp * world;
}
