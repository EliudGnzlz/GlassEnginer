#version 450

// Fragmento del pase 2D de IMÁGENES del Canvas (UI v2): a diferencia
// del pase de fuente (que solo usa el alfa como cobertura), aquí la
// textura aporta el color completo y el vértice solo tiñe.

layout(set = 0, binding = 0) uniform sampler2D atlas_tex;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = texture(atlas_tex, v_uv) * v_color;
}
