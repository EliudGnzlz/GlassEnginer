#version 450

layout(location = 0) out vec3 v_color;

const vec3 POSITIONS[3] = vec3[3](
    vec3( 0.0, -0.5, 0.0),
    vec3( 0.5,  0.5, 0.0),
    vec3(-0.5,  0.5, 0.0)
);

const vec3 COLORS[3] = vec3[3](
    vec3(0.18, 0.53, 0.95), // Azul GlassEngine
    vec3(0.24, 0.82, 0.63), // Esmeralda GlassEngine
    vec3(0.96, 0.32, 0.44)  // Carmesí GlassEngine
);

void main() {
    gl_Position = vec4(POSITIONS[gl_VertexIndex], 1.0);
    v_color = COLORS[gl_VertexIndex];
}
