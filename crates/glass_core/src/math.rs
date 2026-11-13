//! Rotational math shared by every boundary of the engine.
//!
//! # Why this module exists
//!
//! The engine stores entity orientation as **Euler angles in degrees**
//! (`TransformData::rotation: [f32; 3]`) because that is what the editor
//! Inspector and the gizmos edit, and what is serialized to `.scene.json`.
//! The scripting frontier, however, speaks **quaternions**
//! (`TransformFfi::rotation: [f32; 4]`) because that is what C# exposes as
//! `Quaternion` and what Rapier3D returns from `RigidBody::rotation()`.
//!
//! Before this module existed, every crossing of that frontier either dropped
//! the rotation entirely or substituted the identity quaternion, which is why
//! rotating an entity in the editor (or letting physics spin a body) had no
//! visible effect. These two functions are the single conversion point.
//!
//! # Rotation order
//!
//! The convention is `R = Rz(gamma) * Ry(beta) * Rx(alpha)`, i.e. the X rotation
//! is applied first in the parent frame, then Y, then Z ("extrinsic XYZ",
//! equivalently "intrinsic ZYX"). This matches the matrix previously assembled
//! inline in `glass_app`'s `model_matrix_trs_euler`, so existing scenes keep
//! rendering identically.
//!
//! Note: Unity's `Transform.eulerAngles` uses intrinsic Z-X-Y. That is a
//! deliberate known deviation, documented here rather than silently changed,
//! because switching orders would rotate every already-authored scene.

/// Converts Euler angles in **degrees** to a normalized quaternion `[x, y, z, w]`.
pub fn euler_deg_to_quat(euler_deg: [f32; 3]) -> [f32; 4] {
    let half_x = (euler_deg[0].to_radians()) * 0.5;
    let half_y = (euler_deg[1].to_radians()) * 0.5;
    let half_z = (euler_deg[2].to_radians()) * 0.5;

    let (sx, cx) = half_x.sin_cos();
    let (sy, cy) = half_y.sin_cos();
    let (sz, cz) = half_z.sin_cos();

    // Individual axis quaternions.
    let qx = [sx, 0.0, 0.0, cx];
    let qy = [0.0, sy, 0.0, cy];
    let qz = [0.0, 0.0, sz, cz];

    // R = Rz * Ry * Rx  =>  q = qz * qy * qx
    let q = quat_mul(qz, quat_mul(qy, qx));
    quat_normalize(q)
}

/// Converts a quaternion `[x, y, z, w]` back to Euler angles in **degrees**.
///
/// Gimbal lock (|y| approaching 90 degrees) is handled explicitly by pinning the
/// Z angle to zero, which is the standard resolution and keeps the result stable
/// instead of producing NaN.
pub fn quat_to_euler_deg(quat: [f32; 4]) -> [f32; 3] {
    let q = quat_normalize(quat);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);

    // Rotation matrix built directly from the quaternion, row-major.
    let m00 = 1.0 - 2.0 * (y * y + z * z);
    let _m01 = 2.0 * (x * y - z * w);
    let _m02 = 2.0 * (x * z + y * w);

    let m10 = 2.0 * (x * y + z * w);
    let m11 = 1.0 - 2.0 * (x * x + z * z);
    let m12 = 2.0 * (y * z - x * w);

    let m20 = 2.0 * (x * z - y * w);
    let m21 = 2.0 * (y * z + x * w);
    let m22 = 1.0 - 2.0 * (x * x + y * y);

    // For R = Rz * Ry * Rx: m20 = -sin(y), m21 = sin(x)cos(y), m22 = cos(x)cos(y).
    let sin_y = (-m20).clamp(-1.0, 1.0);
    let angle_y = sin_y.asin();

    let (angle_x, angle_z) = if m20.abs() < 0.999_999 {
        (m21.atan2(m22), m10.atan2(m00))
    } else {
        // cos(y) ~= 0: X and Z become a single coupled rotation. Pin Z to zero.
        ((-m12).atan2(m11), 0.0)
    };

    [
        angle_x.to_degrees(),
        angle_y.to_degrees(),
        angle_z.to_degrees(),
    ]
}

/// Rotates a vector by a quaternion.
pub fn quat_rotate_vec3(quat: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let q = quat_normalize(quat);
    let u = [q[0], q[1], q[2]];
    let s = q[3];

    let uv = cross(u, v);
    let uuv = cross(u, uv);

    [
        v[0] + 2.0 * (s * uv[0] + uuv[0]),
        v[1] + 2.0 * (s * uv[1] + uuv[1]),
        v[2] + 2.0 * (s * uv[2] + uuv[2]),
    ]
}

/// Hamilton product of two quaternions `[x, y, z, w]`.
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);

    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Returns the quaternion with unit length, falling back to identity for
/// degenerate (zero-length) inputs so callers never propagate NaN.
pub fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !len_sq.is_finite() || len_sq < 1.0e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len_sq.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

/// Spherical linear interpolation between two quaternions `[x, y, z, w]`,
/// always along the SHORT arc: if the dot product is negative, `b` is
/// negated first (q and -q are the same rotation; without the flip the
/// interpolation takes the long way around and animations visibly detour).
/// Near-parallel inputs fall back to normalized lerp to avoid dividing by
/// sin(theta) ≈ 0. Used by the animation sampler (skinning block).
pub fn quat_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let a = quat_normalize(a);
    let mut b = quat_normalize(b);
    let mut cos_theta = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    // Epsilon: con claves casi ortogonales (giro de ~180 grados) el signo
    // del producto punto es ruido de coma flotante (cos(pi/2) en f32 da
    // -4,4e-8) y decidiría el sentido del giro al azar; solo se niega con
    // un margen claro, y cerca de 180 ambos arcos son equivalentes salvo
    // el hemisferio que traen las claves, que es la intención autorada.
    if cos_theta < -1.0e-6 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        cos_theta = -cos_theta;
    }
    if cos_theta > 1.0 - 1.0e-6 {
        return quat_normalize([
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
            a[3] + t * (b[3] - a[3]),
        ]);
    }
    let theta = cos_theta.acos();
    let sin_theta = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / sin_theta;
    let wb = (t * theta).sin() / sin_theta;
    quat_normalize([
        wa * a[0] + wb * b[0],
        wa * a[1] + wb * b[1],
        wa * a[2] + wb * b[2],
        wa * a[3] + wb * b[3],
    ])
}

