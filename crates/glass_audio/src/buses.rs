#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusId {
    Master,
    Music,
    Sfx,
}

impl BusId {
    pub fn index(self) -> usize {
        match self {
            BusId::Master => 0,
            BusId::Music => 1,
            BusId::Sfx => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BusId::Master => "master",
            BusId::Music => "music",
            BusId::Sfx => "sfx",
        }
    }

    pub fn from_name(s: &str) -> Option<BusId> {
        if s.eq_ignore_ascii_case("master") {
            Some(BusId::Master)
        } else if s.eq_ignore_ascii_case("music") {
            Some(BusId::Music)
        } else if s.eq_ignore_ascii_case("sfx") {
            Some(BusId::Sfx)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Duck {
    start: f64,
    duration: f64,
    amount: f32,
    /// Factor en el instante de arranque: la rampa de bajada parte de
    /// aquí, no de 1.0, para que un duck solapado no salte
    /// (auditoría M-20).
    from: f32,
}

#[derive(Debug, Clone)]
pub struct BusSnapshot {
    pub name: &'static str,
    pub volume: f32,
    pub effective_volume: f32,
    pub ducking: bool,
    pub duck_factor: f32,
}

pub struct BusMixer {
    volumes: [f32; 3],
    ducks: [Option<Duck>; 3],
}

impl BusMixer {
    pub fn new() -> Self {
        Self {
            volumes: [1.0; 3],
            ducks: [None; 3],
        }
    }

    pub fn set_volume(&mut self, bus: BusId, volume: f32) {
        // Auditoría (menor): un NaN se ignoraba a medias — clamp lo
        // dejaba pasar y el bus quedaba en silencio/ruido.
        if volume.is_nan() {
            return;
        }
        self.volumes[bus.index()] = volume.clamp(0.0, 1.0);
    }

    pub fn volume(&self, bus: BusId) -> f32 {
        self.volumes[bus.index()]
    }

    pub fn duck(&mut self, bus: BusId, amount: f32, duration_secs: f32, now: f64) {
        if bus == BusId::Master {
            return;
        }
        // Auditoría (menores): una duración no positiva o no finita y
        // un amount NaN se ignoran — antes una duración 0 CANCELABA
        // el duck activo con un salto a 1.0.
        if !duration_secs.is_finite() || duration_secs <= 0.0 || amount.is_nan() {
            return;
        }
        // Continuidad (M-20): el nuevo duck parte del factor actual.
        let from = self.duck_factor(bus, now);
        self.ducks[bus.index()] = Some(Duck {
            start: now,
            duration: duration_secs as f64,
            amount: amount.clamp(0.0, 1.0),
            from,
        });
    }

    pub fn duck_factor(&self, bus: BusId, now: f64) -> f32 {
        let Some(duck) = self.ducks[bus.index()] else {
            return 1.0;
        };

        let t = now - duck.start;
        if t < 0.0 {
            return duck.from;
        }
        if duck.duration <= 0.0 || t >= duck.duration {
            return 1.0;
        }

        // Auditoría M-21: las rampas se recortan en proporción a la
        // duración (antes, con duración < 0,65 s, había escalones).
        let down_ramp = 0.25f64.min(duck.duration * 0.4);
        let up_ramp = 0.4f64.min(duck.duration * 0.4);

        if t < down_ramp {
            let progress = (t / down_ramp) as f32;
            return duck.from + (duck.amount - duck.from) * progress;
        }

        let plateau_end = duck.duration - up_ramp;
        if t < plateau_end {
            return duck.amount;
        }

        if t < duck.duration {
            let progress = ((t - plateau_end) / up_ramp) as f32;
            return duck.amount + (1.0 - duck.amount) * progress;
        }

        1.0
    }

    pub fn is_ducking(&self, bus: BusId, now: f64) -> bool {
        match self.ducks[bus.index()] {
            Some(duck) => now >= duck.start && now < duck.start + duck.duration,
            None => false,
        }
    }

    pub fn effective(&self, bus: BusId, now: f64) -> f32 {
        self.volume(bus) * self.duck_factor(bus, now)
    }

    pub fn snapshot(&self, now: f64) -> [BusSnapshot; 3] {
        [
            self.snapshot_for(BusId::Master, now),
            self.snapshot_for(BusId::Music, now),
            self.snapshot_for(BusId::Sfx, now),
        ]
    }

    fn snapshot_for(&self, bus: BusId, now: f64) -> BusSnapshot {
        // Auditoría (menor): el efectivo del snapshot es el PERCIBIDO:
        // incluye el volumen de Master para Music/Sfx (el método
        // `effective` sigue sin Master porque el backend ya lo aplica
        // en la pista principal; incluirlo ahí lo contaría dos veces).
        let percibido = if bus == BusId::Master {
            self.effective(bus, now)
        } else {
            self.effective(bus, now) * self.volume(BusId::Master)
        };
        BusSnapshot {
            name: bus.name(),
            volume: self.volume(bus),
            effective_volume: percibido,
            ducking: self.is_ducking(bus, now),
            duck_factor: self.duck_factor(bus, now),
        }
    }
}

impl Default for BusMixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumen_por_defecto_y_clamp() {
        let mut mixer = BusMixer::new();
        assert_eq!(mixer.volume(BusId::Master), 1.0);
        assert_eq!(mixer.volume(BusId::Music), 1.0);
        assert_eq!(mixer.volume(BusId::Sfx), 1.0);
        mixer.set_volume(BusId::Music, -0.25);
        mixer.set_volume(BusId::Sfx, 1.25);
        assert_eq!(mixer.volume(BusId::Music), 0.0);
        assert_eq!(mixer.volume(BusId::Sfx), 1.0);
    }

    #[test]
    fn duck_en_la_meseta_baja_el_efectivo() {
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Music, 0.2, 6.0, 10.0);
        let factor = mixer.duck_factor(BusId::Music, 12.0);
        assert!((factor - 0.2).abs() < 0.01, "factor={factor}");
        let efectivo = mixer.effective(BusId::Music, 12.0);
        assert!((efectivo - 0.2).abs() < 0.01, "efectivo={efectivo}");
        assert!(mixer.is_ducking(BusId::Music, 12.0));
    }

    #[test]
    fn duck_expirado_vuelve_a_uno() {
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Music, 0.2, 6.0, 10.0);
        assert_eq!(mixer.duck_factor(BusId::Music, 20.0), 1.0);
        assert!(!mixer.is_ducking(BusId::Music, 20.0));
    }

