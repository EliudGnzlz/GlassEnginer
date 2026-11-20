//! Extracción de aristas para dibujar los colliders de malla con su forma real.

use std::collections::BTreeMap;

use rapier3d::parry::math::Point;
use rapier3d::parry::shape::ConvexPolyhedron;

/// Aristas del casco convexo calculado por Parry, en el espacio local de la
/// malla. Es el mismo casco que crea la física para el collider Convex
/// (`ColliderBuilder::convex_hull` pasa por este mismo
/// `ConvexPolyhedron::from_convex_hull`), y las aristas son las del
/// poliedro: sin diagonales de triangulación.
pub fn aristas_casco_convexo(posiciones: &[[f32; 3]]) -> Vec<[[f32; 3]; 2]> {
    let puntos: Vec<Point<f32>> = posiciones
        .iter()
        .map(|p| Point::new(p[0], p[1], p[2]))
        .collect();
    let Some(casco) = ConvexPolyhedron::from_convex_hull(&puntos) else {
        return Vec::new();
    };
    let puntos_casco = casco.points();
    let caras = casco.faces();
    casco
        .edges()
        .iter()
        // El poliedro triangula las caras poligonales: la diagonal de una
        // cara plana comparte la normal de sus dos caras adyacentes y no
        // es una arista geométrica; solo quedan los cambios de plano.
        .filter(|e| {
            let n1 = caras[e.faces.x as usize].normal;
            let n2 = caras[e.faces.y as usize].normal;
            n1.dot(&n2) < 1.0 - 1.0e-5
        })
        .map(|e| {
            let a = puntos_casco[e.vertices.x as usize];
            let b = puntos_casco[e.vertices.y as usize];
            [[a.x, a.y, a.z], [b.x, b.y, b.z]]
        })
        .collect()
}

/// Aristas de borde y aristas cuyo ángulo diedro supera `umbral_grados`.
/// Las caras degeneradas no participan en la adyacencia.
pub fn aristas_caracteristicas(
    posiciones: &[[f32; 3]],
    indices: &[u32],
    umbral_grados: f32,
) -> Vec<[[f32; 3]; 2]> {
    let triangulos: Vec<[u32; 3]> = indices.as_chunks::<3>().0.to_vec();
    aristas_segun_normales(
        posiciones,
        &triangulos,
        umbral_grados.to_radians().cos(),
    )
}

/// Reúne las normales no normalizadas de las caras adyacentes a cada arista.
/// La normalización se aplica únicamente al comparar sus cosenos.
fn aristas_segun_normales(
    posiciones: &[[f32; 3]],
    indices: &[[u32; 3]],
    coseno_limite: f32,
) -> Vec<[[f32; 3]; 2]> {
    let mut caras_por_arista: BTreeMap<(u32, u32), Vec<[f32; 3]>> = BTreeMap::new();

    for triangulo in indices {
        let [ia, ib, ic] = *triangulo;
        let (Some(a), Some(b), Some(c)) = (
            posiciones.get(ia as usize),
            posiciones.get(ib as usize),
            posiciones.get(ic as usize),
        ) else {
            continue;
        };
        if ia == ib || ib == ic || ic == ia {
            continue;
        }

        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let longitud2 = normal[0] * normal[0]
            + normal[1] * normal[1]
            + normal[2] * normal[2];
        if !longitud2.is_finite() || longitud2 <= 1.0e-20 {
            continue;
        }

        for (inicio, fin) in [(ia, ib), (ib, ic), (ic, ia)] {
            let arista = if inicio < fin {
                (inicio, fin)
            } else {
                (fin, inicio)
            };
            caras_por_arista.entry(arista).or_default().push(normal);
        }
    }

    let mut segmentos = Vec::new();
    for ((inicio, fin), normales) in caras_por_arista {
        let caracteristica = if normales.len() == 1 {
            true
        } else {
            normales.iter().enumerate().any(|(i, a)| {
                normales[i + 1..].iter().any(|b| {
                    let la = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
                    let lb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
                    let coseno = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (la * lb);
                    coseno < coseno_limite
                })
            })
        };
        if caracteristica {
            segmentos.push([posiciones[inicio as usize], posiciones[fin as usize]]);
        }
    }
    segmentos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cubo() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let posiciones = vec![
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ];
        let indices = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
        ];
        (posiciones, indices)
    }

    #[test]
    fn cubo_tiene_doce_aristas_de_casco() {
        let (posiciones, _) = cubo();
        assert_eq!(aristas_casco_convexo(&posiciones).len(), 12);
    }

    #[test]
    fn cubo_tiene_doce_aristas_caracteristicas() {
        let (posiciones, indices) = cubo();
        let indices_planos: Vec<u32> = indices.iter().flatten().copied().collect();
        assert_eq!(
            aristas_caracteristicas(&posiciones, &indices_planos, 30.0).len(),
            12
        );
    }

    #[test]
    fn quad_solo_conserva_su_borde() {
        let posiciones = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        assert_eq!(
            aristas_caracteristicas(&posiciones, &indices, 30.0).len(),
            4
        );
    }

    #[test]
    fn tetraedro_tiene_seis_aristas_de_casco() {
        let posiciones = [
            [1.0, 1.0, 1.0],
            [1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
        ];
        assert_eq!(aristas_casco_convexo(&posiciones).len(), 6);
    }
}
