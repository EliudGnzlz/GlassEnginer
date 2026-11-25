//! Driver de importación de `.unitypackage` (U5).
//!
//! Orquesta los conversores del crate. Lee el paquete, parsea los
//! `.meta`, actualiza el registro de guids, calcula el plan de rutas de
//! salida y ejecuta las fases en un orden fijo: carpetas, texturas,
//! físicos, materiales, modelos, scripts, prefabs, escenas y el resto.
//!
//! Solo devuelven `Err` los fallos globales: no poder leer el paquete o
//! no poder cargar o guardar el registro. Cualquier problema de un asset
//! concreto se anota como línea del informe y la importación sigue.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::escenas::{convertir_escena, convertir_prefab, ContextoEscena, PrefabConvertido};
use crate::escritor::{escribir_asset, ResultadoEscritura};
use crate::fisico::convertir_material_fisico;
use crate::informe::InformeImportacion;
use crate::instancias::{dependencias_prefab, ordenar_prefabs};
use crate::materiales::convertir_material;
use crate::meta::parsear_meta;
use crate::MetaAsset;
use crate::modelos::convertir_modelo;
use crate::paquete::leer_paquete;
use crate::{EntradaPaquete, LecturaPaquete};
use crate::registro::RegistroGuids;
use crate::scripts::{
    menciona_clase, procesar_script, ruta_cuarentena, DestinoScript, VerificadorCompilacion,
};
use crate::texturas::convertir_textura;
use crate::{EstadoInforme, RipperError};

/// Detalle de la línea que se anota cuando el destino ya existe.
const DETALLE_COLISION: &str = "ya existe y no se sobrescribe";

/// Importa un `.unitypackage` en `raiz_assets` y devuelve el informe.
///
/// Con `sobrescribir == false`, los archivos que ya existen no se tocan
/// y su asset queda como `OmitidoPorColision`. `verificador`, si se
/// pasa, garantiza que los scripts convertidos compilan.
pub fn importar_paquete(
    ruta_paquete: &Path,
    raiz_assets: &Path,
    sobrescribir: bool,
    verificador: Option<&VerificadorCompilacion>,
) -> Result<InformeImportacion, RipperError> {
    let lectura = leer_paquete(ruta_paquete)?;
    let mut imp = Importador::nuevo(ruta_paquete, lectura, raiz_assets, sobrescribir, verificador);

    imp.parsear_metas();

    let dir_glass = raiz_assets.join(".glass");
    let ruta_registro = dir_glass.join("guids.json");
    let mut registro = RegistroGuids::cargar(&ruta_registro)?;
    imp.registrar(&mut registro);

    imp.calcular_plan();

    imp.fase_carpetas();
    imp.fase_texturas();
    imp.fase_fisicos();
    imp.fase_materiales();
    imp.fase_modelos();
    imp.fase_scripts();
    imp.fase_prefabs();
    imp.fase_escenas();
    imp.fase_resto();

    // Mejor esfuerzo: si `.glass` no se puede crear, `guardar` falla a
    // continuación y ese error es el Err global de la importación.
    let _ = fs::create_dir_all(&dir_glass);
    registro.guardar(&ruta_registro)?;

    Ok(imp.cerrar())
}

/// Clasificación interna de una entrada del paquete.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tipo {
    Carpeta,
    Textura,
    Fisico,
    Material,
    Modelo,
    ModeloNoFbx,
    Prefab,
    Escena,
    Script,
    AudioWav,
    AudioOtro,
    YamlSinConversor,
    Copia,
    SinBytes,
    Rechazado,
}

/// Destino de escritura y acumulador del informe.
struct Salida {
    raiz: PathBuf,
    sobrescribir: bool,
    informe: InformeImportacion,
}

impl Salida {
    /// Añade una línea al informe.
    fn linea(&mut self, guid: &str, ruta: Option<&str>, estado: EstadoInforme, detalle: &str) {
        self.informe.anadir(guid, ruta, estado, detalle);
    }

    /// Anota que el destino ya existía.
    fn colision(&mut self, guid: &str, ruta: &str) {
        self.linea(guid, Some(ruta), EstadoInforme::OmitidoPorColision, DETALLE_COLISION);
    }

    /// Escribe sin anotar nada.
    fn escribir_bruto(&self, destino: &str, bytes: &[u8]) -> Result<ResultadoEscritura, RipperError> {
        escribir_asset(&self.raiz, Path::new(destino), bytes, self.sobrescribir)
    }

    /// Escribe y anota la línea del asset más una línea por aviso.
    /// Con `copia = Some(detalle)`, la línea es `Copiado` con ese detalle.
    fn escribir(
        &mut self,
        guid: &str,
        ruta: &str,
        destino: &str,
        bytes: &[u8],
        copia: Option<&str>,
        avisos: &[String],
    ) {
        match self.escribir_bruto(destino, bytes) {
            Ok(ResultadoEscritura::Escrito) => {
                let (estado, detalle) = match copia {
                    Some(d) => (EstadoInforme::Copiado, d.to_string()),
                    None if avisos.is_empty() => {
                        (EstadoInforme::Convertido, format!("convertido a {destino}"))
                    }
                    None => (
                        EstadoInforme::ConvertidoConAvisos,
                        format!("convertido a {destino} con {} avisos", avisos.len()),
                    ),
                };
                self.linea(guid, Some(ruta), estado, &detalle);
                for aviso in avisos {
                    self.linea(guid, Some(ruta), EstadoInforme::ConvertidoConAvisos, aviso);
                }
            }
            Ok(ResultadoEscritura::OmitidoPorColision) => self.colision(guid, ruta),
            Err(err) => self.error_escritura(guid, ruta, destino, &err),
        }
    }