    #[test]
    fn duck_en_master_no_hace_nada() {
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Master, 0.1, 5.0, 0.0);
        assert_eq!(mixer.duck_factor(BusId::Master, 1.0), 1.0);
        assert_eq!(mixer.effective(BusId::Master, 1.0), 1.0);
    }

    #[test]
    fn from_name_case_insensitive() {
        assert_eq!(BusId::from_name("music"), Some(BusId::Music));
        assert_eq!(BusId::from_name("SFX"), Some(BusId::Sfx));
        assert_eq!(BusId::from_name("Master"), Some(BusId::Master));
        assert_eq!(BusId::from_name("otro"), None);
    }

    #[test]
    fn snapshot_refleja_volumen_y_duck() {
        let mut mixer = BusMixer::new();
        mixer.set_volume(BusId::Sfx, 0.5);
        mixer.duck(BusId::Music, 0.25, 8.0, 0.0);
        let snap = mixer.snapshot(2.0);
        assert_eq!(snap[0].name, "master");
        assert_eq!(snap[1].name, "music");
        assert!((snap[1].effective_volume - 0.25).abs() < 0.01);
        assert!(snap[1].ducking);
        assert_eq!(snap[2].volume, 0.5);
        assert_eq!(snap[2].effective_volume, 0.5);
    }

    #[test]
    fn duck_solapado_no_salta() {
        // Auditoría M-20: el segundo duck parte del factor vigente.
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Music, 0.2, 6.0, 0.0);
        let antes = mixer.duck_factor(BusId::Music, 2.0);
        assert!((antes - 0.2).abs() < 0.01);
        mixer.duck(BusId::Music, 0.6, 4.0, 2.0);
        let despues = mixer.duck_factor(BusId::Music, 2.0);
        assert!((despues - antes).abs() < 0.001, "salto: {antes} -> {despues}");
        // Y baja hacia el nuevo amount sin pasar por 1.0.
        let mitad = mixer.duck_factor(BusId::Music, 2.1);
        assert!(mitad > 0.2 && mitad < 0.6, "mitad={mitad}");
    }

    #[test]
    fn duck_corto_sin_escalones() {
        // Auditoría M-21: duración 0,5 s, muestreo fino sin saltos.
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Sfx, 0.3, 0.5, 0.0);
        let mut prev = 1.0f32;
        let mut min = 1.0f32;
        for i in 0..=50 {
            let f = mixer.duck_factor(BusId::Sfx, i as f64 * 0.01);
            assert!((f - prev).abs() < 0.1, "escalón en {}: {prev} -> {f}", i as f64 * 0.01);
            min = min.min(f);
            prev = f;
        }
        assert!(min < 0.45, "el duck corto debe bajar de verdad: min={min}");
        assert_eq!(mixer.duck_factor(BusId::Sfx, 0.6), 1.0);
    }

    #[test]
    fn duck_cero_no_cancela_y_nan_se_ignora() {
        let mut mixer = BusMixer::new();
        mixer.duck(BusId::Music, 0.2, 6.0, 0.0);
        mixer.duck(BusId::Music, 0.9, 0.0, 1.0); // ignorado
        assert!((mixer.duck_factor(BusId::Music, 2.0) - 0.2).abs() < 0.01);
        mixer.set_volume(BusId::Music, f32::NAN); // ignorado
        assert_eq!(mixer.volume(BusId::Music), 1.0);
        mixer.duck(BusId::Music, f32::NAN, 3.0, 2.0); // ignorado
        assert!((mixer.duck_factor(BusId::Music, 3.0) - 0.2).abs() < 0.01);
    }

    #[test]
    fn snapshot_efectivo_incluye_master() {
        let mut mixer = BusMixer::new();
        mixer.set_volume(BusId::Master, 0.5);
        mixer.set_volume(BusId::Music, 0.8);
        let snap = mixer.snapshot(0.0);
        assert!((snap[1].effective_volume - 0.4).abs() < 0.001);
        assert!((snap[0].effective_volume - 0.5).abs() < 0.001);
        // El effective() de backend NO incluye Master (lo aplica la
        // pista principal): sigue siendo el del bus.
        assert!((mixer.effective(BusId::Music, 0.0) - 0.8).abs() < 0.001);
    }
}
