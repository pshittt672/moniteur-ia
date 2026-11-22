//! Compteurs de performance Windows (PDH) lisibles sans droits administrateur :
//! zone thermique ACPI (température approximative du processeur) et
//! utilisation du fichier d'échange.

#[cfg(windows)]
mod imp {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Performance::*;

    pub struct Compteurs {
        requete: PDH_HQUERY,
        thermique: Option<PDH_HCOUNTER>,
        echange: Option<PDH_HCOUNTER>,
        requete_lente: PDH_HQUERY,
        processus: Option<PDH_HCOUNTER>,
    }

    // Les poignées PDH ne sont utilisées que depuis le fil de mesure.
    unsafe impl Send for Compteurs {}

    fn large(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    impl Compteurs {
        pub fn nouveau() -> Option<Self> {
            unsafe {
                let mut requete: PDH_HQUERY = null_mut();
                if PdhOpenQueryW(std::ptr::null(), 0, &mut requete) != 0 {
                    return None;
                }
                let ajouter = |chemin: &str| {
                    let mut c: PDH_HCOUNTER = null_mut();
                    let w = large(chemin);
                    (PdhAddEnglishCounterW(requete, w.as_ptr(), 0, &mut c) == 0).then_some(c)
                };
                let thermique = ajouter("\\Thermal Zone Information(*)\\Temperature");
                let echange = ajouter("\\Paging File(_Total)\\% Usage");
                PdhCollectQueryData(requete);
                // Mémoire des processus vmmem (machine virtuelle de WSL / Docker) : requête à part, plus coûteuse
                let mut requete_lente: PDH_HQUERY = null_mut();
                let mut processus = None;
                if PdhOpenQueryW(std::ptr::null(), 0, &mut requete_lente) == 0 {
                    let mut c: PDH_HCOUNTER = null_mut();
                    let w = large("\\Process(*)\\Working Set");
                    if PdhAddEnglishCounterW(requete_lente, w.as_ptr(), 0, &mut c) == 0 {
                        processus = Some(c);
                    }
                }
                Some(Self { requete, thermique, echange, requete_lente, processus })
            }
        }

        fn valeurs(&self, c: PDH_HCOUNTER) -> Vec<f64> {
            self.valeurs_nommees(c).into_iter().map(|(_, v)| v).collect()
        }

        fn valeurs_nommees(&self, c: PDH_HCOUNTER) -> Vec<(String, f64)> {
            unsafe {
                let (mut taille, mut nombre) = (0u32, 0u32);
                let r = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut taille, &mut nombre, null_mut());
                if r != PDH_MORE_DATA || taille == 0 {
                    return vec![];
                }
                let elem = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                let mut tampon: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                    Vec::with_capacity((taille as usize).div_ceil(elem) + 1);
                let r = PdhGetFormattedCounterArrayW(
                    c,
                    PDH_FMT_DOUBLE,
                    &mut taille,
                    &mut nombre,
                    tampon.as_mut_ptr(),
                );
                if r != 0 {
                    return vec![];
                }
                tampon.set_len(nombre as usize);
                tampon
                    .iter()
                    .filter(|i| i.FmtValue.CStatus == 0)
                    .map(|i| {
                        let mut n = 0;
                        if !i.szName.is_null() {
                            while *i.szName.add(n) != 0 {
                                n += 1;
                            }
                        }
                        let nom = String::from_utf16_lossy(std::slice::from_raw_parts(i.szName, n));
                        (nom, i.FmtValue.Anonymous.doubleValue)
                    })
                    .collect()
            }
        }

        /// Renvoie (température en °C, % du fichier d'échange utilisé, octets des processus vmmem).
        pub fn lire(&mut self, lent: bool) -> (Option<f32>, Option<f32>, Option<u64>) {
            unsafe { PdhCollectQueryData(self.requete) };
            if lent && !self.requete_lente.is_null() {
                unsafe { PdhCollectQueryData(self.requete_lente) };
            }
            let temp = self.thermique.and_then(|c| {
                // Valeurs en kelvins ; on garde la zone la plus chaude plausible.
                self.valeurs(c)
                    .into_iter()
                    .map(|k| (k - 273.15) as f32)
                    .filter(|t| *t > 5.0 && *t < 125.0)
                    .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.max(t))))
            });
            let echange = self.echange.and_then(|c| self.valeurs(c).first().map(|v| *v as f32));
            let vmmem = self.processus.filter(|_| lent).and_then(|c| {
                let total: f64 = self
                    .valeurs_nommees(c)
                    .into_iter()
                    .filter(|(n, _)| n.to_ascii_lowercase().starts_with("vmmem"))
                    .map(|(_, v)| v)
                    .sum();
                (total > 0.0).then_some(total as u64)
            });
            (temp, echange, vmmem)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct Compteurs;
    impl Compteurs {
        pub fn nouveau() -> Option<Self> {
            Some(Self)
        }
        pub fn lire(&mut self, _lent: bool) -> (Option<f32>, Option<f32>, Option<u64>) {
            (None, None, None)
        }
    }
}

pub use imp::Compteurs;
