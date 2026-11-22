//! Relevés du matériel (toutes les secondes) et de l'activité des outils IA
//! (toutes les deux secondes), chacun dans son propre fil.

use crate::compteurs::Compteurs;
use crate::ia;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
    UpdateKind,
};

#[derive(Clone, Default)]
pub struct Gpu {
    pub nom: String,
    pub util: u32,
    pub temp: Option<u32>,
    pub puissance_w: Option<f32>,
    pub limite_w: Option<f32>,
    pub horloge_mhz: Option<u32>,
    pub vram_util: u64,
    pub vram_total: u64,
}

#[derive(Clone, Default)]
pub struct Processus {
    pub nom: String,
    pub cpu: f32,
    pub vram: Option<u64>,
    pub ram: u64,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Niveau {
    #[default]
    Arrete,
    Inactif,
    Actif,
    Alerte,
}

#[derive(Clone, Default)]
pub struct Outil {
    pub niveau: Niveau,
    pub progression: Option<f32>,
    pub detail: String,
    pub etat: String,
    pub conseil: String,
    /// Nombre de tâches dans la file (WanGP), si lisible.
    pub file: Option<usize>,
}

#[derive(Clone, Default)]
pub struct Etat {
    pub pret: bool,
    pub gpu: Option<Gpu>,
    pub gpu_erreur: String,
    pub cpu: f32,
    pub threads: usize,
    pub cpu_temp: Option<f32>,
    pub ram_util: u64,
    pub ram_total: u64,
    pub echange_util: Option<u64>,
    pub docker_ram: Option<u64>,
    pub processus: Vec<Processus>,
    // Indices pour le fil des outils IA
    pub wangp_gpu: Option<f32>,
    pub wangp_lance: bool,
    pub wangp_pids: Vec<u32>,
    pub lmstudio_lance: bool,
    pub wangp: Outil,
    pub forge: Outil,
    pub lmstudio: Outil,
    pub duree_rapide_ms: f32,
    pub duree_lente_ms: f32,
}

pub type Partage = Arc<Mutex<Etat>>;

/// Nom lisible d'un processus ; repère WanGP parmi les python.exe.
fn nom_affiche(nom: &str, cmd: &[std::ffi::OsString]) -> String {
    let n = nom.trim_end_matches(".exe").trim_end_matches(".EXE");
    if n.eq_ignore_ascii_case("python") || n.eq_ignore_ascii_case("pythonw") {
        if cmd.iter().any(|a| a.to_string_lossy().contains("wgp.py")) {
            return "python (WanGP)".into();
        }
    }
    if n.eq_ignore_ascii_case("vmmem") || n.eq_ignore_ascii_case("vmmemWSL") {
        return "Docker / WSL (vmmem)".into();
    }
    n.to_string()
}

pub fn demarrer(partage: Partage, ctx: egui::Context) {
    if std::env::var_os("MONITEUR_DEMO").is_some() {
        *partage.lock().unwrap() = demo();
        return;
    }
    // Fil matériel
    {
        let partage = partage.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("materiel".into())
            .spawn(move || fil_materiel(partage, ctx))
            .ok();
    }
    // Fil des outils IA
    std::thread::Builder::new()
        .name("outils-ia".into())
        .spawn(move || {
            let mut wangp = ia::WanGP::default();
            loop {
                let (lance, gpu, lm) = {
                    let e = partage.lock().unwrap();
                    (e.wangp_lance, e.wangp_gpu, e.lmstudio_lance)
                };
                let w = wangp.lire(lance, gpu);
                let f = ia::forge();
                let l = ia::lmstudio(lm);
                {
                    let mut e = partage.lock().unwrap();
                    let video = w.niveau == Niveau::Actif;
                    e.wangp = w;
                    e.forge = f;
                    e.lmstudio = l;
                    if video && e.lmstudio.niveau != Niveau::Arrete {
                        e.lmstudio.conseil = "à fermer pendant la vidéo".into();
                    }
                    if video && e.forge.niveau != Niveau::Arrete {
                        e.forge.conseil = "occupe RAM et VRAM".into();
                    }
                }
                // Pas de rafraîchissement ici : le fil matériel redessine chaque seconde.
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .ok();
}

pub struct Releveur {
    nvml: Result<nvml_wrapper::Nvml, nvml_wrapper::error::NvmlError>,
    sys: System,
    compteurs: Option<Compteurs>,
    tour: u32,
    lent: Lent,
}

/// Relevés coûteux (liste des processus, mémoire de WSL), faits toutes les 5 s seulement.
#[derive(Clone, Default)]
struct Lent {
    processus: Vec<Processus>,
    wangp_lance: bool,
    wangp_pids: Vec<u32>,
    lmstudio_lance: bool,
    docker_ram: Option<u64>,
    duree_ms: f32,
}

impl Releveur {
    pub fn nouveau() -> Option<Self> {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_usage())
                .with_memory(MemoryRefreshKind::everything()),
        )
        .ok()?;
        Some(Self { nvml: nvml_wrapper::Nvml::init(), sys, compteurs: Compteurs::nouveau(), tour: 0, lent: Lent::default() })
    }

    pub fn releve(&mut self) -> Etat {
        let rafraichir = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_cmd(UpdateKind::OnlyIfNotSet);
        let debut = std::time::Instant::now();
        let lent = self.tour % 5 == 0;
        self.tour = self.tour.wrapping_add(1);
        let (nvml, sys, compteurs) = (&self.nvml, &mut self.sys, &mut self.compteurs);
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let mut e = Etat {
            pret: true,
            cpu: sys.global_cpu_usage(),
            threads: sys.cpus().len(),
            ram_total: sys.total_memory(),
            ram_util: sys.total_memory().saturating_sub(sys.available_memory()),
            ..Default::default()
        };

        // GPU NVIDIA
        match nvml {
            Ok(n) => match n.device_by_index(0) {
                Ok(d) => {
                    let u = d.utilization_rates().ok();
                    let m = d.memory_info().ok();
                    e.gpu = Some(Gpu {
                        nom: d.name().unwrap_or_default().replace("NVIDIA GeForce ", ""),
                        util: u.as_ref().map_or(0, |u| u.gpu),
                        temp: d
                            .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                            .ok(),
                        puissance_w: d.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                        limite_w: d.enforced_power_limit().ok().map(|mw| mw as f32 / 1000.0),
                        horloge_mhz: d
                            .clock_info(nvml_wrapper::enum_wrappers::device::Clock::Graphics)
                            .ok(),
                        vram_util: m.as_ref().map_or(0, |m| m.used),
                        vram_total: m.as_ref().map_or(0, |m| m.total),
                    });
                }
                Err(err) => e.gpu_erreur = format!("GPU introuvable ({err})"),
            },
            Err(err) => e.gpu_erreur = format!("Pilote NVIDIA non lisible ({err})"),
        }

        // Compteurs Windows
        if let Some(c) = compteurs.as_mut() {
            let (t, pct, vmmem) = c.lire(lent);
            e.cpu_temp = t;
            if lent {
                self.lent.docker_ram = vmmem;
            }
            let taille = sys.total_swap(); // taille du fichier d'échange
            e.echange_util = pct.map(|p| (taille as f64 * p as f64 / 100.0) as u64);
        }

        e.duree_rapide_ms = debut.elapsed().as_secs_f32() * 1000.0;
        if lent {
            let t0 = std::time::Instant::now();
            let vmmem = self.lent.docker_ram;
            // VRAM par processus lue auprès du pilote NVIDIA (souvent indisponible sous Windows)
            let mut vram: HashMap<u32, u64> = HashMap::new();
            if let Ok(d) = nvml.as_ref().map_err(|_| ()).and_then(|n| n.device_by_index(0).map_err(|_| ())) {
                for p in d.running_compute_processes().unwrap_or_default().into_iter().chain(d.running_graphics_processes().unwrap_or_default()) {
                    if let nvml_wrapper::enums::device::UsedGpuMemory::Used(o) = p.used_gpu_memory {
                        vram.insert(p.pid, o);
                    }
                }
            }
            self.lent = Self::releve_lent(sys, e.threads, rafraichir, &vram);
            self.lent.docker_ram = vmmem.or(self.lent.docker_ram);
            self.lent.duree_ms = t0.elapsed().as_secs_f32() * 1000.0;
        }
        let l = self.lent.clone();
        e.processus = l.processus;
        e.wangp_lance = l.wangp_lance;
        e.wangp_pids = l.wangp_pids;
        // Activité de WanGP estimée par l'utilisation globale du GPU
        e.wangp_gpu = if l.wangp_lance { e.gpu.as_ref().map(|g| g.util as f32) } else { None };
        e.lmstudio_lance = l.lmstudio_lance;
        e.docker_ram = l.docker_ram;
        e.duree_lente_ms = l.duree_ms;
        e
    }

    fn releve_lent(sys: &mut System, threads: usize, rafraichir: ProcessRefreshKind, vram: &HashMap<u32, u64>) -> Lent {
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, rafraichir);
        let mut e = Lent::default();
        // Processus regroupés par nom
        let mut groupes: HashMap<String, Processus> = HashMap::new();
        for p in sys.processes().values() {
            let nom = nom_affiche(&p.name().to_string_lossy(), p.cmd());
            if nom == "python (WanGP)" {
                e.wangp_pids.push(p.pid().as_u32());
            }
            if nom.eq_ignore_ascii_case("System Idle Process") || nom == "Idle" {
                continue;
            }
            let g = groupes.entry(nom.clone()).or_insert_with(|| Processus {
                nom: nom.clone(),
                ..Default::default()
            });
            g.cpu += p.cpu_usage();
            g.ram += p.memory();
            if let Some(v) = vram.get(&p.pid().as_u32()) {
                g.vram = Some(g.vram.unwrap_or(0) + v);
            }
        }
        let n = threads.max(1) as f32;
        let mut liste: Vec<Processus> = groupes
            .into_values()
            .map(|mut p| {
                p.cpu /= n; // même échelle que le Gestionnaire des tâches
                p
            })
            .collect();
        if liste.iter().any(|p| p.nom == "python (WanGP)") {
            e.wangp_lance = true;
        }
        e.lmstudio_lance = liste.iter().any(|p| p.nom.eq_ignore_ascii_case("LM Studio"));
        e.docker_ram = liste
            .iter()
            .find(|p| p.nom.starts_with("Docker / WSL"))
            .map(|p| p.ram)
            .filter(|r| *r > 0);
        liste.sort_by(|a, b| {
            let cle = |p: &Processus| p.vram.unwrap_or(0) as f64 * 2.0 + p.ram as f64;
            cle(b).partial_cmp(&cle(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        liste.truncate(5);
        e.processus = liste;

        e
    }
}

fn fil_materiel(partage: Partage, ctx: egui::Context) {
    let Some(mut r) = Releveur::nouveau() else { return };
    loop {
        let mut e = r.releve();
        {
            let mut g = partage.lock().unwrap();
            e.wangp = std::mem::take(&mut g.wangp);
            e.forge = std::mem::take(&mut g.forge);
            e.lmstudio = std::mem::take(&mut g.lmstudio);
            *g = e;
        }
        ctx.request_repaint();
        std::thread::sleep(Duration::from_millis(1000));
    }
}

/// Données fictives pour vérifier l'affichage (variable MONITEUR_DEMO).
fn demo() -> Etat {
    let go = |x: f64| (x * 1_073_741_824.0) as u64;
    let pr = |nom: &str, cpu, _gpu: Option<f32>, vram: Option<f64>, ram| Processus {
        nom: nom.into(), cpu, vram: vram.map(go), ram: go(ram),
    };
    Etat {
        pret: true,
        gpu: Some(Gpu {
            nom: "RTX 4060 Laptop GPU".into(), util: 100, temp: Some(72),
            puissance_w: Some(64.2), limite_w: Some(65.0), horloge_mhz: Some(2145),
            vram_util: go(7.3), vram_total: go(8.0),
        }),
        gpu_erreur: String::new(),
        duree_rapide_ms: 0.0, duree_lente_ms: 0.0,
        cpu: 18.0, threads: 16, cpu_temp: None,
        ram_util: go(30.1), ram_total: go(31.6), echange_util: Some(go(1.9)), docker_ram: Some(go(6.2)),
        processus: vec![
            pr("python (WanGP)", 3.0, Some(100.0), Some(6.9), 10.9),
            pr("Docker / WSL (vmmem)", 1.0, Some(0.0), None, 6.2),
            pr("LM Studio", 0.0, Some(0.0), Some(0.1), 0.6),
            pr("Pinokio", 2.0, Some(1.0), Some(0.1), 0.5),
            pr("chrome", 1.0, Some(0.0), Some(0.2), 1.4),
        ],
        wangp_gpu: Some(100.0), wangp_lance: true, wangp_pids: vec![], lmstudio_lance: true,
        wangp: Outil { niveau: Niveau::Actif, progression: Some(0.62), detail: "étape 5/8 · file : 2 tâches · GPU 100 %".into(), etat: "En cours".into(), conseil: String::new(), file: None },
        forge: Outil { niveau: Niveau::Arrete, progression: None, detail: "API non joignable (conteneur arrêté ?)".into(), etat: "Arrêté".into(), conseil: String::new(), file: None },
        lmstudio: Outil { niveau: Niveau::Inactif, progression: None, detail: "Ouvert · aucun modèle chargé · serveur :1234".into(), etat: "Ouvert (inactif)".into(), conseil: "à fermer pendant la vidéo".into(), file: None },
    }
}
