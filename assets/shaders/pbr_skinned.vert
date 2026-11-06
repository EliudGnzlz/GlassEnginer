#version 450

// Variante CON PIEL de pbr.vert: mismo fragment (pbr.frag), mismos push
// constants y mismo set 0 (textura base). Lo único nuevo es el formato de
// vértice (articulaciones y pesos en locations 3 y 4) y el SSBO de
// matrices de articulación en set 1, que la CPU reescribe cada frame con
// la pose evaluada. Las mallas sin piel NO usan este shader: van por
// pbr.vert y su camino no cambia en nada.

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;
layout(location = 3) in uvec4 in_joints;
layout(location = 4) in vec4 in_weights;

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
} u_push;

layout(set = 1, binding = 0, std430) readonly buffer JointMatrices {
    mat4 joint_matrices[];
} u_joints;

void main() {
    float weight_sum = in_weights.x + in_weights.y + in_weights.z + in_weights.w;

    // Mezcla lineal de las 4 influencias. Un vértice con la suma de pesos
    // a cero es un vértice sin piel dentro de un modelo con piel (una
    // primitiva rígida mezclada en el mismo .glb): pasa sin deformar.
    mat4 skin_matrix;
    if (weight_sum > 1e-6) {
        skin_matrix =
            in_weights.x * u_joints.joint_matrices[in_joints.x] +
            in_weights.y * u_joints.joint_matrices[in_joints.y] +
            in_weights.z * u_joints.joint_matrices[in_joints.z] +
            in_weights.w * u_joints.joint_matrices[in_joints.w];
    } else {
        skin_matrix = mat4(1.0);
    }

    vec4 skinned_position = skin_matrix * vec4(in_position, 1.0);

    // Normal deformada con la 3x3 de la matriz mezclada, sin inversa
    // traspuesta: exacto mientras las articulaciones no lleven escala no
    // uniforme (lo habitual en rigs de personaje). Después se le aplica la
    // matriz de normales del modelo, igual que en pbr.vert.
    vec3 skinned_normal = mat3(skin_matrix) * in_normal;

    v_world_pos = (u_push.model * skinned_position).xyz;

    mat3 normal_matrix = transpose(inverse(mat3(u_push.model)));
    v_normal = normal_matrix * skinned_normal;

    v_uv = in_uv;
    gl_Position = u_push.mvp * skinned_position;
}
