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
        self.volumes[bus.index()] = volume.clamp(0.0, 1.0);
    }

    pub fn volume(&self, bus: BusId) -> f32 {
        self.volumes[bus.index()]
    }

    pub fn duck(&mut self, bus: BusId, amount: f32, duration_secs: f32, now: f64) {
        if bus == BusId::Master {
            return;
        }

        self.ducks[bus.index()] = Some(Duck {
            start: now,
            duration: duration_secs.max(0.0) as f64,
            amount: amount.clamp(0.0, 1.0),
        });
    }

    pub fn duck_factor(&self, bus: BusId, now: f64) -> f32 {
        const DOWN_RAMP: f64 = 0.25;
        const UP_RAMP: f64 = 0.4;

        let Some(duck) = self.ducks[bus.index()] else {
            return 1.0;
        };

        let t = now - duck.start;
        if t < 0.0 {
            return 1.0;
        }
        if duck.duration <= 0.0 || t >= duck.duration {
            return 1.0;
        }

        if t < DOWN_RAMP {
            let progress = (t / DOWN_RAMP) as f32;
            return 1.0 + (duck.amount - 1.0) * progress;
        }

        let plateau_end = duck.duration - UP_RAMP;
        if t < plateau_end {
            return duck.amount;
        }

        if t < duck.duration {
            let progress = ((t - plateau_end) / UP_RAMP) as f32;
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
        BusSnapshot {
            name: bus.name(),
            volume: self.volume(bus),
            effective_volume: self.effective(bus, now),
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
}
