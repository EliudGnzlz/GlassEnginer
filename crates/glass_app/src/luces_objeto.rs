//! Asignación de luces por objeto con histéresis (integración de
//! iluminación): cada objeto dibujado resuelve las luces que lo
//! alcanzan por rango; el score decide y la histéresis evita que la
//! lista baile cuando el objeto o las luces se mueven un poco.

use std::collections::BTreeMap;

/// Una luz candidata para un objeto, ya medida contra él.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LuzCandidata {
    pub id: u64,
    /// Score de asignación ya calculado por el llamador:
    /// intensidad × prioridad / (1 + d²/rango²).
    pub score: f32,
    /// Grupo del perfil de la luz.
    pub grupo: u8,
    /// Tope por objeto del perfil de ESTA luz (el del grupo manda el mínimo).
    pub tope_grupo: u8,
}

/// Margen de histéresis: una luz nueva solo desplaza a una asignada
/// del mismo grupo si su score supera al de la peor asignada en este
/// factor.
pub const MARGEN_HISTERESIS: f32 = 1.15;

/// Resuelve la lista de luces asignadas a un objeto.
///
/// - `previa`: ids asignados en la última resolución (puede traer
///   luces que ya no son candidatas: salen).
/// - `candidatas`: luces que alcanzan al objeto este frame.
/// - `tope_total`: ranuras del UBO (8).
///
/// Reglas: por grupo, el tope es el MÍNIMO `tope_grupo` de las
/// candidatas del grupo. Las previas candidatas se quedan (histéresis
/// implícita); los huecos se rellenan por score desc; una candidata
/// solo desplaza a la peor previa DE SU GRUPO si
/// `score > peor × MARGEN_HISTERESIS`. Al final todo se ordena por
/// score desc y se trunca a `tope_total`. Determinista: desempate
/// por id ascendente en todos los ordenados.
pub fn resolver_asignacion(
    previa: &[u64],
    candidatas: &[LuzCandidata],
    tope_total: usize,
) -> Vec<u64> {
    // Agrupa candidatas por grupo, preservando referencia a sus datos.
    let mut por_grupo: BTreeMap<u8, Vec<&LuzCandidata>> = BTreeMap::new();
    for c in candidatas {
        por_grupo.entry(c.grupo).or_default().push(c);
    }

    let mut resultado: Vec<LuzCandidata> = Vec::new();

    for (_grupo, mut lista) in por_grupo {
        // Tope del grupo: mínimo de los tope_grupo de sus candidatas.
        // Si sale 0, el grupo no aporta nada.
        let tope = lista.iter().map(|c| c.tope_grupo).min().unwrap_or(0) as usize;
        if tope == 0 {
            continue;
        }

        // Orden desc por score, desempate por id asc.
        lista.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });

        // Retenidas: las previas que siguen siendo candidatas de este grupo.
        let mut retenidas: Vec<LuzCandidata> = lista
            .iter()
            .filter(|c| previa.contains(&c.id))
            .map(|c| **c)
            .collect();
        // Orden desc por score, desempate por id asc (consistente).
        retenidas.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });

        // Candidatas nuevas (no estaban en `previa`), en orden desc por score.
        let nuevas: Vec<&LuzCandidata> = lista
            .iter()
            .filter(|c| !previa.contains(&c.id))
            .copied()
            .collect();

        for nueva in nuevas {
            if retenidas.len() < tope {
                // Hay hueco libre: entra directo.
                retenidas.push(*nueva);
                retenidas.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.id.cmp(&b.id))
                });
            } else {
                // Grupo lleno: solo desplaza a la peor retenida si la
                // supera por el margen de histéresis.
                if let Some(peor_idx) = peor_indice(&retenidas) {
                    let peor = retenidas[peor_idx];
                    if nueva.score > peor.score * MARGEN_HISTERESIS {
                        retenidas.remove(peor_idx);
                        retenidas.push(*nueva);
                        retenidas.sort_by(|a, b| {
                            b.score
                                .partial_cmp(&a.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then_with(|| a.id.cmp(&b.id))
                        });
                    }
                }
            }
        }

        // Trunca al tope del grupo (por si quedaron más retenidas que tope).
        retenidas.truncate(tope);
        resultado.extend(retenidas);
    }

    // Orden global desc por score, desempate por id asc, y trunca al tope total.
    resultado.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    resultado.truncate(tope_total);

    resultado.into_iter().map(|c| c.id).collect()
}

