#version 450

// Variante CON PIEL de outline.vert para el contorno de selección de la
// vista de Escena: mismos inputs y mismo SSBO de articulaciones (set 1)
// que pbr_skinned.vert, con la misma mezcla de piel. La posición YA
// deformada se expande a lo largo de la normal YA deformada por el grosor
// que llega en material_params.x, y sale el color plano (object_color)
// para outline.frag. Mismo bloque de push constants que outline.vert.

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;
layout(location = 3) in uvec4 in_joints;
layout(location = 4) in vec4 in_weights;

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

layout(set = 1, binding = 0, std430) readonly buffer JointMatrices {
    mat4 joint_matrices[];
} u_joints;

void main() {
    float weight_sum = in_weights.x + in_weights.y + in_weights.z + in_weights.w;

    // Misma mezcla lineal que pbr_skinned.vert: un vértice con la suma de
    // pesos a cero (primitiva rígida dentro del modelo con piel) pasa sin
    // deformar.
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

    // La mezcla de matrices acorta la normal donde las influencias
    // divergen: se renormaliza para que el grosor sea el pedido. Una
    // normal degenerada no desplaza el vértice.
    vec3 skinned_normal = mat3(skin_matrix) * in_normal;
    float n_len = length(skinned_normal);
    vec3 n = n_len > 1e-6 ? skinned_normal / n_len : vec3(0.0);

    vec3 displaced = skinned_position.xyz + n * u_push.material_params.x;
    v_color = u_push.object_color;
    gl_Position = u_push.mvp * vec4(displaced, 1.0);
}
