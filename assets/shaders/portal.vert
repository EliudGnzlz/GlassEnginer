#version 450
// Superficie de portal: posición real con el MVP de la vista
// y posición en el clip de la CÁMARA VIRTUAL como varying
// proyectivo (la textura del otro lado se muestrea con él).
// El anillo del borde se calcula desde la posición LOCAL
// normalizada (local_half), no desde el UV: el cubo glb
// trae UV de atlas y el anillo salía deformado.
layout(location = 0) in vec3 inPos;

layout(push_constant) uniform PC {
    mat4 mvp;
    mat4 mvp_virtual;
    vec4 rim;
    vec2 local_half;
    vec2 _pad;
} pc;

layout(location = 0) out vec4 v_clip_virtual;
layout(location = 1) out vec2 v_local;
layout(location = 2) out vec4 v_rim;

void main() {
    vec4 p = vec4(inPos, 1.0);
    gl_Position = pc.mvp * p;
    v_clip_virtual = pc.mvp_virtual * p;
    v_local = inPos.xy / pc.local_half;
    v_rim = pc.rim;
}
