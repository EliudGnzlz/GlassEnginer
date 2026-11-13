#version 450

layout(location = 0) in vec3 v_world_pos;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec2 v_uv;

layout(location = 0) out vec4 out_color;

layout(push_constant) uniform PushConstants {
    mat4 mvp;                // 64 bytes (offset 0)
    mat4 model;              // 64 bytes (offset 64): matriz de modelo
    vec4 light_dir;          // 16 bytes (offset 128): xyz = light_dir, w = ambient
    vec4 light_color;        // 16 bytes (offset 144): xyz = color*intensidad, w = reservado
    vec4 material_params;    // 16 bytes (offset 160): x = metallic, y = roughness, z = ao, w = has_light
    vec4 object_color;       // 16 bytes (offset 176): rgb = albedo, a = opacity
    vec4 camera_pos;         // 16 bytes (offset 192): xyz = posición de cámara en mundo
    vec4 lm_params;          // 16 bytes (offset 208): sin uso en legacy; mantiene los offsets del bloque común
    vec4 emissive;           // 16 bytes (offset 224): rgb = color emisivo, w = intensidad
    vec4 uv_transform;       // 16 bytes (offset 240): xy = tiling, zw = offset
} u_push;

// Textura base (baseColor) del material glTF. Cuando la entidad no tiene
// textura, el motor enlaza una textura blanca 1x1 y el resultado es el
// color plano de siempre.
layout(set = 0, binding = 0) uniform sampler2D base_color_tex;

// Mapa de sombras del sol (bloque de gráficos): UBO de frame con la VP
// ortográfica de la luz y un sampler de COMPARACIÓN (el hardware filtra
// la comparación: un tap devuelve ya el factor de luz en ese téxel).
// shadow_params: x = 1/tamaño, y = bias de profundidad, z = bias de
// normal (m), w = 1 si las sombras están activas este frame.
layout(set = 2, binding = 0) uniform ShadowUbo {
    mat4 light_vp;
    vec4 shadow_params;
    // Niebla (Fase 2): rgb = color, w = inicio; en fog_far_mode:
    // x = distancia total, y = 1 si activa.
    vec4 fog_color_dist;
    vec4 fog_far_mode;
} u_shadow;
layout(set = 2, binding = 1) uniform sampler2DShadow shadow_map;

// Luces puntuales del frame (bloque de iluminación): hasta 8 luces
// Point con el mismo BRDF que la direccional. Sin sombras en esta v1.
// count = 0 deja el resultado idéntico al anterior a este bloque.
layout(set = 3, binding = 0, std140) uniform PointLights {
    vec4 pos_range[8];        // xyz = posición mundo, w = rango
    vec4 color_intensity[8];  // xyz = color, w = intensidad
    vec4 shadow[8];           // Fase 2: x = cubo o -1, y = lejos, z = bias, w = cerca
    vec4 dir_cos[8];          // Spot: xyz = dirección mundo, w = cos(ángulo exterior)
    vec4 cone[8];             // Spot: x = cos(ángulo interior), y = 1 si Spot
    uint count;               // nº de luces válidas [0..8]
    uint _pad0;
    uint _pad1;
    uint _pad2;
} u_point_lights;

// Atlas de sombras puntuales (Fase 2): 4 cubos compartidos; el índice
// de cubo de cada luz va en u_point_lights.shadow[i].x (-1 = sin sombra).
layout(set = 2, binding = 2) uniform samplerCubeArray u_point_shadow_cubes;

// Factor de sombra de la luz puntual i para el fragmento: compara la
// profundidad NDC del fragmento en la proyección del cubo (90°, cerca
// en shadow.w, lejos en shadow.y) contra la guardada en el atlas.
float point_shadow_factor(int i) {
    vec4 sh = u_point_lights.shadow[i];
    if (sh.x < -0.5) return 1.0;
    vec3 dir = v_world_pos - u_point_lights.pos_range[i].xyz;
    float dist = length(dir);
    if (dist > sh.y || dist < 0.0001) return 1.0;
    float current = sh.y * (dist - sh.w) / ((sh.y - sh.w) * dist);
    float sampled = texture(u_point_shadow_cubes, vec4(dir, int(sh.x + 0.5))).r;
    return (current - sh.z > sampled) ? 0.0 : 1.0;
}


