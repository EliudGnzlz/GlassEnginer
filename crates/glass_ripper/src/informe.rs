use crate::{EstadoInforme, LecturaPaquete, LineaInforme};

#[derive(Debug, Clone, Default)]
pub struct InformeImportacion {
    pub lineas: Vec<LineaInforme>,
    /// Acumulado por prefab (guid, nombre), en orden de primera
    /// aparición; aparte de `lineas` para no afectar a `faltantes`.
    pub prefabs: Vec<PrefabInformado>,
}

/// Totales de un prefab (guid, nombre) acumulados entre escenas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefabInformado {
    pub guid: String,
    pub nombre: String,
    pub instancias: usize,
    pub entidades: usize,
    pub aplicadas: usize,
    /// Guids de las escenas distintas en las que aparece.
    pub escenas: Vec<String>,
}

impl InformeImportacion {
    pub fn nuevo() -> Self {
        Self::default()
    }

    pub fn anadir(
        &mut self,
        guid: &str,
        ruta: Option<&str>,
        estado: EstadoInforme,
        detalle: &str,
    ) {
        self.lineas.push(LineaInforme {
            guid: guid.to_string(),
            ruta: ruta.map(|ruta| ruta.to_string()),
            estado,
            detalle: detalle.to_string(),
        });
    }

    /// Acumula el resumen de un prefab en la escena `guid_escena`:
    /// suma instancias/entidades/aplicadas en la entrada (guid_prefab,
    /// nombre) y cuenta la escena una sola vez por prefab.
    pub fn acumular_prefab(
        &mut self,
        guid_escena: &str,
        guid_prefab: &str,
        nombre: &str,
        instancias: usize,
        entidades: usize,
        aplicadas: usize,
    ) {
        let indice = match self
            .prefabs
            .iter()
            .position(|p| p.guid == guid_prefab && p.nombre == nombre)
        {
            Some(i) => i,
            None => {
                self.prefabs.push(PrefabInformado {
                    guid: guid_prefab.to_string(),
                    nombre: nombre.to_string(),
                    ..PrefabInformado::default()
                });
                self.prefabs.len() - 1
            }
        };
        let p = &mut self.prefabs[indice];
        p.instancias += instancias;
        p.entidades += entidades;
        p.aplicadas += aplicadas;
        if !p.escenas.iter().any(|e| e == guid_escena) {
            p.escenas.push(guid_escena.to_string());
        }
    }

    /// Texto del informe: cabecera y una línea por entrada con el
    /// formato "[ESTADO] ruta — detalle (guid: …)". El estado se
    /// escribe: Convertido→CONVERTIDO, Copiado→COPIADO,
    /// ConvertidoConAvisos→CONVERTIDO CON AVISOS,
    /// NoConvertido→NO CONVERTIDO, Rechazado→RECHAZADO,
    /// OmitidoPorColision→OMITIDO POR COLISIÓN. Sin ruta se
    /// escribe "(sin ruta)".
    /// Si hay prefabs acumulados, al final va la sección "Prefabs:"
    /// con una línea por prefab en orden de primera aparición.
    pub fn a_texto(&self) -> String {
        let mut texto = String::from("Informe de importación\n");

        for linea in &self.lineas {
            let ruta = linea.ruta.as_deref().unwrap_or("(sin ruta)");
            texto.push_str(&format!(
                "[{}] {} — {} (guid: {})\n",
                texto_estado(&linea.estado),
                ruta,
                linea.detalle,
                linea.guid
            ));
        }

        if !self.prefabs.is_empty() {
            texto.push_str("Prefabs:\n");
            for p in &self.prefabs {
                texto.push_str(&format!(
                    "Prefab {} (guid {}): {} instancia(s) en {} escena(s), {} entidades, {} modificaciones aplicadas\n",
                    p.nombre,
                    p.guid,
                    p.instancias,
                    p.escenas.len(),
                    p.entidades,
                    p.aplicadas
                ));
            }
        }

        texto
    }

    /// Invariante del bloque: devuelve la lista de lo que NO tiene
    /// línea en el informe — el guid de cada entrada de la lectura
    /// sin línea con ese guid, y cada ruta suelta sin línea con esa
    /// ruta. Vacío = invariante cumplido.
    pub fn faltantes(&self, lectura: &LecturaPaquete) -> Vec<String> {
        let mut faltantes = Vec::new();

        for entrada in &lectura.entradas {
            if !self
                .lineas
                .iter()
                .any(|linea| linea.guid == entrada.guid)
            {
                faltantes.push(entrada.guid.clone());
            }
        }

        for ruta in &lectura.sueltas {
            if !self
                .lineas
                .iter()
                .any(|linea| linea.ruta.as_deref() == Some(ruta.as_str()))
            {
                faltantes.push(ruta.clone());
            }
        }

        faltantes
    }
}

