#version 450

layout(location = 0) in vec3 v_world_pos;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec2 v_uv;

layout(location = 0) out vec4 out_color;

layout(push_constant) uniform PushConstants {
    mat4 mvp;                // 64 bytes
    vec4 light_dir;          // 16 bytes: xyz = light_dir, w = light_intensity
    vec4 light_color;        // 16 bytes: xyz = light_color, w = ambient_intensity
    vec4 material_params;    // 16 bytes: x = metallic, y = roughness, z = ao, w = has_light
    vec4 object_color;       // 16 bytes: rgb = albedo, a = opacity
} u_push;

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
    vec3 albedo = u_push.object_color.rgb;
    float metallic = clamp(u_push.material_params.x, 0.0, 1.0);
    float roughness = clamp(u_push.material_params.y, 0.05, 1.0);
    float ao = clamp(u_push.material_params.z, 0.0, 1.0);
    float has_light = u_push.material_params.w;

    vec3 N = normalize(v_normal);
    vec3 L = normalize(u_push.light_dir.xyz);
    // Aproximación de vista ortogonal / central
    vec3 V = normalize(vec3(0.0, 0.0, 1.0)); 
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
    vec3 radiance = u_push.light_color.rgb * u_push.light_dir.w * has_light;
    vec3 Lo = (kD * albedo / PI + specular) * radiance * NdotL;

    // Iluminación ambiental base ponderada por AO
    float ambient_intensity = u_push.light_color.w;
    vec3 ambient = vec3(ambient_intensity) * albedo * ao;

    vec3 final_color = ambient + Lo;

    // Reinhard tone mapping y corrección gamma
    final_color = final_color / (final_color + vec3(1.0));
    final_color = pow(final_color, vec3(1.0 / 2.2));

    out_color = vec4(final_color, u_push.object_color.a);
}
