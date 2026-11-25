//! Convenio de mano entre Unity y el motor.
//!
//! Unity es zurdo con Y arriba; el motor y glTF son diestros con Y arriba.
//! La conversión es la conjugación por el espejo Z: se niega la Z de las
//! posiciones y las componentes X e Y de los cuaterniones.

/// Convierte una posición de Unity al convenio del motor.
///
/// `[x, y, z]` pasa a `[x, y, -z]`.
#[must_use]
pub fn convertir_posicion(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[1], -p[2]]
}

/// Convierte un cuaternión de Unity al convenio del motor.
///
/// `[x, y, z, w]` pasa a `[-x, -y, z, w]`.
#[must_use]
pub fn convertir_cuaternion(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], q[2], q[3]]
}

/// Convierte una escala de Unity al convenio del motor.
///
/// La escala no cambia de mano.
#[must_use]
pub fn convertir_escala(e: [f32; 3]) -> [f32; 3] {
    e
}

#[cfg(test)]
mod tests {
    use super::{convertir_cuaternion, convertir_escala, convertir_posicion};

    const TOLERANCIA: f32 = 1e-6;

    fn producto_vectorial(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn rotar_por_cuaternion(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = q;
        let qv = [x, y, z];
        let t = producto_vectorial(qv, v);
        let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
        let wt = [w * t[0], w * t[1], w * t[2]];
        let qvt = producto_vectorial(qv, t);
        [
            v[0] + wt[0] + qvt[0],
            v[1] + wt[1] + qvt[1],
            v[2] + wt[2] + qvt[2],
        ]
    }

    fn assert_cerca(actual: [f32; 3], esperado: [f32; 3]) {
        for (a, e) in actual.iter().zip(esperado.iter()) {
            assert!(
                (*a - *e).abs() < TOLERANCIA,
                "actual {actual:?}, esperado {esperado:?}"
            );
        }
    }

    #[test]
    fn caso_dorado_y() {
        let s = 0.5_f32.sqrt();
        let q_unity = [0.0, s, 0.0, s];
        let avance_unity = [0.0, 0.0, 1.0];

        let q_motor = convertir_cuaternion(q_unity);
        let avance_motor = convertir_posicion(avance_unity);
        let resultado = rotar_por_cuaternion(q_motor, avance_motor);

        assert_cerca(resultado, convertir_posicion([1.0, 0.0, 0.0]));
    }

    #[test]
    fn caso_dorado_x() {
        let s = 0.5_f32.sqrt();
        let q_unity = [s, 0.0, 0.0, s];
        let antes_unity = [0.0, 1.0, 0.0];
        let despues_unity = [0.0, 0.0, 1.0];

        let resultado = rotar_por_cuaternion(
            convertir_cuaternion(q_unity),
            convertir_posicion(antes_unity),
        );

        assert_cerca(resultado, convertir_posicion(despues_unity));
    }

    #[test]
    fn caso_dorado_z() {
        let s = 0.5_f32.sqrt();
        let q_unity = [0.0, 0.0, s, s];
        let antes_unity = [1.0, 0.0, 0.0];
        let despues_unity = [0.0, 1.0, 0.0];

        let resultado = rotar_por_cuaternion(
            convertir_cuaternion(q_unity),
            convertir_posicion(antes_unity),
        );

        assert_cerca(resultado, convertir_posicion(despues_unity));
    }

    #[test]
    fn posicion_y_escala() {
        assert_eq!(convertir_posicion([1.0, 2.0, 3.0]), [1.0, 2.0, -3.0]);
        assert_eq!(convertir_escala([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }
}
