#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(location = 0) out vec3 v_world_pos;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec2 v_uv;

layout(push_constant) uniform PushConstants {
    mat4 mvp;                // 64 bytes (offset 0)
    mat4 model;              // 64 bytes (offset 64)
    vec4 light_dir;          // 16 bytes (offset 128): xyz = light_dir, w = light_intensity
    vec4 light_color;        // 16 bytes (offset 144): xyz = light_color, w = ambient_intensity
    vec4 material_params;    // 16 bytes (offset 160): x = metallic, y = roughness, z = ao, w = has_light
    vec4 object_color;       // 16 bytes (offset 176): rgb = albedo, a = opacity
    vec4 camera_pos;         // 16 bytes (offset 192): xyz = posición de cámara en mundo
    vec4 lm_params;          // x = índice de la entidad en el UBO del lightmap (-1 = sin bake), y = tamaño del atlas en px
} u_push;

void main() {
    // Posición real en espacio de mundo. Antes se enviaba in_position tal cual,
    // pese a llamarse v_world_pos: era espacio local, así que el fragment shader
    // iluminaba con coordenadas equivocadas en cuanto el objeto no estaba en el
    // origen.
    v_world_pos = (u_push.model * vec4(in_position, 1.0)).xyz;

    // Normal a espacio de mundo con la inversa traspuesta de la 3x3. Multiplicar
    // por mat3(model) directamente sesga las normales cuando la escala no es
    // uniforme.
    mat3 normal_matrix = transpose(inverse(mat3(u_push.model)));
    v_normal = normal_matrix * in_normal;

    v_uv = in_uv;
    gl_Position = u_push.mvp * vec4(in_position, 1.0);
}