/// Devuelve el índice de la retenida con menor score (desempate: la
/// de mayor id, para que al desplazar se prefiera quitar la más
/// reciente/arbitraria entre iguales).
fn peor_indice(retenidas: &[LuzCandidata]) -> Option<usize> {
    retenidas
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.id.cmp(&a.id))
        })
        .map(|(idx, _)| idx)
}

/// Una luz de la escena, con todo lo que la asignación y el UBO
/// necesitan (posición/dirección ya en mundo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LuzEscena {
    pub id: u64,
    pub pos: [f32; 3],
    /// Dirección de mundo normalizada (solo Spot; en Point da igual).
    pub dir: [f32; 3],
    /// rgb = color, w = intensidad (como en el UBO).
    pub color_int: [f32; 4],
    pub range: f32,
    pub cos_outer: f32,
    pub cos_inner: f32,
    pub es_spot: bool,
    pub cast_shadow: bool,
    pub prioridad: f32,
    pub grupo: u8,
    pub tope_grupo: u8,
}

/// La asignación vigente de un objeto.
#[derive(Debug, Clone)]
pub struct AsignacionObjeto {
    /// Ranura del buffer de luces (1..=1023; la 0 es el global).
    pub ranura: u32,
    /// Ids de luz asignados, en el orden de los slots del UBO.
    pub luces: Vec<u64>,
    /// Centro del objeto cuando se resolvió.
    pub centro: [f32; 3],
    /// Versión del conjunto de luces con la que se resolvió.
    pub version: u64,
}

/// Estado del sistema de asignación entre frames.
pub struct SistemaAsignacion {
    pub por_entidad: std::collections::HashMap<u64, AsignacionObjeto>,
    libres: Vec<u32>,
    siguiente: u32,
    /// Versión del conjunto de luces: sube cuando la firma cambia.
    pub version: u64,
    firma: u64,
}

impl SistemaAsignacion {
    pub fn nueva() -> Self {
        Self {
            por_entidad: std::collections::HashMap::new(),
            libres: Vec::new(),
            siguiente: 1,
            version: 0,
            firma: 0,
        }
    }

    /// Ranura de un objeto (0 si no tiene asignación).
    pub fn ranura_de(&self, entidad: u64) -> u32 {
        self.por_entidad.get(&entidad).map(|a| a.ranura).unwrap_or(0)
    }

    fn calcular_firma(luces: &[LuzEscena]) -> u64 {
        let mut ordenadas: Vec<&LuzEscena> = luces.iter().collect();
        ordenadas.sort_by_key(|l| l.id);

        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        let mut hash: u64 = FNV_OFFSET;

        fn mezclar(hash: &mut u64, bytes: &[u8]) {
            for b in bytes {
                *hash ^= *b as u64;
                *hash = hash.wrapping_mul(FNV_PRIME);
            }
        }

        for l in ordenadas {
            mezclar(&mut hash, &l.id.to_le_bytes());
            mezclar(&mut hash, &l.pos[0].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.pos[1].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.pos[2].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.range.to_bits().to_le_bytes());
            mezclar(&mut hash, &l.color_int[0].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.color_int[1].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.color_int[2].to_bits().to_le_bytes());
            mezclar(&mut hash, &l.color_int[3].to_bits().to_le_bytes());
            mezclar(&mut hash, &[l.es_spot as u8]);
            mezclar(&mut hash, &[l.cast_shadow as u8]);
            mezclar(&mut hash, &l.prioridad.to_bits().to_le_bytes());
            mezclar(&mut hash, &[l.grupo]);
            mezclar(&mut hash, &[l.tope_grupo]);
            mezclar(&mut hash, &l.cos_outer.to_bits().to_le_bytes());
            mezclar(&mut hash, &l.cos_inner.to_bits().to_le_bytes());
        }

        hash
    }