    /// Escribe un asset cuyo conversor ya anotó su línea: solo se
    /// informa de la colisión o del error de escritura.
    fn escribir_informado(&mut self, guid: &str, ruta: &str, destino: &str, bytes: &[u8]) {
        match self.escribir_bruto(destino, bytes) {
            Ok(ResultadoEscritura::Escrito) => {}
            Ok(ResultadoEscritura::OmitidoPorColision) => self.colision(guid, ruta),
            Err(err) => self.error_escritura(guid, ruta, destino, &err),
        }
    }

    /// Anota un fallo de escritura como `NoConvertido`.
    fn error_escritura(&mut self, guid: &str, ruta: &str, destino: &str, err: &RipperError) {
        let detalle = format!("no se pudo escribir {destino}: {err}");
        self.linea(guid, Some(ruta), EstadoInforme::NoConvertido, &detalle);
    }
}

/// Estado común de una importación.
struct Importador<'v> {
    ruta_paquete: String,
    lectura: LecturaPaquete,
    /// Tipo de cada entrada, paralelo a `lectura.entradas`.
    tipos: Vec<Tipo>,
    metas: HashMap<String, MetaAsset>,
    /// guid → ruta relativa final (con `/`) para todo lo que se escribe.
    plan: HashMap<String, String>,
    plan_texturas: HashMap<String, String>,
    plan_fisicos: HashMap<String, String>,
    /// guid de script convertido → nombre de clase (stem del archivo).
    clases: HashMap<String, String>,
    prefabs: HashMap<String, PrefabConvertido>,
    verificador: Option<&'v VerificadorCompilacion>,
    salida: Salida,
}

impl<'v> Importador<'v> {
    fn nuevo(
        ruta_paquete: &Path,
        lectura: LecturaPaquete,
        raiz: &Path,
        sobrescribir: bool,
        verificador: Option<&'v VerificadorCompilacion>,
    ) -> Self {
        let tipos = lectura.entradas.iter().map(clasificar).collect();
        Self {
            ruta_paquete: ruta_paquete.display().to_string(),
            lectura,
            tipos,
            metas: HashMap::new(),
            plan: HashMap::new(),
            plan_texturas: HashMap::new(),
            plan_fisicos: HashMap::new(),
            clases: HashMap::new(),
            prefabs: HashMap::new(),
            verificador,
            salida: Salida {
                raiz: raiz.to_path_buf(),
                sobrescribir,
                informe: InformeImportacion::default(),
            },
        }
    }

    /// Índices de las entradas de un tipo, en el orden del paquete.
    fn indices(&self, tipo: Tipo) -> Vec<usize> {
        self.tipos
            .iter()
            .enumerate()
            .filter(|(_, t)| **t == tipo)
            .map(|(i, _)| i)
            .collect()
    }

    /// Paso 1: parsea los `.meta`. Un meta ilegible no aborta.
    fn parsear_metas(&mut self) {
        for e in &self.lectura.entradas {
            let Some(texto) = e.meta_texto.as_deref() else {
                continue;
            };
            match parsear_meta(&e.guid, texto) {
                Ok(m) => {
                    self.metas.insert(e.guid.clone(), m);
                }
                Err(err) => self.salida.linea(
                    &e.guid,
                    e.ruta.as_deref(),
                    EstadoInforme::ConvertidoConAvisos,
                    &format!("meta ilegible: {err}"),
                ),
            }
        }
    }

    /// Paso 2: registra cada entrada con ruta y avisa si el guid cambió de ruta.
    fn registrar(&mut self, registro: &mut RegistroGuids) {
        for e in &self.lectura.entradas {
            let Some(ruta) = e.ruta.as_deref() else {
                continue;
            };
            let vieja = registro.resolver(&e.guid).map(|r| r.ruta.clone());
            if let Some(vieja) = vieja.filter(|v| v.as_str() != ruta) {
                let detalle = format!(
                    "el guid ya estaba registrado con otra ruta ({vieja}): se actualiza el registro"
                );
                self.salida
                    .linea(&e.guid, Some(ruta), EstadoInforme::ConvertidoConAvisos, &detalle);
            }
            let importer = self.metas.get(&e.guid).map_or("", |m| m.importer.as_str());
            let _ = registro.registrar(&e.guid, ruta, importer);
        }
    }

    /// Paso 3: calcula la ruta relativa final de cada entrada convertible.
    fn calcular_plan(&mut self) {
        for (e, tipo) in self.lectura.entradas.iter().zip(&self.tipos) {
            let Some(ruta) = e.ruta.as_deref() else {
                continue;
            };
            let esp = espejo(ruta);
            let destino = match *tipo {
                Tipo::Textura => con_extension(esp, "png"),
                Tipo::Material => format!("materials/{}.material.json", stem(ruta)),
                Tipo::Fisico => con_extension(esp, "physmat.json"),
                Tipo::Modelo => con_extension(esp, "glb"),
                Tipo::Prefab => con_extension(esp, "prefab.json"),
                Tipo::Escena => con_extension(esp, "scene.json"),
                Tipo::Script | Tipo::AudioWav | Tipo::Copia => esp.to_string(),
                _ => continue,
            };
            match *tipo {
                Tipo::Textura => {
                    self.plan_texturas.insert(e.guid.clone(), destino.clone());
                }
                Tipo::Fisico => {
                    self.plan_fisicos.insert(e.guid.clone(), destino.clone());
                }
                _ => {}
            }
            self.plan.insert(e.guid.clone(), destino);
        }
    }

