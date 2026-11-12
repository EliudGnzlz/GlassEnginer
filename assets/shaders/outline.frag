#version 450

// Fragmento del pase de contorno toon (Fase 2S, S5): color plano.

layout(location = 0) in vec4 v_color;
layout(location = 0) out vec4 out_color;

void main() {
    out_color = v_color;
}