// Factor de luz (1 = a pleno sol, 0 = en sombra) con PCF 3x3. La sombra
// se desvanece junto al borde de la caja ortográfica para que nada
// aparezca/desaparezca de golpe al cruzar el límite, y todo fragmento
// fuera de la caja (o tras el plano lejano de la luz) queda a pleno sol.
float shadow_factor(vec3 world_pos, vec3 n) {
    if (u_shadow.shadow_params.w < 0.5) return 1.0;
    vec3 sp = world_pos + n * u_shadow.shadow_params.z;
    vec4 lc = u_shadow.light_vp * vec4(sp, 1.0);
    vec3 proj = lc.xyz / lc.w;
    if (proj.z < 0.0 || proj.z > 1.0) return 1.0;
    vec2 uv = proj.xy * 0.5 + 0.5;
    float edge = max(abs(proj.x), abs(proj.y));
    float fade = 1.0 - smoothstep(0.85, 1.0, edge);
    if (fade <= 0.0) return 1.0;
    float ref = proj.z - u_shadow.shadow_params.y;
    float texel = u_shadow.shadow_params.x;
    float s = 0.0;
    for (int x = -1; x <= 1; x++) {
        for (int y = -1; y <= 1; y++) {
            s += texture(shadow_map, vec3(uv + vec2(float(x), float(y)) * texel, ref));
        }
    }
    return mix(1.0, s / 9.0, fade);
}

const float PI = 3.14159265359;

// Normal Distribution Function (GGX / Trowbridge-Reitz)
float DistributionGGX(vec3 N, vec3 H, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float NdotH = max(dot(N, H), 0.0);
    float NdotH2 = NdotH * NdotH;

    float num = a2;
    float denom = (NdotH2 * (a2 - 1.0) + 1.0);
    denom = PI * denom * denom;

    return num / max(denom, 0.0001);
}

// Geometry Function (Schlick-GGX)
float GeometrySchlickGGX(float NdotV, float roughness) {
    float r = (roughness + 1.0);
    float k = (r * r) / 8.0;

    float num = NdotV;
    float denom = NdotV * (1.0 - k) + k;

    return num / max(denom, 0.0001);
}

// Geometry Smith
float GeometrySmith(vec3 N, vec3 V, vec3 L, float roughness) {
    float NdotV = max(dot(N, V), 0.0);
    float NdotL = max(dot(N, L), 0.0);
    float ggx2 = GeometrySchlickGGX(NdotV, roughness);
    float ggx1 = GeometrySchlickGGX(NdotL, roughness);

    return ggx1 * ggx2;
}

// Fresnel Schlick
vec3 fresnelSchlick(float cosTheta, vec3 F0) {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cosTheta, 0.0, 1.0), 5.0);
}

