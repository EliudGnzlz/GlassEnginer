//! Composición de la conversión de scripts y su destino final.

use crate::convertidor::{convertir_script, MarcaScript, ResultadoScript};

/// Destino final de un script tras aplicar conversión y verificación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinoScript {
    /// Script de editor: no se integra como `.cs` de runtime.
    Editor,
    /// Script convertido y listo para integrarse.
    Convertido,
    /// Script bloqueado o descartado por errores.
    Cuarentena,
}

/// Resultado completo del procesado de un script.
pub struct ProcesadoScript {
    /// Destino final del script.
    pub destino: DestinoScript,
    /// Código final, si corresponde integrarlo.
    pub codigo: Option<String>,
    /// Marcas generadas durante conversión y verificación.
    pub marcas: Vec<MarcaScript>,
}

/// Verificador de compilación inyectable: recibe el código convertido
/// y devuelve los errores del compilador si no compila.
pub type VerificadorCompilacion = dyn Fn(&str) -> Result<(), Vec<String>>;

/// Devuelve la ruta de cuarentena para un script.
///
/// Todos los scripts incompatibles se agrupan bajo la carpeta
/// `Incompatibles/` en la raíz de la importación: así quedan juntos y
/// claramente marcados para la revisión del usuario, en lugar de repartidos
/// entre los scripts buenos. Dentro de esa carpeta se conserva la ruta
/// relativa original (para reconocer su origen) y se añade el sufijo
/// `.unity` (para que el script quede inerte y el motor no lo compile).
/// Nada se borra automáticamente: el usuario decide qué hacer con ellos.
pub fn ruta_cuarentena(ruta_relativa: &str) -> String {
    format!("Incompatibles/{ruta_relativa}.unity")
}

/// ¿El código menciona la clase como palabra completa?
///
/// Verdadero si `clase` aparece en `codigo` delimitada por caracteres que no
/// sean alfanuméricos ni `_` (o por los bordes del texto). Así se evita que
/// `Espera` dé positivo dentro de `Esperando`. No usa regex: comprueba los
/// bordes de cada ocurrencia con los caracteres vecinos.
pub fn menciona_clase(codigo: &str, clase: &str) -> bool {
    if clase.is_empty() {
        return false;
    }
    let es_borde = |c: char| !(c.is_alphanumeric() || c == '_');
    for (i, _) in codigo.match_indices(clase) {
        let antes_ok = codigo[..i].chars().next_back().map(es_borde).unwrap_or(true);
        let fin = i + clase.len();
        let despues_ok = codigo[fin..].chars().next().map(es_borde).unwrap_or(true);
        if antes_ok && despues_ok {
            return true;
        }
    }
    false
}