/// Builds the upper-left 3x3 rotation matrix (row-major) for a quaternion.
///
/// Exposed so the renderer can build a model matrix straight from the physics
/// quaternion without a lossy detour through Euler angles.
pub fn quat_to_mat3(quat: [f32; 4]) -> [[f32; 3]; 3] {
    let q = quat_normalize(quat);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);

    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

// ---------------------------------------------------------------------------
// 4x4 matrices
// ---------------------------------------------------------------------------
//
// Layout: **column-major**, exactly as GLSL/SPIR-V expects, so a matrix can be
// uploaded straight into a `mat4` push-constant field with `bytemuck::bytes_of`
// and no transposition. Column `c` occupies elements `[4c..4c+4]`; column 3 is
// the translation.
//
// Element index of row `r`, column `c` is `4 * c + r`.

/// The 4x4 identity matrix in column-major order.
pub fn mat4_identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
}

/// Builds `M = T * R * S` from an entity's local transform.
///
/// `rot_deg` follows the module convention `R = Rz * Ry * Rx`. This is the same
/// matrix that `glass_app` previously assembled inline as
/// `model_matrix_trs_euler`; it now lives here so that the hierarchical world
/// transform and the flat rendering path cannot drift apart.
pub fn mat4_from_trs_euler(pos: [f32; 3], rot_deg: [f32; 3], scale: [f32; 3]) -> [f32; 16] {
    let rx = rot_deg[0].to_radians();
    let ry = rot_deg[1].to_radians();
    let rz = rot_deg[2].to_radians();

    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());

    // R = Rz * Ry * Rx, row-major coefficients.
    let r00 = cy * cz;
    let r01 = sx * sy * cz - cx * sz;
    let r02 = cx * sy * cz + sx * sz;

    let r10 = cy * sz;
    let r11 = sx * sy * sz + cx * cz;
    let r12 = cx * sy * sz - sx * cz;

    let r20 = -sy;
    let r21 = sx * cy;
    let r22 = cx * cy;

    // Column 0 = R * (sx,0,0); column 1 = R * (0,sy,0); column 2 = R * (0,0,sz).
    [
        r00 * scale[0],
        r10 * scale[0],
        r20 * scale[0],
        0.0,
        r01 * scale[1],
        r11 * scale[1],
        r21 * scale[1],
        0.0,
        r02 * scale[2],
        r12 * scale[2],
        r22 * scale[2],
        0.0,
        pos[0],
        pos[1],
        pos[2],
        1.0,
    ]
}

/// Multiplies two column-major 4x4 matrices, returning `a * b`.
///
/// Used to chain a child's local matrix onto its parent's world matrix:
/// `world_child = world_parent * local_child`.
pub fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0_f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[4 * k + r] * b[4 * c + k];
            }
            out[4 * c + r] = sum;
        }
    }
    out
}