    /// Fase a: las carpetas no producen archivo, solo su línea.
    fn fase_carpetas(&mut self) {
        for i in self.indices(Tipo::Carpeta) {
            let e = &self.lectura.entradas[i];
            self.salida
                .linea(&e.guid, e.ruta.as_deref(), EstadoInforme::Convertido, "carpeta");
        }
    }

    /// Fase b: texturas a PNG.
    fn fase_texturas(&mut self) {
        for i in self.indices(Tipo::Textura) {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(bytes), Some(destino)) =
                (e.ruta.as_deref(), e.asset.as_deref(), self.plan.get(&e.guid))
            else {
                continue;
            };
            let ext = extension(ruta);
            match convertir_textura(ruta, &ext, bytes, self.metas.get(&e.guid)) {
                Ok((png, avisos)) => self.salida.escribir(&e.guid, ruta, destino, &png, None, &avisos),
                Err(err) => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &err.to_string(),
                ),
            }
        }
    }

    /// Fase c: materiales físicos a `.physmat.json`.
    fn fase_fisicos(&mut self) {
        for i in self.indices(Tipo::Fisico) {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(destino)) = (e.ruta.as_deref(), self.plan.get(&e.guid)) else {
                continue;
            };
            let texto = match texto_de(e) {
                Ok(t) => t,
                Err(m) => {
                    self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m);
                    continue;
                }
            };
            match convertir_material_fisico(ruta, texto) {
                Ok((pm, avisos)) => match serde_json::to_string_pretty(&pm) {
                    Ok(json) => {
                        self.salida
                            .escribir(&e.guid, ruta, destino, json.as_bytes(), None, &avisos);
                    }
                    Err(err) => self.salida.linea(
                        &e.guid,
                        Some(ruta),
                        EstadoInforme::NoConvertido,
                        &format!("no se pudo serializar: {err}"),
                    ),
                },
                Err(err) => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &err.to_string(),
                ),
            }
        }
    }

    /// Fase d: materiales planos en `materials/`. Dos materiales con el
    /// mismo stem chocan dentro de la misma importación aunque se
    /// permita sobrescribir.
    fn fase_materiales(&mut self) {
        let plan_tex = &self.plan_texturas;
        let resolver = |g: &str| plan_tex.get(g).cloned();
        let mut usados: HashSet<String> = HashSet::new();
        for i in self.indices(Tipo::Material) {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(destino)) = (e.ruta.as_deref(), self.plan.get(&e.guid)) else {
                continue;
            };
            if !usados.insert(destino.clone()) {
                self.salida.colision(&e.guid, ruta);
                continue;
            }
            let texto = match texto_de(e) {
                Ok(t) => t,
                Err(m) => {
                    self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m);
                    continue;
                }
            };
            match convertir_material(ruta, texto, &resolver) {
                Ok((md, avisos)) => match serde_json::to_string_pretty(&md) {
                    Ok(json) => {
                        self.salida
                            .escribir(&e.guid, ruta, destino, json.as_bytes(), None, &avisos);
                    }
                    Err(err) => self.salida.linea(
                        &e.guid,
                        Some(ruta),
                        EstadoInforme::NoConvertido,
                        &format!("no se pudo serializar: {err}"),
                    ),
                },
                Err(err) => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &err.to_string(),
                ),
            }
        }
    }

    /// Fase e: modelos FBX a GLB a través de un directorio temporal.
    fn fase_modelos(&mut self) {
        let indices = self.indices(Tipo::Modelo);
        if indices.is_empty() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("glass_ripper_import_{}", std::process::id()));
        let scratch_ok = fs::create_dir_all(&scratch).map_err(|e| e.to_string());
        for i in indices {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(bytes), Some(destino)) =
                (e.ruta.as_deref(), e.asset.as_deref(), self.plan.get(&e.guid))
            else {
                continue;
            };
            if let Err(m) = &scratch_ok {
                let detalle = format!("no se pudo crear el directorio temporal: {m}");
                self.salida
                    .linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &detalle);
                continue;
            }
            match convertir_fbx(&scratch, &e.guid, bytes, self.metas.get(&e.guid)) {
                Ok((glb, avisos)) => self.salida.escribir(&e.guid, ruta, destino, &glb, None, &avisos),
                Err(m) => self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m),
            }
        }
        let _ = fs::remove_dir_all(&scratch);
    }

