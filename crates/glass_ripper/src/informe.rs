use crate::{EstadoInforme, LecturaPaquete, LineaInforme};

#[derive(Debug, Clone, Default)]
pub struct InformeImportacion {
    pub lineas: Vec<LineaInforme>,
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

    /// Texto del informe: cabecera y una línea por entrada con el
    /// formato "[ESTADO] ruta — detalle (guid: …)". El estado se
    /// escribe: Convertido→CONVERTIDO, Copiado→COPIADO,
    /// ConvertidoConAvisos→CONVERTIDO CON AVISOS,
    /// NoConvertido→NO CONVERTIDO, Rechazado→RECHAZADO,
    /// OmitidoPorColision→OMITIDO POR COLISIÓN. Sin ruta se
    /// escribe "(sin ruta)".
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
}
