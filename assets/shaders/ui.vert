#version 450
// Vértice UI V2: convierte píxeles a NDC y reenvía todo el estado de pintado.
layout(location = 0)  in vec2  a_pos;
layout(location = 1)  in vec2  a_uv;
layout(location = 2)  in vec4  a_color;
layout(location = 3)  in vec2  a_local;
layout(location = 4)  in vec2  a_tamano;
layout(location = 5)  in vec4  a_radios;
layout(location = 6)  in vec4  a_clip;
layout(location = 7)  in vec4  a_pack1;
layout(location = 8)  in vec4  a_pack2;
layout(location = 9)  in vec4  a_grad_c0;
layout(location = 10) in vec4  a_grad_c1;
layout(location = 11) in vec4  a_clip_radios; // M3 (V5c): radios del clip

layout(push_constant) uniform Push { vec2 screen_size; } pc;

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;
layout(location = 2) out vec2 v_local;
layout(location = 3) out vec2 v_tamano;
layout(location = 4) out vec4 v_radios;
layout(location = 5) out vec4 v_clip;
layout(location = 6) out vec4 v_pack1;
layout(location = 7) out vec4 v_pack2;
layout(location = 8) out vec4 v_grad_c0;
layout(location = 9) out vec4 v_grad_c1;
layout(location = 10) out vec4 v_clip_radios;

void main() {
    // Píxeles (origen arriba-izquierda) -> NDC. En Vulkan el framebuffer
    // tiene la Y hacia ABAJO (NDC -1 = fila 0 = arriba), así que NO se
    // niega la Y: el shader viejo hacía exactamente esto y el Canvas V1
    // se veía derecho. Negarla (estilo OpenGL) voltea toda la UI.
    vec2 ndc = (a_pos / pc.screen_size) * 2.0 - 1.0;
    gl_Position = vec4(ndc, 0.0, 1.0);

    v_uv      = a_uv;
    v_color   = a_color;
    v_local   = a_local;
    v_tamano  = a_tamano;
    v_radios  = a_radios;
    v_clip    = a_clip;
    v_pack1   = a_pack1;
    v_pack2   = a_pack2;
    v_grad_c0 = a_grad_c0;
    v_grad_c1 = a_grad_c1;
    v_clip_radios = a_clip_radios;
}