    /// Observa el conjunto de luces del frame: si la firma (ids,
    /// posiciones, rangos, intensidades, perfiles) cambió desde la
    /// última vez, sube la versión — todas las asignaciones quedan
    /// viejas y se re-resuelven al pedirse. Devuelve si cambió.
    pub fn observar_luces(&mut self, luces: &[LuzEscena]) -> bool {
        let nueva_firma = Self::calcular_firma(luces);
        if nueva_firma != self.firma {
            self.firma = nueva_firma;
            self.version += 1;
            true
        } else {
            false
        }
    }

    /// ¿Hay que re-resolver este objeto? Sí si no tiene asignación,
    /// si su versión es vieja, si se movió más de 0,25 m desde el
    /// centro guardado, o si toca el refresco rotativo
    /// ((frame + entidad) % 30 == 0).
    pub fn necesita(&self, entidad: u64, centro: [f32; 3], frame: u64) -> bool {
        match self.por_entidad.get(&entidad) {
            None => true,
            Some(a) => {
                if a.version != self.version {
                    return true;
                }
                let dx = centro[0] - a.centro[0];
                let dy = centro[1] - a.centro[1];
                let dz = centro[2] - a.centro[2];
                let dist2 = dx * dx + dy * dy + dz * dz;
                if dist2 > 0.25 * 0.25 {
                    return true;
                }
                (frame + entidad) % 30 == 0
            }
        }
    }

    /// Resuelve la asignación del objeto contra las luces dadas:
    /// la distancia de ranking es al **punto más cercano de la caja**
    /// del receptor (centro ± semiejes), no a su centro: candidatas =
    /// las que alcanzan la caja (d <= range); score =
    /// intensidad × prioridad / (1 + d²/range²) con range acotado a
    /// >= 0.01. (Antes se medía al centro menos el radio de la
    /// esfera envolvente: en un receptor grande la d salía 0 para
    /// todas las luces y el ranking era un empate sin sentido.)
    /// Reutiliza la ranura si ya tenía; si no hay ranuras (1023
    /// ocupadas), devuelve None y el objeto cae al global. Guarda y
    /// devuelve la asignación.
    pub fn resolver(
        &mut self,
        entidad: u64,
        centro: [f32; 3],
        semiejes: [f32; 3],
        luces: &[LuzEscena],
    ) -> Option<&AsignacionObjeto> {
        let mut candidatas: Vec<LuzCandidata> = Vec::new();
        for l in luces {
            // Punto más cercano de la caja a la luz (clamp por eje) y
            // distancia a ese punto: 0 si la luz está dentro de la
            // caja o sobre ella.
            let mut d2 = 0.0f32;
            for eje in 0..3 {
                let cerca =
                    l.pos[eje].clamp(centro[eje] - semiejes[eje], centro[eje] + semiejes[eje]);
                let delta = l.pos[eje] - cerca;
                d2 += delta * delta;
            }
            let d = d2.sqrt();
            if d <= l.range {
                let range_seguro = l.range.max(0.01);
                let score =
                    l.color_int[3] * l.prioridad / (1.0 + (d * d) / (range_seguro * range_seguro));
                candidatas.push(LuzCandidata {
                    id: l.id,
                    score,
                    grupo: l.grupo,
                    tope_grupo: l.tope_grupo,
                });
            }
        }

        let previa: Vec<u64> = self
            .por_entidad
            .get(&entidad)
            .map(|a| a.luces.clone())
            .unwrap_or_default();

        let nueva_lista = resolver_asignacion(&previa, &candidatas, glass_renderer::MAX_POINT_LIGHTS);

        let ranura = match self.por_entidad.get(&entidad) {
            Some(a) => a.ranura,
            None => {
                if let Some(r) = self.libres.pop() {
                    r
                } else if self.siguiente <= 1023 {
                    let r = self.siguiente;
                    self.siguiente += 1;
                    r
                } else {
                    return None;
                }
            }
        };

        let asignacion = AsignacionObjeto {
            ranura,
            luces: nueva_lista,
            centro,
            version: self.version,
        };
        self.por_entidad.insert(entidad, asignacion);
        self.por_entidad.get(&entidad)
    }

    /// Libera las ranuras de las entidades que ya no existen.
    pub fn podar(&mut self, vivas: &std::collections::HashSet<u64>) {
        let muertas: Vec<u64> = self
            .por_entidad
            .keys()
            .filter(|e| !vivas.contains(e))
            .copied()
            .collect();
        for entidad in muertas {
            if let Some(a) = self.por_entidad.remove(&entidad) {
                self.libres.push(a.ranura);
            }
        }
    }
}