/// Transforms a point (w = 1) by a column-major matrix, dropping the w divide.
///
/// Only intended for affine transforms (no perspective), which is all the
/// engine produces for world matrices.
pub fn mat4_transform_point(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// Reads the translation column of a column-major matrix.
pub fn mat4_translation(m: [f32; 16]) -> [f32; 3] {
    [m[12], m[13], m[14]]
}

/// Builds a column-major TRS matrix from a position, a quaternion `[i,j,k,w]`
/// and a per-axis scale.
///
/// Preferred over `mat4_from_trs_euler` on every runtime path: it skips the
/// Euler round-trip, so a rotation accumulated by a C# script or by Rapier is
/// composed exactly as the solver produced it, with no gimbal-lock snapping.
pub fn mat4_from_trs_quat(position: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> [f32; 16] {
    let q = quat_normalize(rotation);
    let (i, j, k, w) = (q[0], q[1], q[2], q[3]);

    let x2 = i + i;
    let y2 = j + j;
    let z2 = k + k;
    let xx = i * x2;
    let xy = i * y2;
    let xz = i * z2;
    let yy = j * y2;
    let yz = j * z2;
    let zz = k * z2;
    let wx = w * x2;
    let wy = w * y2;
    let wz = w * z2;

    let (sx, sy, sz) = (scale[0], scale[1], scale[2]);

    [
        (1.0 - (yy + zz)) * sx,
        (xy + wz) * sx,
        (xz - wy) * sx,
        0.0,
        (xy - wz) * sy,
        (1.0 - (xx + zz)) * sy,
        (yz + wx) * sy,
        0.0,
        (xz + wy) * sz,
        (yz - wx) * sz,
        (1.0 - (xx + yy)) * sz,
        0.0,
        position[0],
        position[1],
        position[2],
        1.0,
    ]
}

/// Extracts the per-axis scale from an affine column-major matrix.
///
/// The length of each basis column is the scale applied on that axis, since
/// `M = T * R * S` and `R` is orthonormal.
pub fn mat4_scale(m: [f32; 16]) -> [f32; 3] {
    let sx = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    let sy = (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt();
    let sz = (m[8] * m[8] + m[9] * m[9] + m[10] * m[10]).sqrt();
    [sx, sy, sz]
}

/// Extracts the rotation of an affine column-major matrix as a quaternion
/// `[i, j, k, w]`, normalising out any scale first.
///
/// Uses Shepperd's method: pick the largest diagonal term to avoid the
/// division by a near-zero `1 + trace` that destabilises the naive formula.
pub fn mat4_to_quat(m: [f32; 16]) -> [f32; 4] {
    let s = mat4_scale(m);
    let ix = if s[0] > 1.0e-8 { 1.0 / s[0] } else { 0.0 };
    let iy = if s[1] > 1.0e-8 { 1.0 / s[1] } else { 0.0 };
    let iz = if s[2] > 1.0e-8 { 1.0 / s[2] } else { 0.0 };

    // Rotation block, rows x columns (column-major source).
    let r00 = m[0] * ix;
    let r10 = m[1] * ix;
    let r20 = m[2] * ix;
    let r01 = m[4] * iy;
    let r11 = m[5] * iy;
    let r21 = m[6] * iy;
    let r02 = m[8] * iz;
    let r12 = m[9] * iz;
    let r22 = m[10] * iz;

    let trace = r00 + r11 + r22;

    let (i, j, k, w) = if trace > 0.0 {
        let root = (trace + 1.0).sqrt();
        let w = 0.5 * root;
        let inv = 0.5 / root;
        ((r21 - r12) * inv, (r02 - r20) * inv, (r10 - r01) * inv, w)
    } else if r00 > r11 && r00 > r22 {
        let root = (1.0 + r00 - r11 - r22).sqrt();
        let i = 0.5 * root;
        let inv = 0.5 / root;
        (i, (r01 + r10) * inv, (r02 + r20) * inv, (r21 - r12) * inv)
    } else if r11 > r22 {
        let root = (1.0 + r11 - r00 - r22).sqrt();
        let j = 0.5 * root;
        let inv = 0.5 / root;
        ((r01 + r10) * inv, j, (r12 + r21) * inv, (r02 - r20) * inv)
    } else {
        let root = (1.0 + r22 - r00 - r11).sqrt();
        let k = 0.5 * root;
        let inv = 0.5 / root;
        ((r02 + r20) * inv, (r12 + r21) * inv, k, (r10 - r01) * inv)
    };

    quat_normalize([i, j, k, w])
}

/// Inverts an affine column-major matrix (`M = T * R * S`, no projection).
///
/// Used to convert world-space results back into local space: a rigid body
/// simulated by Rapier reports a world transform, but the scene stores the
/// LOCAL transform of a child relative to its parent.
///
/// Returns `None` when the upper 3x3 block is singular (a zero scale on any
/// axis), because such a matrix has no inverse and silently producing garbage
/// would corrupt the entity's transform.
pub fn mat4_inverse_affine(m: [f32; 16]) -> Option<[f32; 16]> {
    // Upper-left 3x3 and its adjugate.
    let a00 = m[0];
    let a01 = m[4];
    let a02 = m[8];
    let a10 = m[1];
    let a11 = m[5];
    let a12 = m[9];
    let a20 = m[2];
    let a21 = m[6];
    let a22 = m[10];

    let c00 = a11 * a22 - a12 * a21;
    let c01 = a12 * a20 - a10 * a22;
    let c02 = a10 * a21 - a11 * a20;

    let det = a00 * c00 + a01 * c01 + a02 * c02;
    if det.abs() < 1.0e-12 {
        return None;
    }
    let inv_det = 1.0 / det;

    // Inverse of the 3x3, stored row-major in local variables.
    let i00 = c00 * inv_det;
    let i01 = (a02 * a21 - a01 * a22) * inv_det;
    let i02 = (a01 * a12 - a02 * a11) * inv_det;

    let i10 = c01 * inv_det;
    let i11 = (a00 * a22 - a02 * a20) * inv_det;
    let i12 = (a02 * a10 - a00 * a12) * inv_det;

    let i20 = c02 * inv_det;
    let i21 = (a01 * a20 - a00 * a21) * inv_det;
    let i22 = (a00 * a11 - a01 * a10) * inv_det;

    // Inverse translation is -(R^-1 * t).
    let t = [m[12], m[13], m[14]];
    let it0 = -(i00 * t[0] + i01 * t[1] + i02 * t[2]);
    let it1 = -(i10 * t[0] + i11 * t[1] + i12 * t[2]);
    let it2 = -(i20 * t[0] + i21 * t[1] + i22 * t[2]);

    Some([
        i00, i10, i20, 0.0, //
        i01, i11, i21, 0.0, //
        i02, i12, i22, 0.0, //
        it0, it1, it2, 1.0,
    ])
}

/// Vista-proyección ortográfica de la luz direccional para el mapa de
/// sombras (bloque de gráficos: sombras + post). Función pura, sin GPU.
///
/// - La caja mira desde `center - L * back` hacia `center`, con
///   semiextensión `half_extent` en X e Y del espacio de luz; el rango de
///   profundidad cubre de `-front` (detrás del centro) a `back` (hacia la
///   luz) medido sobre el eje de vista.
/// - **Snap a téxeles**: el centro, expresado en espacio de luz, se
///   redondea a múltiplos del tamaño de téxel (`2*half_extent/resolution`)
///   en X e Y, y el desplazamiento se deshace en espacio de mundo. Así la
///   retícula del mapa no se desliza bajo la cámara y las sombras no
///   «hierven» al moverla (la técnica estándar de los motores con una
///   sola cascada).
/// - La Y se invierte en la proyección, como en la perspectiva de la app
///   (NDC de Vulkan), y la profundidad queda en [0, 1] con el convenio
///   diestro de la casa (la cámara mira hacia -Z en su espacio).
pub fn directional_shadow_vp(
    center: [f32; 3],
    light_dir_to_sun: [f32; 3],
    half_extent: f32,
    resolution: u32,
    back: f32,
    front: f32,
) -> [f32; 16] {
    let l_len = (light_dir_to_sun[0] * light_dir_to_sun[0]
        + light_dir_to_sun[1] * light_dir_to_sun[1]
        + light_dir_to_sun[2] * light_dir_to_sun[2])
        .sqrt()
        .max(1.0e-6);
    let l = [
        light_dir_to_sun[0] / l_len,
        light_dir_to_sun[1] / l_len,
        light_dir_to_sun[2] / l_len,
    ];
    // Dirección de vista de la luz: del sol hacia la escena (-L).
    let f = [-l[0], -l[1], -l[2]];
    let up = if l[1].abs() > 0.99 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
    let r_raw = cross(f, up);
    let r_len = (r_raw[0] * r_raw[0] + r_raw[1] * r_raw[1] + r_raw[2] * r_raw[2])
        .sqrt()
        .max(1.0e-6);
    let r = [r_raw[0] / r_len, r_raw[1] / r_len, r_raw[2] / r_len];
    let u = cross(r, f);

    // Ojo de la luz: hacia el sol desde el centro (+L * back), mirando a
    // lo largo de f. Vista diestra espejo de la `look_at` de la app
    // (filas r, u, -f).
    let eye = [
        center[0] + l[0] * back,
        center[1] + l[1] * back,
        center[2] + l[2] * back,
    ];
    let mut tx = -(r[0] * eye[0] + r[1] * eye[1] + r[2] * eye[2]);
    let mut ty = -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]);
    let tz = f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2];
    // SNAP A TÉXELES: el centro, en espacio de vista de la luz, cae en
    // (r·center + tx, u·center + ty) — en general NO es (0,0), porque la
    // rotación mezcla las coordenadas del mundo. El muestreo del mapa
    // usa esas coordenadas de vista, así que son ELLAS las que hay que
    // anclar a la retícula: se desplaza la traslación lo justo para que
    // la posición del centro en vista sea un múltiplo exacto del téxel.
    // Al mover la cámara, el origen del mapa salta entonces de téxel en
    // téxel en vez de deslizarse, y las sombras no hierven.
    let texel = 2.0 * half_extent / resolution.max(1) as f32;
    let cx = r[0] * center[0] + r[1] * center[1] + r[2] * center[2] + tx;
    let cy = u[0] * center[0] + u[1] * center[1] + u[2] * center[2] + ty;
    tx += (cx / texel).round() * texel - cx;
    ty += (cy / texel).round() * texel - cy;
    let view: [f32; 16] = [
        r[0], u[0], -f[0], 0.0,
        r[1], u[1], -f[1], 0.0,
        r[2], u[2], -f[2], 0.0,
        tx, ty, tz, 1.0,
    ];
    // Ortográfica con depth [0,1], Y invertida (NDC de Vulkan) y w = 1
    // (sin división perspectiva). Un punto a distancia `d` del ojo tiene
    // z_vista = -d y clip_z = d / (back + front): el centro (d = back)
    // da back/(back+front), el fondo de la caja da 1.
    let far = back + front;
    let ortho: [f32; 16] = [
        1.0 / half_extent, 0.0, 0.0, 0.0,
        0.0, -1.0 / half_extent, 0.0, 0.0,
        0.0, 0.0, -1.0 / far, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    // La traslación X/Y de la ortográfica es cero: la caja está centrada
    // en el origen del espacio de vista por construcción (x = x_vista /
    // half_extent).
    mat4_mul(ortho, view)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Transforma una AABB local (min, max) por una matriz de mundo `m` y
/// devuelve la AABB envolvente en espacio de mundo: transforma las 8
/// esquinas y toma el min/max componente a componente.
pub fn aabb_transform(min: [f32; 3], max: [f32; 3], m: &[f32; 16]) -> ([f32; 3], [f32; 3]) {
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [min[0], max[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [min[0], max[1], max[2]],
        [max[0], max[1], max[2]],
    ];

    let mut out_min = [f32::INFINITY; 3];
    let mut out_max = [f32::NEG_INFINITY; 3];

    for c in &corners {
        let x = m[0] * c[0] + m[4] * c[1] + m[8] * c[2] + m[12];
        let y = m[1] * c[0] + m[5] * c[1] + m[9] * c[2] + m[13];
        let z = m[2] * c[0] + m[6] * c[1] + m[10] * c[2] + m[14];

        out_min[0] = out_min[0].min(x);
        out_min[1] = out_min[1].min(y);
        out_min[2] = out_min[2].min(z);
        out_max[0] = out_max[0].max(x);
        out_max[1] = out_max[1].max(y);
        out_max[2] = out_max[2].max(z);
    }

    (out_min, out_max)
}

/// Frustum de una vista: 6 planos (a, b, c, d) con la convención de que
/// un punto está DENTRO si a*x + b*y + c*z + d >= 0 en los 6.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    pub planes: [[f32; 4]; 6],
}

impl Frustum {
    /// Extrae los planos de una matriz vista-proyección del motor
    /// (Gribb-Hartmann adaptado a la profundidad [0, 1] de Vulkan):
    /// izquierda = fila0 + fila3 · derecha = fila3 - fila0 ·
    /// abajo = fila1 + fila3 · arriba = fila3 - fila1 ·
    /// cerca = fila2 · lejos = fila3 - fila2. Cada plano se normaliza
    /// por la longitud de su normal.
    pub fn from_view_proj(vp: &[f32; 16]) -> Self {
        // Filas matemáticas de la matriz en orden columna-mayor.
        let row = |i: usize| -> [f32; 4] {
            [vp[i], vp[4 + i], vp[8 + i], vp[12 + i]]
        };
        let row0 = row(0);
        let row1 = row(1);
        let row2 = row(2);
        let row3 = row(3);

        let add = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
            [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
        };
        let sub = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
            [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
        };

        let mut planes = [
            add(row0, row3), // izquierda
            sub(row3, row0), // derecha
            add(row1, row3), // abajo
            sub(row3, row1), // arriba
            row2,            // cerca
            sub(row3, row2), // lejos
        ];

        for p in &mut planes {
            let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            if len > 0.0 {
                p[0] /= len;
                p[1] /= len;
                p[2] /= len;
                p[3] /= len;
            }
        }

        Self { planes }
    }

    /// Test AABB contra los 6 planos con el vértice positivo: si para
    /// algún plano la esquina más favorable está fuera, la caja está
    /// fuera. Conservador: nunca descarta una caja visible.
    pub fn intersects_aabb(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        for p in &self.planes {
            // Esquina que maximiza a*x + b*y + c*z según el signo de a, b, c.
            let px = if p[0] >= 0.0 { max[0] } else { min[0] };
            let py = if p[1] >= 0.0 { max[1] } else { min[1] };
            let pz = if p[2] >= 0.0 { max[2] } else { min[2] };

            if p[0] * px + p[1] * py + p[2] * pz + p[3] < 0.0 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {


    // Proyección de Vulkan con Y invertida y profundidad [0, 1],
    // orden columna-mayor, replicando la convención del motor.
    fn perspective(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
        let f = 1.0 / (fov_y_rad / 2.0).tan();
        let mut m = [0.0f32; 16];
        m[0] = f / aspect;
        m[5] = -f;
        m[10] = far / (near - far);
        m[11] = -1.0;
        m[14] = (far * near) / (near - far);
        m
    }

    fn identity() -> [f32; 16] {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m
    }

    fn make_vp() -> [f32; 16] {
        let proj = perspective(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 100.0);
        let view = identity();
        mat4_mul(proj, view)
    }

    #[test]
    fn aabb_intersects_box_in_front() {
        let frustum = Frustum::from_view_proj(&make_vp());
        let min = [-0.5 - 0.0, -0.5, -5.5];
        let max = [0.5, 0.5, -4.5];
        assert!(frustum.intersects_aabb(min, max));
    }

    #[test]
    fn aabb_rejects_box_behind_camera() {
        let frustum = Frustum::from_view_proj(&make_vp());
        let min = [-0.5, -0.5, 4.5];
        let max = [0.5, 0.5, 5.5];
        assert!(!frustum.intersects_aabb(min, max));
    }

    #[test]
    fn frustum_de_sombra_filtra_fuera_de_la_caja_ortografica() {
        // Sol cenital sobre el origen, caja de ±25 m (como
        // SHADOW_ORTHO_HALF en producción, back/front 60).
        let vp = directional_shadow_vp([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 25.0, 2048, 60.0, 60.0);
        let frustum = Frustum::from_view_proj(&vp);
        // Dentro de la caja: puede proyectar.
        assert!(frustum.intersects_aabb([4.5, -0.5, 4.5], [5.5, 0.5, 5.5]));
        // A 500 m del centro: fuera de la caja, no proyecta.
        assert!(!frustum.intersects_aabb([499.5, -0.5, 499.5], [500.5, 0.5, 500.5]));
    }

    #[test]
    fn aabb_rejects_box_outside_right() {
        let frustum = Frustum::from_view_proj(&make_vp());
        // fov 90 a z=-5: semiancho = 5, por lo que x=10 queda fuera.
        let min = [9.5, -0.5, -5.5];
        let max = [10.5, 0.5, -4.5];
        assert!(!frustum.intersects_aabb(min, max));
    }

    #[test]
    fn aabb_rejects_box_beyond_far_plane() {
        let frustum = Frustum::from_view_proj(&make_vp());
        let min = [-0.5, -0.5, -150.5];
        let max = [0.5, 0.5, -149.5];
        assert!(!frustum.intersects_aabb(min, max));
    }

    #[test]
    fn aabb_huge_box_wrapping_camera_intersects() {
        let frustum = Frustum::from_view_proj(&make_vp());
        let min = [-1000.0, -1000.0, -1000.0];
        let max = [1000.0, 1000.0, 1000.0];
        assert!(frustum.intersects_aabb(min, max));
    }

    #[test]
    fn aabb_crossing_near_plane_intersects() {
        let frustum = Frustum::from_view_proj(&make_vp());
        let min = [-0.5, -0.5, -0.55];
        let max = [0.5, 0.5, 0.45];
        assert!(frustum.intersects_aabb(min, max));
    }

    #[test]
    fn aabb_transform_pure_translation() {
        let mut m = identity();
        m[12] = 1.0;
        m[13] = 2.0;
        m[14] = 3.0;

        let (min, max) = aabb_transform([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0], &m);
        assert_eq!(min, [0.0, 1.0, 2.0]);
        assert_eq!(max, [2.0, 3.0, 4.0]);
    }

    #[test]
    fn aabb_transform_uniform_scale() {
        let mut m = identity();
        m[0] = 2.0;
        m[5] = 2.0;
        m[10] = 2.0;

        let (min, max) = aabb_transform([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0], &m);
        assert_eq!(min, [-2.0, -2.0, -2.0]);
        assert_eq!(max, [2.0, 2.0, 2.0]);
    }
    use super::*;

    const EPS: f32 = 1.0e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    #[test]
    fn identity_round_trip() {
        let q = euler_deg_to_quat([0.0, 0.0, 0.0]);
        assert!(approx_eq(q[0], 0.0) && approx_eq(q[1], 0.0));
        assert!(approx_eq(q[2], 0.0) && approx_eq(q[3], 1.0));

        let e = quat_to_euler_deg([0.0, 0.0, 0.0, 1.0]);
        assert!(approx_eq(e[0], 0.0) && approx_eq(e[1], 0.0) && approx_eq(e[2], 0.0));
    }

    #[test]
    fn single_axis_round_trips() {
        for angles in [
            [30.0, 0.0, 0.0],
            [0.0, 45.0, 0.0],
            [0.0, 0.0, 90.0],
            [-25.0, 0.0, 0.0],
            [0.0, -60.0, 0.0],
        ] {
            let back = quat_to_euler_deg(euler_deg_to_quat(angles));
            for i in 0..3 {
                assert!(
                    approx_eq(back[i], angles[i]),
                    "axis {i}: expected {:?}, got {:?}",
                    angles,
                    back
                );
            }
        }
    }

    #[test]
    fn compound_rotation_round_trips() {
        for angles in [
            [20.0, 35.0, -15.0],
            [-70.0, 12.0, 130.0],
            [5.0, -5.0, 5.0],
        ] {
            let back = quat_to_euler_deg(euler_deg_to_quat(angles));
            for i in 0..3 {
                assert!(
                    approx_eq(back[i], angles[i]),
                    "component {i}: expected {:?}, got {:?}",
                    angles,
                    back
                );
            }
        }
    }

    #[test]
    fn quaternion_is_always_unit_length() {
        for angles in [[10.0, 20.0, 30.0], [89.0, 0.0, 0.0], [180.0, 180.0, 180.0]] {
            let q = euler_deg_to_quat(angles);
            let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            assert!(approx_eq(len, 1.0), "non-unit quaternion for {angles:?}: {len}");
        }
    }

    #[test]
    fn gimbal_lock_is_finite() {
        let e = quat_to_euler_deg(euler_deg_to_quat([0.0, 90.0, 45.0]));
        assert!(e.iter().all(|v| v.is_finite()), "gimbal lock produced NaN: {e:?}");
        assert!(approx_eq(e[2], 0.0), "Z should be pinned to zero at gimbal lock");

        // The orientation must still be preserved even though the decomposition
        // changed: rotating a vector must give the same answer either way.
        let original = euler_deg_to_quat([0.0, 90.0, 45.0]);
        let rebuilt = euler_deg_to_quat(e);
        let probe = [0.3, -0.7, 0.5];
        let a = quat_rotate_vec3(original, probe);
        let b = quat_rotate_vec3(rebuilt, probe);
        for i in 0..3 {
            assert!(approx_eq(a[i], b[i]), "orientation lost at gimbal lock");
        }
    }

    #[test]
    fn degenerate_quaternion_falls_back_to_identity() {
        assert_eq!(quat_normalize([0.0, 0.0, 0.0, 0.0]), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            quat_normalize([f32::NAN, 0.0, 0.0, 1.0]),
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn euler_matches_legacy_matrix_convention() {
        // Reproduces the exact matrix that glass_app's model_matrix_trs_euler
        // used to build, and checks the quaternion path agrees with it.
        let angles = [23.0_f32, -41.0, 67.0];
        let m = quat_to_mat3(euler_deg_to_quat(angles));

        let (rx, ry, rz) = (
            angles[0].to_radians(),
            angles[1].to_radians(),
            angles[2].to_radians(),
        );
        let (sx, cx) = (rx.sin(), rx.cos());
        let (sy, cy) = (ry.sin(), ry.cos());
        let (sz, cz) = (rz.sin(), rz.cos());

        // Legacy layout: legacy[r][c].
        let legacy = [
            [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
            [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
            [-sy, sx * cy, cx * cy],
        ];

        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    approx_eq(m[r][c], legacy[r][c]),
                    "mismatch at [{r}][{c}]: quat={} legacy={}",
                    m[r][c],
                    legacy[r][c]
                );
            }
        }
    }

    #[test]
    fn vector_rotation_matches_matrix() {
        let angles = [12.0_f32, 34.0, 56.0];
        let q = euler_deg_to_quat(angles);
        let m = quat_to_mat3(q);
        let v = [0.4, 0.9, -0.2];

        let by_quat = quat_rotate_vec3(q, v);
        let by_matrix = [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ];

        for i in 0..3 {
            assert!(approx_eq(by_quat[i], by_matrix[i]), "component {i} differs");
        }
    }

    // ---- 4x4 matrix contract ------------------------------------------------

    /// The inline implementation that `glass_app` used before this module
    /// existed. Kept verbatim so the new helper is proven bit-for-bit
    /// compatible instead of "looks about right".
    fn legacy_trs(pos: [f32; 3], scale: [f32; 3], rot_deg: [f32; 3]) -> [f32; 16] {
        let (sx, cx) = rot_deg[0].to_radians().sin_cos();
        let (sy, cy) = rot_deg[1].to_radians().sin_cos();
        let (sz, cz) = rot_deg[2].to_radians().sin_cos();

        let r00 = cy * cz;
        let r01 = sx * sy * cz - cx * sz;
        let r02 = cx * sy * cz + sx * sz;
        let r10 = cy * sz;
        let r11 = sx * sy * sz + cx * cz;
        let r12 = cx * sy * sz - sx * cz;
        let r20 = -sy;
        let r21 = sx * cy;
        let r22 = cx * cy;

        [
            r00 * scale[0], r10 * scale[0], r20 * scale[0], 0.0,
            r01 * scale[1], r11 * scale[1], r21 * scale[1], 0.0,
            r02 * scale[2], r12 * scale[2], r22 * scale[2], 0.0,
            pos[0], pos[1], pos[2], 1.0,
        ]
    }

    #[test]
    fn trs_matches_legacy_inline_matrix() {
        let cases = [
            ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]),
            ([1.5, -2.0, 0.25], [1.0, 1.0, 1.0], [0.0, 90.0, 0.0]),
            ([-3.0, 0.5, 7.0], [2.0, 3.0, 0.5], [15.0, -40.0, 72.0]),
            ([0.1, 0.2, 0.3], [-1.0, 1.0, -1.0], [180.0, 0.0, 45.0]),
        ];

        for (pos, scale, rot) in cases {
            let n = mat4_from_trs_euler(pos, rot, scale);
            let l = legacy_trs(pos, scale, rot);
            for i in 0..16 {
                assert!(
                    approx_eq(n[i], l[i]),
                    "TRS mismatch at index {i} for {pos:?}/{scale:?}/{rot:?}"
                );
            }
        }
    }

    #[test]
    fn identity_is_multiplicative_neutral() {
        let id = mat4_identity();
        let m = mat4_from_trs_euler([4.0, 5.0, 6.0], [30.0, 60.0, 10.0], [2.0, 1.0, 0.5]);

        for i in 0..16 {
            assert!(approx_eq(mat4_mul(id, m)[i], m[i]));
            assert!(approx_eq(mat4_mul(m, id)[i], m[i]));
        }
    }

    #[test]
    fn multiplication_composes_translations_in_parent_space() {
        // Parent moved +10 on X; child moved +1 on X locally. Under
        // world = parent * local the child must land at x = 11, not 1.
        let parent = mat4_from_trs_euler([10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]);
        let child = mat4_from_trs_euler([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]);

        let world = mat4_mul(parent, child);
        let t = mat4_translation(world);

        assert!(approx_eq(t[0], 11.0), "child x = {}, expected 11", t[0]);
        assert!(approx_eq(t[1], 0.0));
        assert!(approx_eq(t[2], 0.0));
    }

    #[test]
    fn parent_rotation_is_inherited_by_child_offset() {
        // Parent yawed 90 degrees about Y; child offset +1 on X must end up
        // pointing along -Z (right-handed, M = Rz*Ry*Rx, yaw positive CCW
        // looking down +Y maps +X to -Z).
        let parent = mat4_from_trs_euler([0.0, 0.0, 0.0], [0.0, 90.0, 0.0], [1.0; 3]);
        let child = mat4_from_trs_euler([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]);

        let t = mat4_translation(mat4_mul(parent, child));
        assert!(approx_eq(t[0], 0.0), "x = {}", t[0]);
        assert!(approx_eq(t[2], -1.0), "z = {}, expected -1", t[2]);
    }

    #[test]
    fn parent_scale_is_inherited_by_child_offset() {
        let parent = mat4_from_trs_euler([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let child = mat4_from_trs_euler([3.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]);

        let t = mat4_translation(mat4_mul(parent, child));
        assert!(approx_eq(t[0], 6.0), "x = {}, expected 6", t[0]);
    }

    #[test]
    fn transform_point_agrees_with_translation_column() {
        let m = mat4_from_trs_euler([2.0, 3.0, 4.0], [0.0, 0.0, 0.0], [1.0; 3]);
        let p = mat4_transform_point(m, [1.0, 1.0, 1.0]);
        assert!(approx_eq(p[0], 3.0) && approx_eq(p[1], 4.0) && approx_eq(p[2], 5.0));
    }

    #[test]
    fn inverse_affine_round_trips() {
        let cases = [
            ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ([3.0, -1.0, 2.0], [20.0, 70.0, -35.0], [1.0, 1.0, 1.0]),
            ([-4.0, 0.5, 9.0], [0.0, 180.0, 0.0], [2.0, 0.5, 3.0]),
        ];

        for (pos, rot, scale) in cases {
            let m = mat4_from_trs_euler(pos, rot, scale);
            let inv = mat4_inverse_affine(m).expect("affine matrix must be invertible");
            let id = mat4_mul(m, inv);

            for (i, expected) in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0].iter().enumerate() {
                assert!(
                    approx_eq(id[i], *expected),
                    "M * M^-1 != I at index {i} ({} vs {expected})",
                    id[i]
                );
            }
        }
    }

    #[test]
    fn inverse_affine_maps_world_back_to_local() {
        // Parent moves +10 X; the child's local offset is +2. Feeding the
        // child's WORLD matrix back through the parent inverse must recover
        // exactly the local offset, which is the physics writeback path.
        let parent = mat4_from_trs_euler([10.0, 0.0, 0.0], [0.0, 90.0, 0.0], [2.0, 2.0, 2.0]);
        let local = mat4_from_trs_euler([2.0, 1.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]);
        let world = mat4_mul(parent, local);

        let recovered = mat4_mul(mat4_inverse_affine(parent).unwrap(), world);
        let t = mat4_translation(recovered);

        assert!(approx_eq(t[0], 2.0), "x = {}", t[0]);
        assert!(approx_eq(t[1], 1.0), "y = {}", t[1]);
        assert!(approx_eq(t[2], 0.0), "z = {}", t[2]);
    }

    #[test]
    fn inverse_affine_rejects_singular_matrix() {
        let collapsed = mat4_from_trs_euler([0.0; 3], [0.0; 3], [0.0, 1.0, 1.0]);
        assert!(
            mat4_inverse_affine(collapsed).is_none(),
            "a zero-scale axis must report singularity instead of returning garbage"
        );
    }

    #[test]
    fn decompose_recovers_rotation_quaternion() {
        let cases = [
            ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ([0.0, 90.0, 0.0], [1.0, 1.0, 1.0]),
            ([35.0, -60.0, 120.0], [2.0, 0.5, 3.0]),
        ];

        for (rot, scale) in cases {
            let m = mat4_from_trs_euler([1.0, 2.0, 3.0], rot, scale);
            let q = mat4_to_quat(m);
            let expected = euler_deg_to_quat(rot);

            // q and -q describe the same rotation.
            let dot = q[0] * expected[0] + q[1] * expected[1] + q[2] * expected[2] + q[3] * expected[3];
            assert!(
                (dot.abs() - 1.0).abs() < 1.0e-4,
                "quaternion mismatch for rot {rot:?}: dot = {dot}"
            );
        }
    }

    #[test]
    fn decompose_recovers_scale() {
        let m = mat4_from_trs_euler([0.0; 3], [0.0, 30.0, 0.0], [2.0, 0.5, 3.0]);
        let s = mat4_scale(m);
        assert!(approx_eq(s[0], 2.0), "sx = {}", s[0]);
        assert!(approx_eq(s[1], 0.5), "sy = {}", s[1]);
        assert!(approx_eq(s[2], 3.0), "sz = {}", s[2]);
    }

    #[test]
    fn trs_quat_agrees_with_euler_path() {
        // Ambas rutas deben producir la MISMA matriz: el cuaternión es la
        // fuente de verdad, el Euler sólo una vista derivada.
        let cases = [
            ([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ([0.0, 0.0, 0.0], [30.0, 45.0, 60.0], [2.0, 0.5, 1.5]),
            ([-1.0, 4.0, 0.5], [0.0, 180.0, 0.0], [1.0; 3]),
        ];

        for (pos, rot, scale) in cases {
            let via_euler = mat4_from_trs_euler(pos, rot, scale);
            let via_quat = mat4_from_trs_quat(pos, euler_deg_to_quat(rot), scale);

            for idx in 0..16 {
                assert!(
                    approx_eq(via_euler[idx], via_quat[idx]),
                    "discrepancia en indice {idx}: {} vs {}",
                    via_euler[idx],
                    via_quat[idx]
                );
            }
        }
    }

    #[test]
    fn trs_quat_round_trips_through_decompose() {
        let q = euler_deg_to_quat([12.0, -34.0, 56.0]);
        let m = mat4_from_trs_quat([2.0, 3.0, 4.0], q, [2.0, 2.0, 2.0]);

        let back = mat4_to_quat(m);
        let dot = back[0] * q[0] + back[1] * q[1] + back[2] * q[2] + back[3] * q[3];
        assert!((dot.abs() - 1.0).abs() < 1.0e-4, "dot = {dot}");

        let t = mat4_translation(m);
        assert!(approx_eq(t[0], 2.0) && approx_eq(t[1], 3.0) && approx_eq(t[2], 4.0));
    }
    #[test]
    fn shadow_vp_centro_queda_en_el_centro_del_mapa() {
        // Sol cenital: L = +Y. El centro de la caja debe proyectar al
        // centro del mapa (uv 0,5, 0,5) y a una profundidad intermedia.
        let vp = directional_shadow_vp([10.0, 0.0, -4.0], [0.0, 1.0, 0.0], 25.0, 2048, 60.0, 60.0);
        let c = mat4_transform_point(vp, [10.0, 0.0, -4.0]);
        // Con snap, el centro cae dentro de MEDIO téxel del centro del
        // mapa (téxel NDC = 2/2048 ≈ 0,00098), no en el centro exacto.
        assert!(c[0].abs() < 6.0e-4 && c[1].abs() < 6.0e-4, "centro NDC = {c:?}");
        assert!(c[2] > 0.0 && c[2] < 1.0, "profundidad del centro = {}", c[2]);
    }

    #[test]
    fn shadow_vp_el_borde_de_la_caja_es_el_borde_del_mapa() {
        // Desplazar el centro 25 m (la semiextensión) contra el eje X de
        // luz debe llevar el NDC a ±1 en ese eje.
        let vp = directional_shadow_vp([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 25.0, 2048, 60.0, 60.0);
        let p = mat4_transform_point(vp, [25.0, 0.0, 0.0]);
        assert!((p[0].abs() - 1.0).abs() < 1.0e-3, "borde NDC.x = {}", p[0]);
        let q = mat4_transform_point(vp, [0.0, 0.0, 25.0]);
        assert!((q[1].abs() - 1.0).abs() < 1.0e-3, "borde NDC.y = {}", q[1]);
    }

    #[test]
    fn shadow_vp_snap_mantiene_el_centro_en_un_texel_exacto() {
        // Con snap, el centro proyectado en UV debe caer en un múltiplo
        // exacto del téxel (el origen del mapa se mueve con la retícula).
        for centro in [[0.13, 0.0, -0.71], [3.7, 1.0, 2.9], [-11.31, 0.0, 8.47]] {
            let vp = directional_shadow_vp(centro, [0.35, 0.8, 0.49], 25.0, 2048, 60.0, 60.0);
            let c = mat4_transform_point(vp, centro);
            let texel_uv = 1.0 / 2048.0;
            let u = c[0] * 0.5 + 0.5;
            let v = c[1] * 0.5 + 0.5;
            let ru = (u / texel_uv).round() * texel_uv;
            let rv = (v / texel_uv).round() * texel_uv;
            assert!((u - ru).abs() < 1.0e-4, "u no alineada a téxel: {u} vs {ru}");
            assert!((v - rv).abs() < 1.0e-4, "v no alineada a téxel: {v} vs {rv}");
        }
    }

    #[test]
    fn shadow_vp_la_geometria_hacia_el_sol_esta_mas_cerca() {
        // Subir un punto hacia el sol (a lo largo de +L) debe ACERCARLO
        // al ojo de la luz: menor profundidad NDC que el punto en el suelo.
        let l = [0.3, 0.85, 0.43];
        let vp = directional_shadow_vp([0.0, 0.0, 0.0], l, 25.0, 2048, 60.0, 60.0);
        let suelo = mat4_transform_point(vp, [2.0, 0.0, 2.0]);
        let alto = mat4_transform_point(vp, [2.0 + l[0] * 5.0, l[1] * 5.0, 2.0 + l[2] * 5.0]);
        assert!(alto[2] < suelo[2], "alto {} !< suelo {}", alto[2], suelo[2]);
    }
}