// el import pasa a incluir menciona_clase
    /// Fase f: scripts C#. Anota la clase de cada script convertido.
    fn fase_scripts(&mut self) {
        let indices = self.indices(Tipo::Script);
        let mut convertidos: Vec<(String, String)> = Vec::new();
        let mut incompatibles: Vec<(String, String, String)> = Vec::new();
        for &i in &indices {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(destino)) = (e.ruta.as_deref(), self.plan.get(&e.guid)) else {
                continue;
            };
            let texto = match texto_de(e) {
                Ok(t) => t,
                Err(m) => {
                    self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m);
                    continue;
                }
            };
            let p = procesar_script(destino, texto, self.verificador);
            match p.destino {
                DestinoScript::Editor => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    "script de editor: no se importa",
                ),
                DestinoScript::Convertido => {
                    let codigo = p.codigo.as_deref().unwrap_or(texto);
                    let avisos: Vec<String> = p
                        .marcas
                        .iter()
                        .map(|m| format!("línea {}: {} — {}", m.linea, m.elemento, m.motivo))
                        .collect();
                    self.salida
                        .escribir(&e.guid, ruta, destino, codigo.as_bytes(), None, &avisos);
                    // Regla de Unity: la clase se llama como el archivo.
                    self.clases.insert(e.guid.clone(), stem(ruta).to_string());
                    convertidos.push((destino.clone(), codigo.to_string()));
                }
                DestinoScript::Cuarentena => {
                    let destino_q = ruta_cuarentena(destino);
                    let contenido = p.codigo.as_deref().unwrap_or(texto);
                    match self.salida.escribir_bruto(&destino_q, contenido.as_bytes()) {
                        Ok(ResultadoEscritura::Escrito) => {}
                        Ok(ResultadoEscritura::OmitidoPorColision) => {
                            self.salida.colision(&e.guid, ruta);
                        }
                        Err(err) => self.salida.error_escritura(&e.guid, ruta, &destino_q, &err),
                    }
                    let detalle =
                        format!("en la carpeta Incompatibles: {} marcas", p.marcas.len());
                    self.salida
                        .linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &detalle);
                    for m in &p.marcas {
                        let detalle = format!("línea {}: {} — {}", m.linea, m.elemento, m.motivo);
                        self.salida
                            .linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &detalle);
                    }
                    let clase = stem(destino).to_string();
                    incompatibles.push((e.guid.clone(), ruta.to_string(), clase));
                }
            }
        }
        if self.verificador.is_none() && !indices.is_empty() {
            self.salida.linea(
                "",
                Some(&self.ruta_paquete),
                EstadoInforme::ConvertidoConAvisos,
                "scripts convertidos SIN la garantía de compilación (no se pasó verificador)",
            );
        }
        let mut sin_referencias = 0usize;
        for (guid, ruta, clase) in &incompatibles {
            let referentes: Vec<&str> = convertidos
                .iter()
                .filter(|(_, codigo)| menciona_clase(codigo, clase))
                .map(|(destino, _)| destino.as_str())
                .collect();
            if referentes.is_empty() {
                sin_referencias += 1;
            } else {
                let lista = referentes.join(", ");
                self.salida.linea(
                    guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &format!("referenciado por scripts convertidos: {lista}"),
                );
            }
        }
        if sin_referencias > 0 {
            self.salida.linea(
                "",
                Some(&self.ruta_paquete),
                EstadoInforme::ConvertidoConAvisos,
                &format!(
                    "{sin_referencias} scripts en Incompatibles sin referencias de scripts convertidos"
                ),
            );
        }
    }

    /// Fase g: prefabs en orden de dependencias. Los ciclos se saltan.
    fn fase_prefabs(&mut self) {
        let indices = self.indices(Tipo::Prefab);
        let mut por_guid: HashMap<String, usize> = HashMap::new();
        let mut nodos: Vec<(String, Vec<String>)> = Vec::new();
        for &i in &indices {
            let e = &self.lectura.entradas[i];
            let Some(ruta) = e.ruta.as_deref() else {
                continue;
            };
            match texto_de(e) {
                Ok(t) => {
                    por_guid.insert(e.guid.clone(), i);
                    nodos.push((e.guid.clone(), dependencias_prefab(t)));
                }
                Err(m) => self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m),
            }
        }
        // Solo cuentan como dependencias los prefabs de este paquete.
        for (_, deps) in &mut nodos {
            deps.retain(|d| por_guid.contains_key(d));
        }
        let (orden, en_ciclo) = ordenar_con_ciclos(nodos);
        for &i in &indices {
            let e = &self.lectura.entradas[i];
            if en_ciclo.contains(&e.guid) {
                self.salida.linea(
                    &e.guid,
                    e.ruta.as_deref(),
                    EstadoInforme::NoConvertido,
                    "ciclo de prefabs",
                );
            }
        }

        let plan = &self.plan;
        let clases = &self.clases;
        let fisicos = &self.plan_fisicos;
        let metas = &self.metas;
        let rutas = |g: &str| plan.get(g).cloned();
        let clase = |g: &str| clases.get(g).cloned();
        let physmat = |g: &str| fisicos.get(g).cloned();
        let objeto = |g: &str, fid: i64| {
            metas.get(g).and_then(|m| {
                m.sub_objetos
                    .iter()
                    .find(|s| s.file_id == fid)
                    .map(|s| s.nombre.clone())
            })
        };
        let ctx = ContextoEscena {
            rutas: &rutas,
            clase_de_script: &clase,
            physmat_de_guid: &physmat,
            objeto_de_submalla: &objeto,
        };

        for guid in orden {
            let Some(&i) = por_guid.get(&guid) else {
                continue;
            };
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Ok(texto), Some(destino)) =
                (e.ruta.as_deref(), texto_de(e), plan.get(&e.guid))
            else {
                continue;
            };
            let resultado = convertir_prefab(
                &e.guid,
                ruta,
                texto,
                &ctx,
                &self.prefabs,
                &mut self.salida.informe,
            );
            match resultado {
                Ok(pc) => {
                    match serde_json::to_string_pretty(&pc.prefab) {
                        Ok(json) => {
                            self.salida
                                .escribir_informado(&e.guid, ruta, destino, json.as_bytes());
                        }
                        Err(err) => self.salida.linea(
                            &e.guid,
                            Some(ruta),
                            EstadoInforme::NoConvertido,
                            &format!("no se pudo serializar: {err}"),
                        ),
                    }
                    self.prefabs.insert(e.guid.clone(), pc);
                }
                Err(err) => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &err.to_string(),
                ),
            }
        }
    }

    /// Fase h: escenas con los prefabs ya convertidos.
    fn fase_escenas(&mut self) {
        let plan = &self.plan;
        let clases = &self.clases;
        let fisicos = &self.plan_fisicos;
        let metas = &self.metas;
        let rutas = |g: &str| plan.get(g).cloned();
        let clase = |g: &str| clases.get(g).cloned();
        let physmat = |g: &str| fisicos.get(g).cloned();
        let objeto = |g: &str, fid: i64| {
            metas.get(g).and_then(|m| {
                m.sub_objetos
                    .iter()
                    .find(|s| s.file_id == fid)
                    .map(|s| s.nombre.clone())
            })
        };
        let ctx = ContextoEscena {
            rutas: &rutas,
            clase_de_script: &clase,
            physmat_de_guid: &physmat,
            objeto_de_submalla: &objeto,
        };

        for i in self.indices(Tipo::Escena) {
            let e = &self.lectura.entradas[i];
            let (Some(ruta), Some(destino)) = (e.ruta.as_deref(), plan.get(&e.guid)) else {
                continue;
            };
            let texto = match texto_de(e) {
                Ok(t) => t,
                Err(m) => {
                    self.salida.linea(&e.guid, Some(ruta), EstadoInforme::NoConvertido, &m);
                    continue;
                }
            };
            let resultado = convertir_escena(
                &e.guid,
                ruta,
                texto,
                &ctx,
                &self.prefabs,
                &mut self.salida.informe,
            );
            match resultado {
                Ok(entidades) => match serde_json::to_string_pretty(&entidades) {
                    Ok(json) => {
                        self.salida
                            .escribir_informado(&e.guid, ruta, destino, json.as_bytes());
                    }
                    Err(err) => self.salida.linea(
                        &e.guid,
                        Some(ruta),
                        EstadoInforme::NoConvertido,
                        &format!("no se pudo serializar: {err}"),
                    ),
                },
                Err(err) => self.salida.linea(
                    &e.guid,
                    Some(ruta),
                    EstadoInforme::NoConvertido,
                    &err.to_string(),
                ),
            }
        }
    }

    /// Fase i: audio, copias, YAML sin conversor, modelos no FBX,
    /// entradas sin bytes, rechazadas y sueltas.
    fn fase_resto(&mut self) {
        for (i, e) in self.lectura.entradas.iter().enumerate() {
            let tipo = self.tipos[i];
            let ruta = e.ruta.as_deref();
            match tipo {
                Tipo::AudioWav | Tipo::Copia => {
                    let (Some(ruta), Some(bytes), Some(destino)) =
                        (ruta, e.asset.as_deref(), self.plan.get(&e.guid))
                    else {
                        continue;
                    };
                    let detalle = if tipo == Tipo::AudioWav {
                        "audio WAV: copia tal cual"
                    } else {
                        "copia tal cual, sin conversor en v1"
                    };
                    self.salida
                        .escribir(&e.guid, ruta, destino, bytes, Some(detalle), &[]);
                }
                Tipo::AudioOtro => self.salida.linea(
                    &e.guid,
                    ruta,
                    EstadoInforme::NoConvertido,
                    "formato de audio sin soporte: el motor solo carga WAV en v1",
                ),
                Tipo::ModeloNoFbx => self.salida.linea(
                    &e.guid,
                    ruta,
                    EstadoInforme::NoConvertido,
                    "formato de modelo sin conversor en v1 (solo FBX)",
                ),
                Tipo::YamlSinConversor => self.salida.linea(
                    &e.guid,
                    ruta,
                    EstadoInforme::NoConvertido,
                    "asset YAML sin conversor en v1",
                ),
                Tipo::SinBytes => self.salida.linea(
                    &e.guid,
                    ruta,
                    EstadoInforme::NoConvertido,
                    "la entrada no trae bytes del asset",
                ),
                Tipo::Rechazado => self.salida.linea(
                    &e.guid,
                    None,
                    EstadoInforme::Rechazado,
                    "entrada rechazada por el lector del paquete (sin ruta válida)",
                ),
                _ => {}
            }
        }
        for nombre in &self.lectura.sueltas {
            self.salida.linea(
                "",
                Some(nombre),
                EstadoInforme::Copiado,
                "entrada suelta del paquete: sin bytes en el lector de U1, no se copia",
            );
        }
    }

    /// Paso 6: red de seguridad del invariante y entrega del informe.
    fn cerrar(mut self) -> InformeImportacion {
        let faltan = self.salida.informe.faltantes(&self.lectura);
        for guid in faltan {
            let ruta = self
                .lectura
                .entradas
                .iter()
                .find(|e| e.guid == guid)
                .and_then(|e| e.ruta.as_deref());
            self.salida.linea(
                &guid,
                ruta,
                EstadoInforme::NoConvertido,
                "sin fase de conversión en v1",
            );
        }
        self.salida.informe
    }
}

