#version 450

// Pase 2D del Canvas de juego (Screen Space Overlay) sin egui.
// Los vértices llegan en píxeles de pantalla (origen arriba a la izquierda,
// Y hacia abajo, como el resto de la UI del motor) y se convierten a NDC
// con el tamaño del framebuffer que viaja en el push constant.

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec4 in_color;

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;

layout(push_constant) uniform PushConstants {
    vec2 screen_size; // 8 bytes (offset 0): tamaño del target en píxeles
} u_push;

void main() {
    vec2 ndc = vec2(
        in_pos.x / u_push.screen_size.x * 2.0 - 1.0,
        in_pos.y / u_push.screen_size.y * 2.0 - 1.0
    );
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = in_uv;
    v_color = in_color;
}