fn texto_estado(estado: &EstadoInforme) -> &'static str {
    match estado {
        EstadoInforme::Convertido => "CONVERTIDO",
        EstadoInforme::Copiado => "COPIADO",
        EstadoInforme::ConvertidoConAvisos => "CONVERTIDO CON AVISOS",
        EstadoInforme::NoConvertido => "NO CONVERTIDO",
        EstadoInforme::Rechazado => "RECHAZADO",
        EstadoInforme::OmitidoPorColision => "OMITIDO POR COLISIÓN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntradaPaquete;

    fn entrada(guid: &str, ruta: Option<&str>) -> EntradaPaquete {
        EntradaPaquete {
            guid: guid.to_string(),
            ruta: ruta.map(|ruta| ruta.to_string()),
            es_carpeta: false,
            asset: None,
            meta_texto: None,
            preview: None,
        }
    }

    #[test]
    fn a_texto_contiene_estados_en_texto() {
        let mut informe = InformeImportacion::nuevo();
        informe.anadir(
            "g1",
            Some("Assets/Uno.prefab"),
            EstadoInforme::Convertido,
            "ok",
        );
        informe.anadir(
            "g2",
            Some("Assets/Dos.mat"),
            EstadoInforme::ConvertidoConAvisos,
            "aviso",
        );
        informe.anadir(
            "g3",
            None,
            EstadoInforme::OmitidoPorColision,
            "duplicado",
        );

        let texto = informe.a_texto();

        assert!(texto.contains("[CONVERTIDO]"));
        assert!(texto.contains("[CONVERTIDO CON AVISOS]"));
        assert!(texto.contains("[OMITIDO POR COLISIÓN]"));
        assert!(texto.contains("Assets/Uno.prefab"));
        assert!(texto.contains("(sin ruta)"));
        assert!(texto.contains("(guid: g1)"));
    }

    #[test]
    fn faltantes_detecta_entrada_y_suelta_y_vacio_cuando_cubre() {
        let lectura = LecturaPaquete {
            entradas: vec![
                entrada("a", Some("Assets/a.prefab")),
                entrada("b", Some("Assets/b.prefab")),
            ],
            sueltas: vec!["suelta.txt".to_string(), "cubierta.txt".to_string()],
        };

        let mut parcial = InformeImportacion::nuevo();
        parcial.anadir(
            "a",
            Some("Assets/a.prefab"),
            EstadoInforme::Convertido,
            "ok",
        );
        parcial.anadir(
            "x",
            Some("cubierta.txt"),
            EstadoInforme::Copiado,
            "copiado",
        );

        let faltantes = parcial.faltantes(&lectura);
        assert!(faltantes.contains(&"b".to_string()));
        assert!(faltantes.contains(&"suelta.txt".to_string()));
        assert!(!faltantes.contains(&"a".to_string()));
        assert!(!faltantes.contains(&"cubierta.txt".to_string()));

        let mut completo = InformeImportacion::nuevo();
        completo.anadir(
            "a",
            Some("Assets/a.prefab"),
            EstadoInforme::Convertido,
            "ok",
        );
        completo.anadir(
            "b",
            Some("Assets/b.prefab"),
            EstadoInforme::Convertido,
            "ok",
        );
        completo.anadir(
            "y",
            Some("suelta.txt"),
            EstadoInforme::Copiado,
            "ok",
        );
        completo.anadir(
            "z",
            Some("cubierta.txt"),
            EstadoInforme::Copiado,
            "ok",
        );

        assert!(completo.faltantes(&lectura).is_empty());
    }

    #[test]
    fn acumular_prefab_suma_entre_escenas_y_cuenta_escenas_distintas() {
        let mut informe = InformeImportacion::nuevo();
        informe.acumular_prefab("escena1", "p1", "Puerta", 1, 3, 2);
        informe.acumular_prefab("escena2", "p1", "Puerta", 2, 6, 1);
        informe.acumular_prefab("escena1", "p2", "Lampara", 1, 1, 0);

        assert_eq!(informe.prefabs.len(), 2);
        let puerta = &informe.prefabs[0];
        assert_eq!(puerta.guid, "p1");
        assert_eq!(puerta.nombre, "Puerta");
        assert_eq!(puerta.instancias, 3);
        assert_eq!(puerta.entidades, 9);
        assert_eq!(puerta.aplicadas, 3);
        assert_eq!(puerta.escenas.len(), 2);
        let lampara = &informe.prefabs[1];
        assert_eq!(lampara.guid, "p2");
        assert_eq!(lampara.escenas.len(), 1);
        assert!(informe.lineas.is_empty());
    }

    #[test]
    fn acumular_prefab_varias_instancias_en_una_escena_cuentan_una_escena() {
        let mut informe = InformeImportacion::nuevo();
        informe.acumular_prefab("escena1", "p1", "Puerta", 1, 3, 2);
        informe.acumular_prefab("escena1", "p1", "Puerta", 2, 6, 0);

        assert_eq!(informe.prefabs.len(), 1);
        let puerta = &informe.prefabs[0];
        assert_eq!(puerta.instancias, 3);
        assert_eq!(puerta.entidades, 9);
        assert_eq!(puerta.aplicadas, 2);
        assert_eq!(puerta.escenas, vec!["escena1".to_string()]);
    }

    #[test]
    fn a_texto_seccion_prefabs_solo_con_datos_y_formato_exacto() {
        let mut informe = InformeImportacion::nuevo();
        informe.anadir(
            "g1",
            Some("Assets/Escena.unity"),
            EstadoInforme::Convertido,
            "escena: 2 entidades",
        );

        let sin = informe.a_texto();
        assert_eq!(
            sin,
            "Informe de importación\n\
             [CONVERTIDO] Assets/Escena.unity — escena: 2 entidades (guid: g1)\n"
        );
        assert!(!sin.contains("Prefabs:"));

        informe.acumular_prefab("g1", "p1", "Puerta", 2, 6, 1);
        informe.acumular_prefab("g2", "p1", "Puerta", 1, 3, 2);
        informe.acumular_prefab("g1", "p2", "Lampara", 1, 1, 0);

        let con = informe.a_texto();
        assert!(con.starts_with(&sin));
        assert_eq!(
            &con[sin.len()..],
            "Prefabs:\n\
             Prefab Puerta (guid p1): 3 instancia(s) en 2 escena(s), 9 entidades, 3 modificaciones aplicadas\n\
             Prefab Lampara (guid p2): 1 instancia(s) en 1 escena(s), 1 entidades, 0 modificaciones aplicadas\n"
        );
    }
}