void main() {
    vec2 uv_base = v_uv * u_push.uv_transform.xy + u_push.uv_transform.zw;
    vec3 albedo = u_push.object_color.rgb * texture(base_color_tex, uv_base).rgb;
    float metallic = clamp(u_push.material_params.x, 0.0, 1.0);
    float roughness = clamp(u_push.material_params.y, 0.05, 1.0);
    float ao = clamp(u_push.material_params.z, 0.0, 1.0);
    float has_light = u_push.material_params.w;

    vec3 N = normalize(v_normal);
    vec3 L = normalize(u_push.light_dir.xyz);
    // Vector de vista real: desde la superficie hacia la cámara. Antes era una
    // constante (0,0,1), así que el término especular no dependía del punto de
    // vista y los reflejos no se movían al orbitar.
    vec3 V = normalize(u_push.camera_pos.xyz - v_world_pos);
    vec3 H = normalize(V + L);

    // Reflectancia base al 0% de ángulo
    vec3 F0 = vec3(0.04);
    F0 = mix(F0, albedo, metallic);

    // Ecuación de reflectancia Cook-Torrance
    float NDF = DistributionGGX(N, H, roughness);
    float G = GeometrySmith(N, V, L, roughness);
    vec3 F = fresnelSchlick(max(dot(H, V), 0.0), F0);

    vec3 numerator = NDF * G * F;
    float denominator = 4.0 * max(dot(N, V), 0.0) * max(dot(N, L), 0.0) + 0.0001;
    vec3 specular = numerator / denominator;

    // Conservación de energía
    vec3 kS = F;
    vec3 kD = vec3(1.0) - kS;
    kD *= (1.0 - metallic);

    float NdotL = max(dot(N, L), 0.0);

    // Iluminación directa solar
    vec3 radiance = u_push.light_color.rgb * has_light;
    vec3 Lo = (kD * albedo / PI + specular) * radiance * NdotL;
    // La sombra del sol atenúa SOLO la luz directa: el ambiente
    // hemisférico sigue llegando (es luz rebotada/difusa del cielo).
    Lo *= shadow_factor(v_world_pos, N) * has_light + (1.0 - has_light);

    // Iluminación ambiental hemisférica ponderada por AO: las caras
    // hacia el cielo reciben más ambiente (con un tinte frío) que las
    // que miran al suelo (tinte cálido y menor factor). Antes el
    // ambiente era plano — el mismo para todas las caras — y las caras
    // en sombra salían casi tan brillantes como las iluminadas: la
    // escena se veía lavada y sin volumen.
    float ambient_intensity = u_push.light_dir.w;
    float hemi_t = N.y * 0.5 + 0.5;
    float hemi_factor = mix(0.45, 1.0, hemi_t);
    vec3 ambient_color = mix(vec3(0.62, 0.58, 0.55), vec3(0.82, 0.88, 1.0), hemi_t);
    vec3 ambient = ambient_color * (ambient_intensity * hemi_factor) * albedo * ao;

        // --- Puntuales: mismo BRDF Cook-Torrance que la direccional ---------
    // Atenuación: corte suave a 0 en el rango y caída 1/d², coherente
    // con el tono físico del PBR y sin borde duro.
    // Con sombra: hasta 4 puntuales la proyectan (atlas de cubos).
    vec3 Lo_points = vec3(0.0);
    for (int i = 0; i < int(u_point_lights.count); ++i) {
        vec3 to_light = u_point_lights.pos_range[i].xyz - v_world_pos;
        float d = length(to_light);
        float range = max(u_point_lights.pos_range[i].w, 0.0001);
        if (d >= range) continue;

        vec3 Lp = to_light / max(d, 0.0001);
        float atten = clamp(1.0 - pow(d / range, 4.0), 0.0, 1.0);
        atten *= atten;
        atten /= (d * d + 1.0);

        vec3 Hp = normalize(V + Lp);
        vec3 Fp = fresnelSchlick(max(dot(Hp, V), 0.0), F0);

        float NDFp = DistributionGGX(N, Hp, roughness);
        float Gp = GeometrySmith(N, V, Lp, roughness);
        vec3 specular_p = (NDFp * Gp * Fp) /
            (4.0 * max(dot(N, V), 0.0) * max(dot(N, Lp), 0.0) + 0.0001);

        vec3 kSp = Fp;
        vec3 kDp = vec3(1.0) - kSp;
        kDp *= (1.0 - metallic);

        float NdotLp = max(dot(N, Lp), 0.0);
        vec3 radiance_p = u_point_lights.color_intensity[i].rgb
                        * u_point_lights.color_intensity[i].w
                        * atten;
        // Cono Spot: fuera del ángulo exterior no llega luz; entre el
        // interior y el exterior cae lineal en coseno (penumbra).
        if (u_point_lights.cone[i].y > 0.5) {
            float cos_a = dot(-Lp, u_point_lights.dir_cos[i].xyz);
            float f_cono = clamp(
                (cos_a - u_point_lights.dir_cos[i].w)
                    / max(u_point_lights.cone[i].x - u_point_lights.dir_cos[i].w, 0.0001),
                0.0, 1.0);
            radiance_p *= f_cono;
        }
        radiance_p *= point_shadow_factor(i);

        Lo_points += (kDp * albedo / PI + specular_p) * radiance_p * NdotLp;
    }

    vec3 final_color = ambient + Lo + Lo_points * has_light;

    // Niebla analítica lineal (Fase 2): mezcla hacia el color de niebla
    // entre la distancia de inicio y la total, con la distancia al ojo.
    if (u_shadow.fog_far_mode.y > 0.5) {
        float fog_dist = length(u_push.camera_pos.xyz - v_world_pos);
        float fog_f = clamp(
            (fog_dist - u_shadow.fog_color_dist.w)
                / max(u_shadow.fog_far_mode.x - u_shadow.fog_color_dist.w, 0.001),
            0.0, 1.0);
        final_color = mix(final_color, u_shadow.fog_color_dist.rgb, fog_f);
    }

    // Mapeo de tono ACES (aproximación de Narkowicz) con exposición.
    // Sustituye a Reinhard (c/(c+1)), que era parte del problema:
    // comprimía el contraste entre caras y desaturaba los colores
    // hacia el gris. ACES conserva tono y saturación en los medios
    // tonos y rueda los altos de forma fílmica.
    //
    // SIN corrección gamma aquí a propósito: la presentación ya codifica
    // a sRGB aguas abajo (el swapchain es *_SRGB y el blit/egui aplica la
    // conversión). El pow(1/2.2) que había antes sumaba una SEGUNDA gamma
    // sobre la de presentación: medido con un shader de depuración, un
    // valor lineal 0,5 llegaba a pantalla como 0,87 en vez de 0,73 — las
    // sombras se levantaban y todo se veía lavado. El shader entrega el
    // color lineal mapeado y la presentación hace la única gamma.
    final_color *= 1.5;
    final_color = clamp(
        (final_color * (2.51 * final_color + 0.03)) / (final_color * (2.43 * final_color + 0.59) + 0.14),
        0.0,
        1.0
    );

    final_color += u_push.emissive.rgb * u_push.emissive.w;
    out_color = vec4(final_color, u_push.object_color.a);
}
