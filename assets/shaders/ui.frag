#version 450

// Fragmento del pase 2D del Canvas: TODO pasa por el atlas de fuente.
// Los rectángulos sólidos apuntan su UV a un texel opaco reservado del
// atlas (alfa = 1) y los glifos a su cobertura (alfa = cobertura); el
// color final es el color del vértice modulado por ese alfa.

layout(set = 0, binding = 0) uniform sampler2D atlas_tex;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    float coverage = texture(atlas_tex, v_uv).a;
    out_color = vec4(v_color.rgb, v_color.a * coverage);
}
