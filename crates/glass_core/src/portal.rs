//! Matemática de portales tipo Portal: la transformación de
//! pareja (dst · giro180Y · src⁻¹), la cámara virtual, la
//! distancia al plano y la detección del cruce del
//! rectángulo. Todo puro y testeable; el render y el
//! teletransporte viven en la app.
use crate::math::{
    mat4_from_trs_euler, mat4_inverse_affine, mat4_mul, mat4_transform_point,
    mat4_translation,
};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalData {
    pub pair: String,
    pub width: f32,
    pub height: f32,
}

impl Default for PortalData {
    fn default() -> Self {
        Self {
            pair: String::new(),
            width: 1.2,
            height: 2.0,
        }
    }
}

pub fn flip_y() -> [f32; 16] {
    // Giro de 180 grados sobre Y según la convención del portal.
    mat4_from_trs_euler([0.0; 3], [0.0, 180.0, 0.0], [1.0; 3])
}

pub fn pair_transform(src_world: [f32; 16], dst_world: [f32; 16]) -> Option<[f32; 16]> {
    let inv_src = mat4_inverse_affine(src_world)?;
    // dst · flip_y() · src⁻¹; mat4_mul(a, b) aplica b primero.
    Some(mat4_mul(dst_world, mat4_mul(flip_y(), inv_src)))
}

pub fn virtual_camera_world(
    cam_world: [f32; 16],
    src_world: [f32; 16],
    dst_world: [f32; 16],
) -> Option<[f32; 16]> {
    let pair_t = pair_transform(src_world, dst_world)?;
    Some(mat4_mul(pair_t, cam_world))
}

pub fn view_from_world(world: [f32; 16]) -> Option<[f32; 16]> {
    mat4_inverse_affine(world)
}

pub fn portal_normal(world: [f32; 16]) -> [f32; 3] {
    let n = [world[8], world[9], world[10]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 1e-6 {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 0.0, 1.0]
    }
}

pub fn signed_distance(world: [f32; 16], point: [f32; 3]) -> f32 {
    let t = mat4_translation(world);
    let n = portal_normal(world);
    (point[0] - t[0]) * n[0] + (point[1] - t[1]) * n[1] + (point[2] - t[2]) * n[2]
}

pub fn crossing_point(
    prev: [f32; 3],
    curr: [f32; 3],
    world: [f32; 16],
    width: f32,
    height: f32,
) -> Option<[f32; 3]> {
    let d0 = signed_distance(world, prev);
    let d1 = signed_distance(world, curr);

    // Sin cambio de signo no hay cruce. Si uno es exactamente 0 y el otro no,
    // sí se considera cruce.
    if d0 == d1 || d0 * d1 > 0.0 {
        return None;
    }

    let t = d0 / (d0 - d1);
    let cut = [
        prev[0] + (curr[0] - prev[0]) * t,
        prev[1] + (curr[1] - prev[1]) * t,
        prev[2] + (curr[2] - prev[2]) * t,
    ];

    let inv_world = mat4_inverse_affine(world)?;
    let local = mat4_transform_point(inv_world, cut);

    if local[0].abs() <= width / 2.0 && local[1].abs() <= height / 2.0 {
        Some(cut)
    } else {
        None
    }
}

pub fn transport_point(pair_t: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    mat4_transform_point(pair_t, p)
}

pub fn transport_direction(pair_t: [f32; 16], d: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = pair_t[i] * d[0] + pair_t[4 + i] * d[1] + pair_t[8 + i] * d[2];
    }

    let len = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
    if len > 1e-6 {
        [out[0] / len, out[1] / len, out[2] / len]
    } else {
        d
    }
}

