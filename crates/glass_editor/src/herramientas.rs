//! Registro de herramientas del espacio «Herramientas».
//!
//! La lista de herramientas del espacio ya no está escrita a mano en el
//! panel: sale de este registro. Cada herramienta declara su portón (el
//! complemento que la habilita) y solo aparece si ese portón pasa. Las
//! herramientas futuras (vista de huesos/auto-rigging, visor de Ripper)
//! se añaden aquí y nacen ya dentro del espacio.

/// Ficha de una herramienta del espacio «Herramientas».
#[derive(Debug, Clone)]
pub struct HerramientaInfo {
    /// Identificador estable (se guarda como herramienta activa).
    pub id: &'static str,
    /// Nombre visible en la interfaz.
    pub nombre: &'static str,
    /// Icono que acompaña al nombre.
    pub icono: &'static str,
    /// Descripción corta (texto de ayuda).
    pub descripcion: &'static str,
    /// Complemento que hace de portón; None = siempre disponible.
    pub plugin_id: Option<&'static str>,
}

/// Todas las herramientas conocidas por el motor, estén o no disponibles.
pub fn registro_herramientas() -> Vec<HerramientaInfo> {
    vec![HerramientaInfo {
        id: "meshforge",
        nombre: "MeshForge",
        icono: "🧊",
        descripcion: "Editor de mallas (.glb)",
        plugin_id: Some("herramienta-3d"),
    }]
}

/// Herramientas del registro cuyo portón pasa, con el estado de los
/// complementos leído en vivo de disco (como hace el menú Window).
pub fn herramientas_disponibles() -> Vec<HerramientaInfo> {
    disponibles_con(crate::plugins::plugin_enabled)
}

/// Filtra el registro con el portón dado. Es la versión testeable de
/// `herramientas_disponibles`: el portón recibe el `plugin_id` y dice si
/// el complemento está activo. Las herramientas sin portón pasan siempre.
pub fn disponibles_con(porton: impl Fn(&str) -> bool) -> Vec<HerramientaInfo> {
    registro_herramientas()
        .into_iter()
        .filter(|h| h.plugin_id.is_none_or(&porton))
        .collect()
}

/// Resuelve la herramienta activa: la pedida si está disponible; si no,
/// la primera disponible; None si no hay ninguna.
pub fn resolver_activa<'a>(
    disponibles: &'a [HerramientaInfo],
    pedida: &str,
) -> Option<&'a HerramientaInfo> {
    disponibles
        .iter()
        .find(|h| h.id == pedida)
        .or_else(|| disponibles.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ficha(id: &'static str) -> HerramientaInfo {
        HerramientaInfo {
            id,
            nombre: id,
            icono: "",
            descripcion: "",
            plugin_id: None,
        }
    }

    #[test]
    fn porton_apagado_deja_la_lista_vacia() {
        let lista = disponibles_con(|p| p != "herramienta-3d");
        assert!(lista.is_empty());
    }

    #[test]
    fn porton_abierto_muestra_meshforge() {
        let lista = disponibles_con(|_| true);
        assert!(lista.iter().any(|h| h.id == "meshforge"));
    }

    #[test]
    fn resolver_con_la_pedida_presente() {
        let lista = vec![ficha("a"), ficha("b")];
        assert_eq!(resolver_activa(&lista, "b").map(|h| h.id), Some("b"));
    }

    #[test]
    fn resolver_con_la_pedida_ausente_cae_a_la_primera() {
        let lista = vec![ficha("a"), ficha("b")];
        assert_eq!(resolver_activa(&lista, "zzz").map(|h| h.id), Some("a"));
    }

    #[test]
    fn resolver_con_lista_vacia() {
        let lista: Vec<HerramientaInfo> = Vec::new();
        assert!(resolver_activa(&lista, "meshforge").is_none());
    }
}