/// Vuelca el FBX al temporal, lo convierte y devuelve los bytes del GLB.
fn convertir_fbx(
    scratch: &Path,
    guid: &str,
    bytes: &[u8],
    meta: Option<&MetaAsset>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let fuente = scratch.join(format!("{guid}.fbx"));
    let destino = scratch.join(format!("{guid}.glb"));
    fs::write(&fuente, bytes).map_err(|e| format!("no se pudo volcar el FBX al temporal: {e}"))?;
    let (_informe, avisos) =
        convertir_modelo(&fuente, &destino, meta).map_err(|e| e.to_string())?;
    let glb = fs::read(&destino).map_err(|e| format!("no se pudo leer el GLB generado: {e}"))?;
    Ok((glb, avisos))
}

/// Ordena los prefabs. Devuelve el orden y los guids que quedan fuera
/// por estar en un ciclo.
fn ordenar_con_ciclos(mut nodos: Vec<(String, Vec<String>)>) -> (Vec<String>, HashSet<String>) {
    let mut en_ciclo: HashSet<String> = HashSet::new();
    loop {
        match ordenar_prefabs(&nodos) {
            Ok(orden) => return (orden, en_ciclo),
            Err(ciclo) => {
                let antes = nodos.len();
                en_ciclo.extend(ciclo);
                nodos.retain(|(g, _)| !en_ciclo.contains(g));
                if nodos.len() == antes {
                    // Salvaguarda: el ciclo no nombró ningún nodo conocido.
                    // Se sigue en el orden del paquete para no entrar en bucle.
                    let orden = nodos.into_iter().map(|(g, _)| g).collect();
                    return (orden, en_ciclo);
                }
                for (_, deps) in &mut nodos {
                    deps.retain(|d| !en_ciclo.contains(d));
                }
            }
        }
    }
}