/// Construye el UBO de un objeto desde su lista de ids: rellena
/// pos_range/color_intensity/dir_cos/cone por luz y el índice de
/// cubo de sombra desde `cubos` (id de luz → cubo o -1 si no tiene;
/// far = range, bias 0.0025, near 0.1, como en la recolección
/// global). `luces` indexa por id.
pub fn construir_ubo(
    lista: &[u64],
    luces: &std::collections::HashMap<u64, &LuzEscena>,
    cubos: &std::collections::HashMap<u64, i32>,
) -> glass_renderer::PointLightsUbo {
    let mut ubo = glass_renderer::PointLightsUbo::default();

    for (i, id) in lista.iter().enumerate() {
        if i >= glass_renderer::MAX_POINT_LIGHTS {
            break;
        }
        let Some(l) = luces.get(id) else { continue };

        ubo.pos_range[i] = [l.pos[0], l.pos[1], l.pos[2], l.range];
        ubo.color_intensity[i] = l.color_int;

        let cubo = cubos.get(id).copied().unwrap_or(-1);
        ubo.shadow[i] = [cubo as f32, l.range.max(0.1), 0.0025, 0.1];

        if l.es_spot {
            ubo.dir_cos[i] = [l.dir[0], l.dir[1], l.dir[2], l.cos_outer];
            ubo.cone[i] = [l.cos_inner, 1.0, 0.0, 0.0];
        } else {
            ubo.dir_cos[i] = [0.0; 4];
            ubo.cone[i] = [0.0; 4];
        }
    }

    ubo.count = lista.len() as u32;
    ubo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u64, score: f32, grupo: u8, tope_grupo: u8) -> LuzCandidata {
        LuzCandidata {
            id,
            score,
            grupo,
            tope_grupo,
        }
    }

    #[test]
    fn sin_previa_rellena_por_score() {
        let candidatas = vec![
            c(1, 5.0, 0, 4),
            c(2, 9.0, 0, 4),
            c(3, 1.0, 0, 4),
            c(4, 7.0, 0, 4),
            c(5, 3.0, 0, 4),
        ];
        let resultado = resolver_asignacion(&[], &candidatas, 8);
        assert_eq!(resultado, vec![2, 4, 1, 5]);
    }

    #[test]
    fn previa_fuera_de_rango_sale() {
        let previa = vec![1, 2];
        let candidatas = vec![c(2, 5.0, 0, 4), c(3, 3.0, 0, 4)];
        let resultado = resolver_asignacion(&previa, &candidatas, 8);
        assert!(!resultado.contains(&1));
        assert_eq!(resultado, vec![2, 3]);
    }

    #[test]
    fn histeresis_retiene() {
        let previa = vec![1];

        // Nueva con score 1.10: no supera 1.0 * 1.15 = 1.15, se queda 1.
        let candidatas_retiene = vec![c(1, 1.0, 0, 1), c(2, 1.10, 0, 1)];
        let resultado = resolver_asignacion(&previa, &candidatas_retiene, 8);
        assert_eq!(resultado, vec![1]);

        // Nueva con score 1.20: supera 1.0 * 1.15 = 1.15, entra 2 y sale 1.
        let candidatas_desplaza = vec![c(1, 1.0, 0, 1), c(2, 1.20, 0, 1)];
        let resultado = resolver_asignacion(&previa, &candidatas_desplaza, 8);
        assert_eq!(resultado, vec![2]);
    }

    #[test]
    fn tope_por_grupo_es_el_minimo() {
        let candidatas = vec![
            c(1, 9.0, 0, 4),
            c(2, 8.0, 0, 2), // este fija el tope del grupo en 2
            c(3, 7.0, 0, 4),
            c(4, 6.0, 0, 4),
        ];
        let resultado = resolver_asignacion(&[], &candidatas, 8);
        assert_eq!(resultado, vec![1, 2]);
    }

    #[test]
    fn grupos_no_se_pisan() {
        let previa = vec![10];
        let candidatas = vec![
            c(10, 5.0, 0, 1), // grupo 0, tope 1, retenida
            c(20, 9.0, 1, 2),
            c(21, 8.0, 1, 2),
        ];
        let resultado = resolver_asignacion(&previa, &candidatas, 8);
        let mut esperado = vec![10, 20, 21];
        esperado.sort_by(|a, b| {
            let sa = candidatas.iter().find(|c| c.id == *a).unwrap().score;
            let sb = candidatas.iter().find(|c| c.id == *b).unwrap().score;
            sb.partial_cmp(&sa).unwrap().then(a.cmp(b))
        });
        assert_eq!(resultado, esperado);
        assert_eq!(resultado.len(), 3);
    }

    #[test]
    fn trunca_al_tope_total() {
        let candidatas = vec![
            c(1, 10.0, 0, 8),
            c(2, 9.0, 0, 8),
            c(3, 8.0, 0, 8),
            c(4, 7.0, 0, 8),
            c(5, 6.0, 0, 8),
            c(6, 5.0, 1, 8),
            c(7, 4.0, 1, 8),
            c(8, 3.0, 1, 8),
            c(9, 2.0, 1, 8),
            c(10, 1.0, 1, 8),
        ];
        let resultado = resolver_asignacion(&[], &candidatas, 8);
        assert_eq!(resultado.len(), 8);
        assert_eq!(resultado, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn determinista_con_empates() {
        let candidatas = vec![
            c(5, 1.0, 0, 8),
            c(3, 1.0, 0, 8),
            c(1, 1.0, 0, 8),
            c(4, 1.0, 0, 8),
            c(2, 1.0, 0, 8),
        ];
        let resultado = resolver_asignacion(&[], &candidatas, 8);
        assert_eq!(resultado, vec![1, 2, 3, 4, 5]);
    }

#[test]
    fn observar_luces_detecta_cambios() {
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![LuzEscena {
            id: 1,
            pos: [0.0, 0.0, 0.0],
            dir: [0.0, -1.0, 0.0],
            color_int: [1.0, 1.0, 1.0, 1.0],
            range: 5.0,
            cos_outer: 0.5,
            cos_inner: 0.8,
            es_spot: false,
            cast_shadow: false,
            prioridad: 1.0,
            grupo: 0,
            tope_grupo: 8,
        }];

        assert!(sistema.observar_luces(&luces));
        let version_tras_primera = sistema.version;

        assert!(!sistema.observar_luces(&luces));
        assert_eq!(sistema.version, version_tras_primera);

        let mut luces_movidas = luces.clone();
        luces_movidas[0].pos = [1.0, 0.0, 0.0];
        assert!(sistema.observar_luces(&luces_movidas));
        assert_eq!(sistema.version, version_tras_primera + 1);
    }

    fn luz_en(id: u64, pos: [f32; 3], range: f32) -> LuzEscena {
        LuzEscena {
            id,
            pos,
            dir: [0.0, -1.0, 0.0],
            color_int: [1.0, 1.0, 1.0, 7.0],
            range,
            cos_outer: 0.5,
            cos_inner: 0.8,
            es_spot: false,
            cast_shadow: false,
            prioridad: 1.0,
            grupo: 0,
            tope_grupo: 8,
        }
    }

    #[test]
    fn distancia_a_cotas_no_al_centro() {
        // Caja alargada (semiejes 10×1×1): la luz A está más lejos
        // del CENTRO que la B, pero más cerca de la CAJA. Con la
        // métrica vieja (centro) ganaba B; con cotas debe ganar A.
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![luz_en(1, [9.0, 3.0, 0.0], 30.0), luz_en(2, [0.0, 8.0, 0.0], 30.0)];
        let a = sistema.resolver(10, [0.0, 0.0, 0.0], [10.0, 1.0, 1.0], &luces).unwrap();
        assert_eq!(a.luces[0], 1);
    }

    #[test]
    fn luz_fuera_de_rango_de_la_caja_no_es_candidata() {
        // El suelo real (30×18 a y=-0,5): una vela encima entra; una
        // luz a 25 m del borde, no, aunque el shader la atenuaría.
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![luz_en(1, [0.0, 1.0, 0.0], 7.5), luz_en(2, [40.0, 1.0, 0.0], 7.5)];
        let a = sistema
            .resolver(3, [0.0, -0.5, 0.0], [15.0, 0.5, 9.0], &luces)
            .unwrap();
        assert_eq!(a.luces, vec![1]);
    }

    #[test]
    fn sobre_la_caja_se_rankean_por_distancia_real() {
        // Regresión del empate del suelo: dos velas SOBRE la misma
        // caja grande, a distinta altura. La métrica vieja daba
        // d = 0 a ambas (empate; ganaba el orden de llegada); la de
        // cotas rankea por la distancia real a la caja.
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![luz_en(1, [-11.0, 6.0, 0.0], 7.5), luz_en(2, [11.0, 1.0, 0.0], 7.5)];
        let a = sistema
            .resolver(3, [0.0, -0.5, 0.0], [15.0, 0.5, 9.0], &luces)
            .unwrap();
        assert_eq!(a.luces[0], 2);
    }

    #[test]
    fn resolver_asigna_ranura_y_respeta_rango() {
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![
            LuzEscena {
                id: 1,
                pos: [2.0, 0.0, 0.0],
                dir: [0.0, -1.0, 0.0],
                color_int: [1.0, 1.0, 1.0, 1.0],
                range: 5.0,
                cos_outer: 0.5,
                cos_inner: 0.8,
                es_spot: false,
                cast_shadow: false,
                prioridad: 1.0,
                grupo: 0,
                tope_grupo: 8,
            },
            LuzEscena {
                id: 2,
                pos: [50.0, 0.0, 0.0],
                dir: [0.0, -1.0, 0.0],
                color_int: [1.0, 1.0, 1.0, 1.0],
                range: 5.0,
                cos_outer: 0.5,
                cos_inner: 0.8,
                es_spot: false,
                cast_shadow: false,
                prioridad: 1.0,
                grupo: 0,
                tope_grupo: 8,
            },
        ];

        sistema.observar_luces(&luces);
        let asignacion = sistema.resolver(10, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], &luces).unwrap();
        assert_eq!(asignacion.luces, vec![1]);
        assert!(asignacion.ranura >= 1);
        let ranura_previa = asignacion.ranura;

        let asignacion2 = sistema.resolver(10, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], &luces).unwrap();
        assert_eq!(asignacion2.ranura, ranura_previa);
    }

    #[test]
    fn podar_libera_ranuras() {
        let mut sistema = SistemaAsignacion::nueva();
        let luces = vec![LuzEscena {
            id: 1,
            pos: [0.0, 0.0, 0.0],
            dir: [0.0, -1.0, 0.0],
            color_int: [1.0, 1.0, 1.0, 1.0],
            range: 5.0,
            cos_outer: 0.5,
            cos_inner: 0.8,
            es_spot: false,
            cast_shadow: false,
            prioridad: 1.0,
            grupo: 0,
            tope_grupo: 8,
        }];

        sistema.observar_luces(&luces);
        sistema.resolver(1, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], &luces);
        assert_ne!(sistema.ranura_de(1), 0);

        let vivas = std::collections::HashSet::new();
        sistema.podar(&vivas);
        assert_eq!(sistema.ranura_de(1), 0);

        sistema.resolver(2, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], &luces);
        assert!(sistema.ranura_de(2) >= 1);
    }

    #[test]
    fn construir_ubo_rellena_spot_y_cubo() {
        let luz = LuzEscena {
            id: 7,
            pos: [1.0, 2.0, 3.0],
            dir: [0.0, -1.0, 0.0],
            color_int: [1.0, 0.5, 0.25, 2.0],
            range: 10.0,
            cos_outer: 0.6,
            cos_inner: 0.9,
            es_spot: true,
            cast_shadow: true,
            prioridad: 1.0,
            grupo: 0,
            tope_grupo: 8,
        };

        let mut luces = std::collections::HashMap::new();
        luces.insert(7u64, &luz);

        let mut cubos = std::collections::HashMap::new();
        cubos.insert(7u64, 2);

        let ubo = construir_ubo(&[7], &luces, &cubos);

        assert_eq!(ubo.count, 1);
        assert_eq!(ubo.cone[0][1], 1.0);
        assert_eq!(ubo.dir_cos[0], [0.0, -1.0, 0.0, 0.6]);
        assert_eq!(ubo.shadow[0], [2.0, 10.0, 0.0025, 0.1]);
    }
}