/// Procesa un script: conversión, detección de bloqueos y verificación opcional.
pub fn procesar_script(
    ruta_relativa: &str,
    texto: &str,
    verificador: Option<&VerificadorCompilacion>,
) -> ProcesadoScript {
    let ResultadoScript {
        codigo,
        mut marcas,
        es_editor,
    } = convertir_script(ruta_relativa, texto);

    if es_editor {
        return ProcesadoScript {
            destino: DestinoScript::Editor,
            codigo: None,
            marcas,
        };
    }

    let codigo = match codigo {
        Some(codigo) => codigo,
        None => {
            return ProcesadoScript {
                destino: DestinoScript::Cuarentena,
                codigo: None,
                marcas,
            };
        }
    };

    match verificador {
        None => ProcesadoScript {
            destino: DestinoScript::Convertido,
            codigo: Some(codigo),
            marcas,
        },
        Some(verificar) => match verificar(&codigo) {
            Ok(()) => ProcesadoScript {
                destino: DestinoScript::Convertido,
                codigo: Some(codigo),
                marcas,
            },
            Err(errores) => {
                for error in errores {
                    marcas.push(MarcaScript {
                        linea: 0,
                        elemento: "compilación".to_string(),
                        motivo: error,
                        bloqueante: true,
                    });
                }

                ProcesadoScript {
                    destino: DestinoScript::Cuarentena,
                    codigo: None,
                    marcas,
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verificador_ok(_: &str) -> Result<(), Vec<String>> {
        Ok(())
    }

    fn verificador_error(_: &str) -> Result<(), Vec<String>> {
        Err(vec!["error CS1002".to_string()])
    }

#[test]
    fn ruta_cuarentena_va_a_la_carpeta_de_incompatibles() {
        assert_eq!(
            ruta_cuarentena("Assets/X/Foo.cs"),
            "Incompatibles/Assets/X/Foo.cs.unity"
        );
    }

    #[test]
    fn menciona_clase_en_medio_de_codigo() {
        let codigo = "public class Usa : MonoBehaviour\n{\n    public Espera espera;\n}\n";
        assert!(menciona_clase(codigo, "Espera"));
    }

    #[test]
    fn menciona_clase_no_como_parte_de_otra_palabra() {
        let codigo = "public class Esperando {}\n";
        assert!(!menciona_clase(codigo, "Espera"));
    }

    #[test]
    fn menciona_clase_al_principio_del_texto() {
        let codigo = "Espera espera;";
        assert!(menciona_clase(codigo, "Espera"));
    }

    #[test]
    fn menciona_clase_al_final_del_texto() {
        let codigo = "public class Espera";
        assert!(menciona_clase(codigo, "Espera"));
    }

    #[test]
    fn menciona_clase_vacia_es_falso() {
        assert!(!menciona_clase("cualquier cosa", ""));
    }

    #[test]
    fn script_directo_sin_verificador_queda_convertido() {
        let texto = "using UnityEngine;\npublic class Foo : MonoBehaviour {}\n";
        let resultado = procesar_script("Assets/Foo.cs", texto, None);

        assert_eq!(resultado.destino, DestinoScript::Convertido);

        match resultado.codigo.as_deref() {
            Some(codigo) => assert!(codigo.contains("class Foo")),
            None => panic!("se esperaba código convertido"),
        }
    }

    #[test]
    fn script_con_corrutina_queda_en_cuarentena() {
        let texto = "using UnityEngine;\npublic class Foo : MonoBehaviour { void Start() { StartCoroutine(Bar()); } }\n";
        let resultado = procesar_script("Assets/Foo.cs", texto, None);

        assert_eq!(resultado.destino, DestinoScript::Cuarentena);
        assert!(resultado.codigo.is_none());
        assert!(resultado
            .marcas
            .iter()
            .any(|marca| marca.bloqueante && marca.elemento == "StartCoroutine"));
    }

    #[test]
    fn script_de_editor_queda_como_editor() {
        let texto = "using UnityEngine;\n";
        let resultado = procesar_script("Assets/Editor/Foo.cs", texto, None);

        assert_eq!(resultado.destino, DestinoScript::Editor);
        assert!(resultado.codigo.is_none());
    }

    #[test]
    fn verificador_decide_entre_convertido_y_cuarentena() {
        let texto = "using UnityEngine;\npublic class Foo : MonoBehaviour {}\n";

        let ok: &dyn Fn(&str) -> Result<(), Vec<String>> = &verificador_ok;
        let convertido = procesar_script("Assets/Foo.cs", texto, Some(ok));

        assert_eq!(convertido.destino, DestinoScript::Convertido);
        assert!(convertido.codigo.is_some());

        let error: &dyn Fn(&str) -> Result<(), Vec<String>> = &verificador_error;
        let cuarentena = procesar_script("Assets/Foo.cs", texto, Some(error));

        assert_eq!(cuarentena.destino, DestinoScript::Cuarentena);
        assert!(cuarentena.codigo.is_none());
        assert!(cuarentena.marcas.iter().any(|marca| {
            marca.linea == 0
                && marca.elemento == "compilación"
                && marca.motivo == "error CS1002"
                && marca.bloqueante
        }));
    }
}
