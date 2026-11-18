#version 450

// Fragmento de las líneas del editor: color por vértice directo,
// con el alfa que trae el desvanecido por distancia.

layout(location = 0) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = v_color;
}