pub fn find_pair<'a>(
    entities: &'a [crate::scene::EntityData],
    self_id: u64,
    pair: &str,
) -> Option<&'a crate::scene::EntityData> {
    entities.iter().find(|e| {
        e.id != self_id
            && e.active
            && e.portal
                .as_ref()
                .map_or(false, |p| p.pair.as_str() == pair)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::mat4_from_trs_euler;

    fn mundo(pos: [f32; 3], rot: [f32; 3]) -> [f32; 16] {
        mat4_from_trs_euler(pos, rot, [1.0; 3])
    }

    fn cerca(a: [f32; 3], b: [f32; 3]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 && (a[2] - b[2]).abs() < 1e-4
    }

    #[test]
    fn identidad_la_camara_sale_por_detras_mirando_al_reves() {
        // src = dst = identidad: ver el portal de frente desde
        // (0, 1.6, 5) equivale a estar en (0, 1.6, -5) mirando
        // hacia +Z (el forward local -Z sale apuntando a +Z).
        let id = mundo([0.0; 3], [0.0; 3]);
        let cam = mundo([0.0, 1.6, 5.0], [0.0; 3]);
        let virt = virtual_camera_world(cam, id, id).unwrap();
        assert!(cerca(mat4_translation(virt), [0.0, 1.6, -5.0]));
        // El punto 1 m delante de la cámara virtual:
        let delante = mat4_transform_point(virt, [0.0, 0.0, -1.0]);
        assert!(cerca(delante, [0.0, 1.6, -4.0]), "delante: {delante:?}");
    }

    #[test]
    fn cara_a_cara_lo_de_delante_cae_detras_del_destino() {
        // Los dos portales se miran: src en el origen rotado
        // 180° (su +Z apunta a +Z mundo), dst en (0,0,-10) sin
        // rotar (su +Z apunta a -Z mundo: se encaran).
        let src = mundo([0.0; 3], [0.0, 180.0, 0.0]);
        let dst = mundo([0.0, 0.0, -10.0], [0.0; 3]);
        let t = pair_transform(src, dst).unwrap();
        // 1 m delante de src: su frente local +Z con rotación
        // 180° apunta a −Z del mundo → el punto es (0,0,−1).
        let llevado = transport_point(t, [0.0, 0.0, -1.0]);
        assert!(
            signed_distance(dst, llevado) < 0.0,
            "debería caer DETRÁS del destino: {llevado:?}"
        );
        // 1 m detrás de src cae delante del destino:
        let llevado2 = transport_point(t, [0.0, 0.0, 1.0]);
        assert!(signed_distance(dst, llevado2) > 0.0);
        // Y el centro del origen va al centro del destino:
        assert!(cerca(transport_point(t, [0.0; 3]), [0.0, 0.0, -10.0]));
    }

    #[test]
    fn cruce_del_rectangulo() {
        let id = mundo([0.0; 3], [0.0; 3]);
        // Cruza por el centro:
        let corte = crossing_point([0.0, 1.0, 0.5], [0.0, 1.0, -0.5], id, 1.2, 2.0);
        assert!(cerca(corte.unwrap(), [0.0, 1.0, 0.0]));
        // Cruza el plano pero fuera del rectángulo (x = 3):
        assert!(crossing_point([3.0, 1.0, 0.5], [3.0, 1.0, -0.5], id, 1.2, 2.0).is_none());
        // Sin cambio de signo no hay cruce:
        assert!(crossing_point([0.0, 1.0, 0.5], [0.0, 1.0, 0.2], id, 1.2, 2.0).is_none());
        // Por encima del rectángulo (y = 1.5 > alto/2 = 1.0):
        assert!(crossing_point([0.0, 1.5, 0.5], [0.0, 1.5, -0.5], id, 1.2, 2.0).is_none());
    }

    #[test]
    fn direccion_transporte_conserva_longitud_y_gira() {
        let src = mundo([0.0; 3], [0.0; 3]);
        let dst = mundo([5.0, 0.0, 0.0], [0.0, 90.0, 0.0]);
        let t = pair_transform(src, dst).unwrap();
        let d = transport_direction(t, [0.0, 0.0, -3.0]);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "longitud: {len}");
    }
}