/// Clasifica una entrada según su ruta y sus bytes.
fn clasificar(e: &EntradaPaquete) -> Tipo {
    let Some(ruta) = e.ruta.as_deref() else {
        return Tipo::Rechazado;
    };
    if e.es_carpeta {
        return Tipo::Carpeta;
    }
    let Some(bytes) = e.asset.as_deref() else {
        return Tipo::SinBytes;
    };
    match extension(ruta).as_str() {
        "png" | "jpg" | "jpeg" | "tga" | "bmp" | "gif" | "psd" | "exr" => Tipo::Textura,
        "mat" => Tipo::Material,
        "physicmaterial" | "physicsmaterial" => Tipo::Fisico,
        "fbx" => Tipo::Modelo,
        "obj" | "dae" | "blend" => Tipo::ModeloNoFbx,
        "prefab" => Tipo::Prefab,
        "unity" => Tipo::Escena,
        "cs" => Tipo::Script,
        "wav" => Tipo::AudioWav,
        "mp3" | "ogg" | "flac" | "aif" | "aiff" => Tipo::AudioOtro,
        _ => {
            let sin_bom = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            if sin_bom.starts_with(b"%YAML") {
                Tipo::YamlSinConversor
            } else {
                Tipo::Copia
            }
        }
    }
}

/// Bytes del asset como texto UTF-8, sin BOM.
fn texto_de(e: &EntradaPaquete) -> Result<&str, String> {
    let bytes = e
        .asset
        .as_deref()
        .ok_or_else(|| "la entrada no trae bytes del asset".to_string())?;
    std::str::from_utf8(bytes)
        .map(|t| t.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("el asset no es texto UTF-8: {err}"))
}

/// Quita el prefijo `Assets/` o `Packages/` de una ruta de Unity.
fn espejo(ruta: &str) -> &str {
    ruta.strip_prefix("Assets/")
        .or_else(|| ruta.strip_prefix("Packages/"))
        .unwrap_or(ruta)
}

/// Último componente de una ruta con `/`.
fn nombre_archivo(ruta: &str) -> &str {
    ruta.rfind('/').map_or(ruta, |p| &ruta[p + 1..])
}

/// Nombre de archivo sin la última extensión.
fn stem(ruta: &str) -> &str {
    let nombre = nombre_archivo(ruta);
    match nombre.rfind('.') {
        Some(p) if p > 0 => &nombre[..p],
        _ => nombre,
    }
}

