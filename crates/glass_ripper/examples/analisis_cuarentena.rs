//! Análisis de cuarentenas (verificación NeoFPS, 2026-11-25): recorre
//! los `.cs.unity` que una importación dejó en cuarentena, vuelve a
//! pasar cada uno por `procesar_script` sin verificador (igual que la
//! corrida original) y agrega las marcas bloqueantes por motivo y por
//! símbolo de preprocesador, para decidir con datos qué símbolos se
//! pueden resolver con evidencia y cuáles no.
//!
//! Uso: cargo run -p glass_ripper --example analisis_cuarentena -- <raíz importación>
//! La raíz contiene el árbol `NeoFPS/...`; las rutas relativas se
//! reconstruyen como `Assets/<ruta>` para que la detección de
//! scripts de editor vea la misma ruta que vio el importador.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glass_ripper::scripts::{procesar_script, DestinoScript};

fn recoger(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.is_dir() {
            recoger(&ruta, out);
        } else if ruta.to_string_lossy().ends_with(".cs.unity") {
            out.push(ruta);
        }
    }
}

fn main() {
    let raiz = std::env::args()
        .nth(1)
        .expect("uso: analisis_cuarentena <raíz importación>");
    let raiz = PathBuf::from(raiz);
    let mut archivos = Vec::new();
    recoger(&raiz, &mut archivos);
    archivos.sort();
    println!("scripts en cuarentena encontrados: {}", archivos.len());

    // Motivo bloqueante completo -> nº de scripts que lo sufren.
    let mut por_motivo: BTreeMap<String, usize> = BTreeMap::new();
    // Símbolo suelto dentro de condiciones no resolubles -> nº de scripts.
    let mut por_simbolo: BTreeMap<String, usize> = BTreeMap::new();
    let mut solo_preprocesador = 0usize;
    let mut con_otros_bloqueos = 0usize;
    let mut no_cuarentena = 0usize;

    for ruta in &archivos {
        let rel = ruta.strip_prefix(&raiz).unwrap_or(ruta);
        let ruta_logica = format!("Assets/{}", rel.to_string_lossy().replace('\\', "/"));
        let ruta_logica = ruta_logica
            .strip_suffix(".unity")
            .unwrap_or(&ruta_logica)
            .to_string();
        let texto = std::fs::read_to_string(ruta).expect("leer script en cuarentena");
        let p = procesar_script(&ruta_logica, &texto, None);
        if p.destino != DestinoScript::Cuarentena {
            no_cuarentena += 1;
            continue;
        }
        let bloqueantes: Vec<_> = p.marcas.iter().filter(|m| m.bloqueante).collect();
        let mut simbolos_script: Vec<String> = Vec::new();
        let mut tiene_preprocesador = false;
        let mut tiene_otros = false;
        for m in &bloqueantes {
            *por_motivo.entry(m.motivo.clone()).or_default() += 1;
            if let Some(cond) = m.motivo.strip_prefix("símbolo de preprocesador no resoluble: ") {
                tiene_preprocesador = true;
                // Símbolos sueltos de la condición (sin !, &&, || ni paréntesis).
                for token in cond.split(['&', '|', '(', ')']) {
                    let s = token.trim().trim_start_matches('!').trim();
                    if !s.is_empty() && s != "true" && s != "false" {
                        simbolos_script.push(s.to_string());
                    }
                }
            } else {
                tiene_otros = true;
            }
        }
        simbolos_script.sort();
        simbolos_script.dedup();
        for s in simbolos_script {
            *por_simbolo.entry(s).or_default() += 1;
        }
        if tiene_preprocesador && !tiene_otros {
            solo_preprocesador += 1;
        } else {
            con_otros_bloqueos += 1;
        }
    }

    println!("\n== resumen ==");
    println!("solo bloqueados por preprocesador: {solo_preprocesador}");
    println!("con otros bloqueos (además o en vez): {con_otros_bloqueos}");
    println!("ya no caen en cuarentena: {no_cuarentena}");

    println!("\n== motivos bloqueantes (nº de scripts) ==");
    let mut motivos: Vec<_> = por_motivo.iter().collect();
    motivos.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (motivo, n) in motivos {
        println!("{n:4}  {motivo}");
    }

    println!("\n== símbolos sueltos en condiciones no resolubles (nº de scripts) ==");
    let mut simbolos: Vec<_> = por_simbolo.iter().collect();
    simbolos.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (simbolo, n) in simbolos {
        println!("{n:4}  {simbolo}");
    }
}