/// Extensión en minúsculas, sin punto; cadena vacía si no hay.
fn extension(ruta: &str) -> String {
    let nombre = nombre_archivo(ruta);
    match nombre.rfind('.') {
        Some(p) if p > 0 => nombre[p + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Sustituye la última extensión de la ruta por `ext` (o la añade).
fn con_extension(ruta: &str, ext: &str) -> String {
    let inicio = ruta.rfind('/').map_or(0, |p| p + 1);
    let base = match ruta[inicio..].rfind('.') {
        Some(p) if p > 0 => &ruta[..inicio + p],
        _ => ruta,
    };
    format!("{base}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::{GzEncoder, ZlibEncoder};
    use flate2::Compression;
    use std::io::Write;

    const G_CARPETA: &str = "a0000000000000000000000000000001";
    const G_TEX: &str = "a0000000000000000000000000000002";
    const G_MAT: &str = "a0000000000000000000000000000003";
    const G_DIRECTO: &str = "a0000000000000000000000000000004";
    const G_CORRUTINA: &str = "a0000000000000000000000000000005";
    const G_BIN: &str = "a0000000000000000000000000000006";
    const G_CTRL: &str = "a0000000000000000000000000000007";

    const R_TEX: &str = "Assets/Synty/Tex/Roca.png";
    const R_MAT: &str = "Assets/Synty/Mat/Roca.mat";
    const R_CTRL: &str = "Assets/Synty/Anim/Jugador.controller";

    type Tar = tar::Builder<GzEncoder<Vec<u8>>>;

    fn dir_temporal(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("glass_u5_{}_{}", std::process::id(), nombre));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn crc32(datos: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &b in datos {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mascara = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mascara);
            }
        }
        !crc
    }

    fn trozo(png: &mut Vec<u8>, tipo: &[u8], datos: &[u8]) {
        png.extend_from_slice(&u32::try_from(datos.len()).unwrap().to_be_bytes());
        let mut cuerpo = tipo.to_vec();
        cuerpo.extend_from_slice(datos);
        png.extend_from_slice(&cuerpo);
        png.extend_from_slice(&crc32(&cuerpo).to_be_bytes());
    }

    /// PNG RGB de 1×1 válida, con CRC y zlib reales.
    fn png_1x1() -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        trozo(&mut png, b"IHDR", &ihdr);
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&[0, 200, 100, 50]).unwrap();
        trozo(&mut png, b"IDAT", &z.finish().unwrap());
        trozo(&mut png, b"IEND", &[]);
        png
    }

    fn archivo(b: &mut Tar, ruta: &str, datos: &[u8]) {
        let mut h = tar::Header::new_gnu();
        h.set_size(u64::try_from(datos.len()).unwrap());
        h.set_mode(0o644);
        b.append_data(&mut h, ruta, datos).unwrap();
    }

    fn entrada(b: &mut Tar, guid: &str, ruta: &str, importer: &str, asset: Option<&[u8]>) {
        archivo(b, &format!("{guid}/pathname"), ruta.as_bytes());
        let extra = if asset.is_none() { "folderAsset: yes\n" } else { "" };
        let meta = format!("fileFormatVersion: 2\nguid: {guid}\n{extra}{importer}:\n");
        archivo(b, &format!("{guid}/asset.meta"), meta.as_bytes());
        if let Some(datos) = asset {
            archivo(b, &format!("{guid}/asset"), datos);
        }
    }

    fn nuevo_tar() -> Tar {
        tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()))
    }

    fn cerrar_tar(b: Tar, destino: &Path) {
        let gz = b.into_inner().unwrap();
        fs::write(destino, gz.finish().unwrap()).unwrap();
    }

    fn material_yaml() -> String {
        format!(
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!21 &2100000\nMaterial:\n  \
             serializedVersion: 6\n  m_ObjectHideFlags: 0\n  m_Name: Roca\n  \
             m_Shader: {{fileID: 46, guid: 0000000000000000f000000000000000, type: 0}}\n  \
             m_SavedProperties:\n    serializedVersion: 3\n    m_TexEnvs:\n    - _MainTex:\n        \
             m_Texture: {{fileID: 2800000, guid: {G_TEX}, type: 3}}\n        \
             m_Scale: {{x: 1, y: 1}}\n        m_Offset: {{x: 0, y: 0}}\n    m_Floats:\n    \
             - _Glossiness: 0.5\n    m_Colors:\n    - _Color: {{r: 1, g: 1, b: 1, a: 1}}\n"
        )
    }

    const SCRIPT_DIRECTO: &str = "using UnityEngine;\n\npublic class Mover : MonoBehaviour\n{\n    \
        public float velocidad = 1.0f;\n\n    void Update()\n    {\n    }\n}\n";

    const SCRIPT_CORRUTINA: &str = "using System.Collections;\nusing UnityEngine;\n\n\
        public class Espera : MonoBehaviour\n{\n    void Start()\n    {\n        \
        StartCoroutine(Esperar());\n    }\n\n    IEnumerator Esperar()\n    {\n        \
        yield return null;\n    }\n}\n";

    const BIN: &[u8] = &[0x00, 0x01, 0x02, 0xFF, 0xFE, 0x10, 0x00, 0x7F];

    fn paquete_base(destino: &Path) {
        let mut b = nuevo_tar();
        entrada(&mut b, G_CARPETA, "Assets/Synty", "DefaultImporter", None);
        entrada(&mut b, G_TEX, R_TEX, "TextureImporter", Some(&png_1x1()));
        entrada(&mut b, G_MAT, R_MAT, "NativeFormatImporter", Some(material_yaml().as_bytes()));
        entrada(
            &mut b,
            G_DIRECTO,
            "Assets/Synty/Scripts/Mover.cs",
            "MonoImporter",
            Some(SCRIPT_DIRECTO.as_bytes()),
        );
        entrada(
            &mut b,
            G_CORRUTINA,
            "Assets/Synty/Scripts/Espera.cs",
            "MonoImporter",
            Some(SCRIPT_CORRUTINA.as_bytes()),
        );
        entrada(&mut b, G_BIN, "Assets/Synty/Datos/blob.bin", "DefaultImporter", Some(BIN));
        cerrar_tar(b, destino);
    }

    /// Hay una línea del asset (por guid o ruta) con el detalle dado.
    fn hay_linea(texto: &str, guid: &str, ruta: &str, detalle: &str) -> bool {
        texto
            .lines()
            .any(|l| l.contains(detalle) && (l.contains(guid) || l.contains(ruta)))
    }

    #[test]
    fn importa_paquete_basico() {
        let d = dir_temporal("basico");
        let paquete = d.join("p.unitypackage");
        let raiz = d.join("assets");
        paquete_base(&paquete);

        let informe = importar_paquete(&paquete, &raiz, false, None).unwrap();

        assert!(raiz.join("Synty/Tex/Roca.png").is_file());

        let mat = fs::read_to_string(raiz.join("materials/Roca.material.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&mat).unwrap();
        assert_eq!(v["texture"].as_str(), Some("Synty/Tex/Roca.png"));

        assert!(raiz.join("Synty/Scripts/Mover.cs").is_file());
        assert!(raiz.join("Incompatibles/Synty/Scripts/Espera.cs.unity").is_file());
        assert!(!raiz.join("Synty/Scripts/Espera.cs.unity").exists());
        assert!(!raiz.join("Synty/Scripts/Espera.cs").exists());

        assert_eq!(fs::read(raiz.join("Synty/Datos/blob.bin")).unwrap(), BIN);

        let ruta_registro = raiz.join(".glass").join("guids.json");
        assert!(ruta_registro.is_file());
        let registro = RegistroGuids::cargar(&ruta_registro).unwrap();
        assert_eq!(registro.len(), 6);
        for g in [G_CARPETA, G_TEX, G_MAT, G_DIRECTO, G_CORRUTINA, G_BIN] {
            assert!(registro.resolver(g).is_some(), "falta {g} en el registro");
        }

        let lectura = leer_paquete(&paquete).unwrap();
        assert!(informe.faltantes(&lectura).is_empty());

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn cuarentena_en_carpeta_e_informe_marca_referencias() {
        let d = dir_temporal("cuarentena_referencias");
        let paquete = d.join("p.unitypackage");
        let raiz = d.join("assets");

        let mut b = nuevo_tar();
        let usa_cs = "using UnityEngine;\n\npublic class Usa : MonoBehaviour\n{\n    public Espera espera;\n}\n";
        entrada(
            &mut b,
            G_DIRECTO,
            "Assets/Synty/Scripts/Usa.cs",
            "MonoImporter",
            Some(usa_cs.as_bytes()),
        );
        entrada(
            &mut b,
            G_CORRUTINA,
            "Assets/Synty/Scripts/Espera.cs",
            "MonoImporter",
            Some(SCRIPT_CORRUTINA.as_bytes()),
        );
        cerrar_tar(b, &paquete);

        let informe = importar_paquete(&paquete, &raiz, false, None).unwrap();

        assert!(raiz.join("Incompatibles/Synty/Scripts/Espera.cs.unity").is_file());
        assert!(raiz.join("Synty/Scripts/Usa.cs").is_file());

        assert!(hay_linea(
            &informe.a_texto(),
            G_CORRUTINA,
            "Assets/Synty/Scripts/Espera.cs",
            "referenciado por scripts convertidos: Synty/Scripts/Usa.cs",
        ));
    }

    #[test]
    fn segunda_pasada_no_sobrescribe() {
        let d = dir_temporal("segunda");
        let paquete = d.join("p.unitypackage");
        let raiz = d.join("assets");
        paquete_base(&paquete);

        importar_paquete(&paquete, &raiz, false, None).unwrap();
        let ruta_mat = raiz.join("materials/Roca.material.json");
        fs::write(&ruta_mat, "marca").unwrap();

        let informe = importar_paquete(&paquete, &raiz, false, None).unwrap();
        let texto = informe.a_texto();

        assert!(hay_linea(&texto, G_TEX, R_TEX, DETALLE_COLISION));
        assert!(hay_linea(&texto, G_MAT, R_MAT, DETALLE_COLISION));
        assert_eq!(fs::read_to_string(&ruta_mat).unwrap(), "marca");

        let lectura = leer_paquete(&paquete).unwrap();
        assert!(informe.faltantes(&lectura).is_empty());

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn yaml_sin_conversor_no_se_escribe() {
        let d = dir_temporal("yaml");
        let paquete = d.join("p.unitypackage");
        let raiz = d.join("assets");
        let ctrl = "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!91 &9100000\n\
                    AnimatorController:\n  m_Name: Jugador\n";
        let mut b = nuevo_tar();
        entrada(&mut b, G_CTRL, R_CTRL, "NativeFormatImporter", Some(ctrl.as_bytes()));
        cerrar_tar(b, &paquete);

        let informe = importar_paquete(&paquete, &raiz, false, None).unwrap();

        assert!(hay_linea(
            &informe.a_texto(),
            G_CTRL,
            R_CTRL,
            "asset YAML sin conversor en v1"
        ));
        assert!(!raiz.join("Synty/Anim/Jugador.controller").exists());
        assert!(!raiz.join("Synty/Anim").exists());

        let lectura = leer_paquete(&paquete).unwrap();
        assert!(informe.faltantes(&lectura).is_empty());

        let _ = fs::remove_dir_all(&d);
    }
}
